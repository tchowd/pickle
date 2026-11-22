//! Windows capture runs on an MTA worker so an unresponsive UI Automation provider
//! cannot permanently block Rune's UI loop. No clipboard synthesis or elevated access.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
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
fn wide(text: &str) -> Result<Vec<u16>, PlatformError> {
    if text.contains('\0') {
        return Err(error(
            "A native setting contains an invalid null character.",
        ));
    }
    Ok(text.encode_utf16().chain(Some(0)).collect())
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
            let result = unsafe { capture_on_worker(HWND(hwnd as *mut _), pid, &excluded) };
            let _ = sender.send(result);
        });
    if worker.is_err() {
        CAPTURE_ACTIVE.store(false, Ordering::Release);
        return Err(error(
            "Selection capture is temporarily unavailable. Paste the passage or retry.",
        ));
    }
    receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| error("The source did not answer in time. Try again or paste the passage."))?
}
unsafe fn capture_on_worker(
    hwnd: HWND,
    pid: u32,
    exclusions: &[String],
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
        if let Ok(settings) = automation.cast::<IUIAutomation2>() {
            let _ = settings.SetConnectionTimeout(500);
            let _ = settings.SetTransactionTimeout(500);
        }
        let focused = automation.GetFocusedElement().map_err(|_| {
            error("The source does not expose a focused selection. Paste the passage.")
        })?;
        if focused.CurrentProcessId().unwrap_or(0) as u32 != pid {
            return Err(error(
                "Focus changed during capture. Select the passage again.",
            ));
        }
        let walker = automation
            .RawViewWalker()
            .map_err(|_| error("Could not inspect source protection."))?;
        let mut ancestor = focused.clone();
        for _ in 0..12 {
            if ancestor
                .CurrentIsPassword()
                .map_err(|_| error("Could not determine whether this content is protected."))?
                .as_bool()
            {
                return Err(error(
                    "Pickle does not capture password or protected fields.",
                ));
            }
            match walker.GetParentElement(&ancestor) {
                Ok(parent) => ancestor = parent,
                Err(_) => break,
            }
        }
        let pattern:IUIAutomationTextPattern=focused.GetCurrentPatternAs(UIA_TextPatternId).map_err(|_|error("This app does not expose selected text. Use the browser extension or paste the passage."))?;
        let selected = pattern
            .GetSelection()
            .map_err(|_| error("The selection could not be read. Paste the passage."))?;
        let count = selected
            .Length()
            .map_err(|_| error("The selected text is unavailable."))?;
        if count != 1 {
            return Err(error("Select one continuous passage, or paste the text."));
        }
        let range = selected
            .GetElement(0)
            .map_err(|_| error("The selection could not be read."))?;
        let text = range
            .GetText(12_001)
            .map_err(|_| error("The selected text could not be read."))?
            .to_string();
        if GetForegroundWindow() != hwnd || focused.CurrentProcessId().unwrap_or(0) as u32 != pid {
            return Err(error(
                "The source changed during capture. Select the passage again.",
            ));
        }
        let app_name = std::path::Path::new(&executable)
            .file_stem()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Application".into());
        validate_selection(
            CapturedSelection {
                text,
                app_name,
                app_id,
                bounds: None,
                source_url: None,
                source_pid: pid,
                source_window: Some(hwnd.0 as u64),
            },
            exclusions,
        )
    }
}

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
