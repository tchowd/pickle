//! Windows capture runs on an MTA worker so an unresponsive UI Automation provider
//! cannot permanently block Rune's UI loop. No clipboard synthesis or elevated access.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_NOT_FOUND, HWND},
        Security::Credentials::*,
        System::{Com::*, Threading::*},
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
    core::{Interface, PCWSTR, PWSTR},
};

fn error(message: &str) -> PlatformError {
    PlatformError(message.into())
}

/// A native dialog independent of Rune/D3D and any reader window. Fixed strings
/// only: arbitrary rendering errors and source content never enter the dialog.
pub fn present_reader_failure() -> Result<(), PlatformError> {
    let title = wide(READER_FAILURE_TITLE)?;
    let message = wide(READER_FAILURE_MESSAGE)?;
    let result = unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR | MB_TASKMODAL,
        )
    };
    if result.0 == 0 {
        Err(error("The native error dialog is unavailable."))
    } else {
        Ok(())
    }
}
fn wide(text: &str) -> Result<Vec<u16>, PlatformError> {
    if text.contains('\0') {
        return Err(error(
            "A native setting contains an invalid null character.",
        ));
    }
    Ok(text.encode_utf16().chain(Some(0)).collect())
}
/// Metadata only: no UIA, selection, URL, screenshot, audio, or prompts.
/// Worker callers impose a wait deadline and the shared resolver flight gate.
pub(crate) fn resolve_browser_target(expected: &str) -> Result<CapturedSelection, PlatformError> {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return Err(error("No foreground browser window is available."));
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
    if pid == 0
        || pid == unsafe { GetCurrentProcessId() }
        || !unsafe { IsWindowVisible(foreground) }.as_bool()
    {
        return Err(error("The foreground browser window is unavailable."));
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|_| error("The foreground browser identity is unavailable."))?;
    let mut path = vec![0u16; 32768];
    let mut size = path.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.map_err(|_| error("The foreground browser identity is unavailable."))?;
    let executable = String::from_utf16(&path[..size as usize])
        .map_err(|_| error("The foreground browser has an invalid executable identity."))?;
    let source = CapturedSelection {
        text: String::new(),
        app_name: "Browser".into(),
        app_id: format!("windows:{executable}"),
        bounds: None,
        source_url: None,
        source_pid: pid,
        source_window: Some(foreground.0 as u64),
    };
    if !super::browser_target::browser_target_matches(expected, &source.app_id) {
        return Err(error(
            "The foreground window does not belong to the registered browser.",
        ));
    }
    validate_source_window(&source)?;
    if unsafe { GetForegroundWindow() } != foreground
        || !unsafe { IsWindowVisible(foreground) }.as_bool()
    {
        return Err(error(
            "The foreground browser changed during source resolution.",
        ));
    }
    Ok(source)
}
pub fn capabilities() -> Capabilities {
    Capabilities {
        selection: true,
        menu_bar: true,
        modifier_gesture: false,
        window_capture_ocr: super::windows_context::has_package_identity(),
        source_audio: local_transcriber_config().is_some(),
    }
}
/// Windows saved desktop positions are physical: keep the origin unchanged and
/// use the returned monitor scale only for the saved logical content size.
pub fn restore_work_area(x: i32, y: i32, saved_scale: f64) -> Option<(WorkArea, f64)> {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint},
        UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
    };
    if !saved_scale.is_finite() || saved_scale <= 0.0 {
        return None;
    }
    let area = window_work_area(None, x, y)?;
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    let (mut dpi_x, mut dpi_y) = (0, 0);
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.ok()?;
    if dpi_x == 0 || dpi_y == 0 {
        return None;
    }
    Some((area, f64::from(dpi_x) / 96.0))
}

/// Rune initializes per-monitor DPI awareness before native windows are created.
/// Fallback coordinates must be WindowPlacement's physical desktop position.
pub fn window_work_area(_: Option<u32>, fallback_x: i32, fallback_y: i32) -> Option<WorkArea> {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    };
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: fallback_x,
                y: fallback_y,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        if monitor.0.is_null() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let width = i64::from(info.rcWork.right) - i64::from(info.rcWork.left);
        let height = i64::from(info.rcWork.bottom) - i64::from(info.rcWork.top);
        if width <= 0 || height <= 0 {
            return None;
        }
        Some(WorkArea {
            x: info.rcWork.left,
            y: info.rcWork.top,
            width: u32::try_from(width).ok()?,
            height: u32::try_from(height).ok()?,
        })
    }
}
pub fn permission_status() -> PermissionStatus {
    PermissionStatus {
        accessibility: true,
        screen_capture: true,
    }
}
pub fn request_accessibility() {}
pub fn request_screen_capture() {}
pub fn capture_page(source: &CapturedSelection) -> Result<CapturedPage, PlatformError> {
    super::windows_context::capture_page(source)
}
pub fn capture_page_with_cancel(
    source: &CapturedSelection,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<CapturedPage, PlatformError> {
    super::windows_context::capture_page_with_cancel(source, cancel)
}
pub fn record_audio(
    source: &CapturedSelection,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<String, PlatformError> {
    super::windows_audio::record_audio(source, cancel)
}

pub fn capture_selection(exclusions: &[String]) -> Result<CapturedSelection, PlatformError> {
    static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return Err(error(
            "Select text in another application, or paste the passage.",
        ));
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
    if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
        return Err(error(
            "Select text in another application, or paste the passage.",
        ));
    }
    if CAPTURE_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(error(
            "A source application is still answering the previous capture. Paste the passage, or restart Pickle if it remains unavailable.",
        ));
    }
    let excluded = exclusions.to_vec();
    let deadline = Instant::now() + Duration::from_secs(2);
    let hwnd = foreground.0 as usize;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("pickle-uia".into())
        .spawn(move || {
            struct CaptureGuard;
            impl Drop for CaptureGuard {
                fn drop(&mut self) {
                    CAPTURE_ACTIVE.store(false, Ordering::Release)
                }
            }
            let _active = CaptureGuard;
            let result =
                unsafe { capture_on_worker(HWND(hwnd as *mut _), pid, &excluded, deadline) };
            let _ = sender.send(result);
        });
    if worker.is_err() {
        CAPTURE_ACTIVE.store(false, Ordering::Release);
        return Err(error(
            "Selection capture is temporarily unavailable. Paste the passage or retry.",
        ));
    }
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| error("The source did not answer in time. Try again or paste the passage."))?
}
unsafe fn capture_on_worker(
    hwnd: HWND,
    pid: u32,
    exclusions: &[String],
    deadline: Instant,
) -> Result<CapturedSelection, PlatformError> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|_| error("Could not initialize selection capture."))?;
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = ComGuard;
        let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid).map_err(|_|error("This application cannot be read. Elevated or protected applications may require pasting."))?;
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        let path_result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        path_result.map_err(|_| error("Could not identify the source application."))?;
        let executable = String::from_utf16(&path[..size as usize])
            .map_err(|_| error("The source application has an invalid identity."))?;
        let app_id = format!("windows:{}", executable.to_lowercase());
        if source_is_excluded(&app_id, exclusions) {
            return Err(error("This source application is excluded in Settings."));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| error("Windows UI Automation is unavailable."))?;
        let settings = automation
            .cast::<IUIAutomation2>()
            .map_err(|_| error("Bounded selection capture is unavailable. Paste the passage."))?;
        let walker = automation
            .RawViewWalker()
            .map_err(|_| error("Could not inspect source protection."))?;
        let app_name = std::path::Path::new(&executable)
            .file_stem()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Application".into());
        let mut source = CapturedSelection {
            text: String::new(),
            app_name,
            app_id,
            bounds: None,
            source_url: None,
            source_pid: pid,
            source_window: Some(hwnd.0 as u64),
        };
        let mut reader = NativeSelectionReader {
            ui: automation,
            settings,
            walker,
            source: &source,
            hwnd,
            deadline,
        };
        let text = read_bound_selection(&mut reader)?;
        source.text = text;
        validate_selection(source, exclusions)
    }
}

const SELECTION_ANCESTRY_LIMIT: usize = 16;
fn selection_request_ms(deadline: Instant, now: Instant) -> Result<u32, PlatformError> {
    if now >= deadline {
        return Err(error(
            "The source did not answer in time. Try again or paste the passage.",
        ));
    }
    Ok(deadline
        .saturating_duration_since(now)
        .as_millis()
        .clamp(1, 500) as u32)
}
fn origin_error() -> PlatformError {
    error(
        "The selected control could not be bound to the source window. Select the passage again or paste it.",
    )
}
/// Shared production ordering, with a small injectable native boundary for
/// guard tests. No Name, Value, document text, or HWND on virtual descendants
/// is required to prove raw ancestry to the retained source-window root.
trait SelectionReader {
    type Node: Clone;
    fn validate_source(&mut self) -> Result<(), PlatformError>;
    fn root(&mut self) -> Result<Self::Node, PlatformError>;
    fn focused(&mut self) -> Result<Self::Node, PlatformError>;
    fn same(&mut self, left: &Self::Node, right: &Self::Node) -> Result<bool, PlatformError>;
    fn password(&mut self, node: &Self::Node) -> Result<bool, PlatformError>;
    fn parent(&mut self, node: &Self::Node) -> Result<Self::Node, PlatformError>;
    fn selected_text(&mut self, node: &Self::Node) -> Result<String, PlatformError>;
}
fn prove_selection_root<R: SelectionReader>(
    reader: &mut R,
    root: &R::Node,
    focused: &R::Node,
) -> Result<(), PlatformError> {
    let mut ancestor = focused.clone();
    for index in 0..SELECTION_ANCESTRY_LIMIT {
        // Password checks include the focused control and exact window root.
        if reader.password(&ancestor)? {
            return Err(error(
                "Pickle does not capture password or protected fields.",
            ));
        }
        if reader.same(&ancestor, root)? {
            return Ok(());
        }
        if index + 1 == SELECTION_ANCESTRY_LIMIT {
            break;
        }
        // An error/null end before reaching this root is a refusal, never an
        // excuse to proceed. Cycles also fail at the bounded depth limit.
        ancestor = reader.parent(&ancestor)?;
    }
    Err(origin_error())
}
fn read_bound_selection<R: SelectionReader>(reader: &mut R) -> Result<String, PlatformError> {
    reader.validate_source()?;
    let root = reader.root()?;
    let focused = reader.focused()?;
    prove_selection_root(reader, &root, &focused)?;
    // No TextPattern/GetSelection/GetText request occurs before the proof.
    let text = reader.selected_text(&focused)?;
    reader.validate_source()?;
    let fresh_root = reader.root()?;
    let fresh_focused = reader.focused()?;
    if !reader.same(&root, &fresh_root)? || !reader.same(&focused, &fresh_focused)? {
        return Err(origin_error());
    }
    prove_selection_root(reader, &fresh_root, &fresh_focused)?;
    reader.validate_source()?;
    Ok(text)
}
struct NativeSelectionReader<'a> {
    ui: IUIAutomation,
    settings: IUIAutomation2,
    walker: IUIAutomationTreeWalker,
    source: &'a CapturedSelection,
    hwnd: HWND,
    deadline: Instant,
}
impl NativeSelectionReader<'_> {
    fn check(&self) -> Result<(), PlatformError> {
        if Instant::now() >= self.deadline {
            return Err(error(
                "The source did not answer in time. Try again or paste the passage.",
            ));
        }
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(self.hwnd, Some(&mut pid));
        }
        if pid == 0
            || pid != self.source.source_pid
            || unsafe { GetForegroundWindow() } != self.hwnd
        {
            return Err(origin_error());
        }
        Ok(())
    }
    fn remaining_request_ms(&self) -> Result<u32, PlatformError> {
        self.check()?;
        selection_request_ms(self.deadline, Instant::now())
    }
    fn rpc<T>(
        &self,
        work: impl FnOnce() -> ::windows::core::Result<T>,
    ) -> Result<T, PlatformError> {
        unsafe {
            self.settings
                .SetConnectionTimeout(self.remaining_request_ms()?)
        }
        .map_err(|_| origin_error())?;
        unsafe {
            self.settings
                .SetTransactionTimeout(self.remaining_request_ms()?)
        }
        .map_err(|_| origin_error())?;
        self.check()?;
        let result = work().map_err(|_| origin_error())?;
        self.check()?;
        Ok(result)
    }
}
impl SelectionReader for NativeSelectionReader<'_> {
    type Node = IUIAutomationElement;
    fn validate_source(&mut self) -> Result<(), PlatformError> {
        self.check()?;
        validate_source_window(self.source)?;
        self.check()
    }
    fn root(&mut self) -> Result<Self::Node, PlatformError> {
        let root = self.rpc(|| unsafe { self.ui.ElementFromHandle(self.hwnd) })?;
        if self.rpc(|| unsafe { root.CurrentProcessId() })? as u32 != self.source.source_pid {
            return Err(origin_error());
        }
        Ok(root)
    }
    fn focused(&mut self) -> Result<Self::Node, PlatformError> {
        let focused = self.rpc(|| unsafe { self.ui.GetFocusedElement() })?;
        if self.rpc(|| unsafe { focused.CurrentProcessId() })? as u32 != self.source.source_pid {
            return Err(origin_error());
        }
        Ok(focused)
    }
    fn same(&mut self, left: &Self::Node, right: &Self::Node) -> Result<bool, PlatformError> {
        self.rpc(|| unsafe { self.ui.CompareElements(left, right) })
            .map(|same| same.as_bool())
    }
    fn password(&mut self, node: &Self::Node) -> Result<bool, PlatformError> {
        self.rpc(|| unsafe { node.CurrentIsPassword() })
            .map(|password| password.as_bool())
    }
    fn parent(&mut self, node: &Self::Node) -> Result<Self::Node, PlatformError> {
        self.rpc(|| unsafe { self.walker.GetParentElement(node) })
    }
    fn selected_text(&mut self, node: &Self::Node) -> Result<String, PlatformError> {
        let pattern: IUIAutomationTextPattern =
            self.rpc(|| unsafe { node.GetCurrentPatternAs(UIA_TextPatternId) })?;
        let selected = self.rpc(|| unsafe { pattern.GetSelection() })?;
        if self.rpc(|| unsafe { selected.Length() })? != 1 {
            return Err(error("Select one continuous passage, or paste the text."));
        }
        let range = self.rpc(|| unsafe { selected.GetElement(0) })?;
        self.rpc(|| unsafe { range.GetText(12_001) })
            .map(|text| text.to_string())
    }
}

#[cfg(test)]
#[path = "windows_selection_origin_tests.rs"]
mod selection_origin_tests;

#[cfg(test)]
#[path = "windows_selection_origin_native_tests.rs"]
mod selection_origin_native_tests;

pub(super) fn validate_source_window(source: &CapturedSelection) -> Result<(), PlatformError> {
    let hwnd = HWND(
        source
            .source_window
            .ok_or_else(|| error("Invoke Pickle from the source app before capturing context."))?
            as *mut _,
    );
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 || pid != source.source_pid || pid == unsafe { GetCurrentProcessId() } {
        return Err(error(
            "The source application changed. Invoke Pickle again.",
        ));
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|_| error("The source application cannot be read."))?;
    let mut path = vec![0u16; 32768];
    let mut size = path.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.map_err(|_| error("The source application cannot be identified."))?;
    let executable = String::from_utf16(&path[..size as usize])
        .map_err(|_| error("The source application has an invalid identity."))?;
    if !source
        .app_id
        .eq_ignore_ascii_case(&format!("windows:{executable}"))
    {
        return Err(error("The source identity changed. Invoke Pickle again."));
    }
    Ok(())
}

pub fn credential_get(account: &str, _interactive: bool) -> Result<Option<String>, PlatformError> {
    let target = wide(&format!("Pickle.Rune:{account}"))?;
    let mut raw = std::ptr::null_mut();
    unsafe {
        if let Err(err) = CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut raw) {
            if err.code() == windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0) {
                return Ok(None);
            }
            return Err(error(
                "Windows Credential Manager could not read this access key.",
            ));
        }
        if raw.is_null() {
            return Err(error("Credential Manager returned no credential."));
        }
        let credential = &*raw;
        let value = if credential.CredentialBlobSize == 0 {
            Ok(String::new())
        } else if credential.CredentialBlobSize > 20_000 || credential.CredentialBlob.is_null() {
            Err(error("Stored credential is invalid."))
        } else {
            String::from_utf8(
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec(),
            )
            .map_err(|_| error("Stored credential has an invalid encoding."))
        };
        CredFree(raw.cast());
        value.map(Some)
    }
}
pub fn credential_set(account: &str, token: &str) -> Result<(), PlatformError> {
    if token.is_empty() {
        return credential_remove(account);
    }
    if token.len() > 5120 {
        return Err(error(
            "The access key exceeds the Windows credential size limit.",
        ));
    }
    let mut target = wide(&format!("Pickle.Rune:{account}"))?;
    let mut username = wide("Pickle")?;
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: token.len() as u32,
        CredentialBlob: token.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(username.as_mut_ptr()),
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0) }
        .map_err(|_| error("Windows Credential Manager could not save this access key."))
}
pub fn credential_remove(account: &str) -> Result<(), PlatformError> {
    let target = wide(&format!("Pickle.Rune:{account}"))?;
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(err) if err.code() == windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
        Err(_) => Err(error(
            "Windows Credential Manager could not remove this access key.",
        )),
    }
}
