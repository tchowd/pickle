//! Explicit installed-package probe. Only a same-executable owned child window
//! is captured; the normal source-process guard and WGC/OCR pipeline stay intact.
use super::{CapturedSelection, PlatformError, windows_child_job};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, HWND,
            INVALID_HANDLE_VALUE, LPARAM, LRESULT, RECT, WPARAM,
        },
        Graphics::Gdi::*,
        Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType},
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, PWSTR, w},
};

const WIDTH: u32 = 768;
const HEIGHT: u32 = 180;
const READY_TEST: &str = "windows-owned-window-capture-fixture-ready";
const CLOSED_TEST: &str = "windows-owned-window-capture-fixture-closed";
const FAILURE_TEST: &str = "windows-owned-window-capture-fixture-failure";
const CLASS: PCWSTR = w!("PickleOwnedWindowCaptureProbe");
fn error(category: &'static str) -> PlatformError {
    PlatformError(category.into())
}
const DIAGNOSTIC_LINE_LIMIT: usize = 4096;
const DIAGNOSTIC_TOTAL_LIMIT: usize = 14 * 1024;
struct BoundedSink<W> {
    output: W,
    remaining: usize,
    failed: bool,
}
impl<W: Write> BoundedSink<W> {
    fn write(&mut self, value: &serde_json::Value) {
        let Ok(mut bytes) = serde_json::to_vec(value) else {
            self.failed = true;
            return;
        };
        bytes.push(b'\n');
        if bytes.len() > DIAGNOSTIC_LINE_LIMIT || bytes.len() > self.remaining {
            self.failed = true;
            return;
        }
        self.remaining -= bytes.len();
        if self
            .output
            .write_all(&bytes)
            .and_then(|_| self.output.flush())
            .is_err()
        {
            self.failed = true;
        }
    }
}
struct StableDiagnostic {
    sink: BoundedSink<File>,
    original_stderr: *mut std::ffi::c_void,
}
thread_local! {
    static DIAGNOSTIC: RefCell<Option<StableDiagnostic>> = const { RefCell::new(None) };
}
struct DiagnosticSession;
fn valid_stderr_handle(handle: *mut std::ffi::c_void) -> bool {
    !handle.is_null() && handle != INVALID_HANDLE_VALUE.0
}
impl DiagnosticSession {
    fn start() -> Result<Self, PlatformError> {
        let original = std::io::stderr().as_raw_handle();
        // INVALID_HANDLE_VALUE is also a process pseudo-handle; passing it to
        // DuplicateHandle would clone the wrong object instead of a pipe.
        if !valid_stderr_handle(original) {
            let _ = writeln!(
                std::io::stderr().lock(),
                "{}",
                serde_json::json!({
                "schema":1,"test":FAILURE_TEST,"stage":"diagnostic-source-handle","valid":false})
            );
            return Err(error("report"));
        }
        let process = unsafe { GetCurrentProcess() };
        let mut duplicate = HANDLE::default();
        let cloned = unsafe {
            DuplicateHandle(
                process,
                HANDLE(original),
                process,
                &mut duplicate,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if let Err(failure) = cloned {
            // No owned channel exists yet. Fixed scalar initialization failure
            // only; never create a file or change the process's standard table.
            let _ = writeln!(
                std::io::stderr().lock(),
                "{}",
                serde_json::json!({
                "schema":1,"test":FAILURE_TEST,"stage":"diagnostic-duplicate",
                "hresult":failure.code().0})
            );
            return Err(error("report"));
        }
        if unsafe { GetFileType(duplicate) } != FILE_TYPE_PIPE {
            // DuplicateHandle also accepts non-file objects. Validate the
            // redirected diagnostic pipe before giving the handle to File.
            let _ = unsafe { CloseHandle(duplicate) };
            let _ = writeln!(
                std::io::stderr().lock(),
                "{}",
                serde_json::json!({
                "schema":1,"test":FAILURE_TEST,"stage":"diagnostic-pipe","valid":false})
            );
            return Err(error("report"));
        }
        let file = unsafe { File::from_raw_handle(duplicate.0) };
        DIAGNOSTIC.with(|state| {
            *state.borrow_mut() = Some(StableDiagnostic {
                sink: BoundedSink {
                    output: file,
                    remaining: DIAGNOSTIC_TOTAL_LIMIT,
                    failed: false,
                },
                original_stderr: original,
            })
        });
        Ok(Self)
    }
    fn failed(&self) -> bool {
        DIAGNOSTIC.with(|state| {
            state
                .borrow()
                .as_ref()
                .is_none_or(|state| state.sink.failed)
        })
    }
    fn stderr_changed(&self) -> Option<bool> {
        DIAGNOSTIC.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|state| state.original_stderr != std::io::stderr().as_raw_handle())
        })
    }
}
impl Drop for DiagnosticSession {
    fn drop(&mut self) {
        DIAGNOSTIC.with(|state| {
            state.borrow_mut().take();
        });
    }
}
fn diagnostic(value: serde_json::Value) {
    DIAGNOSTIC.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.sink.write(&value);
        }
    });
}
fn breadcrumb(stage: &'static str) {
    diagnostic(
        serde_json::json!({"schema":1,"test":"windows-owned-window-capture-breadcrumb","stage":stage}),
    );
}
fn spawn_observation(role: &'static str, observation: windows_child_job::SpawnObservation) {
    diagnostic(
        serde_json::json!({"schema":1,"test":"windows-owned-window-capture-spawn",
        "role":role,"native":observation}),
    );
}

fn start_io_failure(stage: &'static str, failure: &std::io::Error) -> PlatformError {
    let native = windows_child_job::native_failure(failure);
    diagnostic(
        serde_json::json!({"schema":1,"test":FAILURE_TEST,"passed":false,
            "stage":stage,"io_kind":format!("{:?}",failure.kind()),
            "raw_os_error":failure.raw_os_error(),
            "native_operation":native.map(|value|value.0),
            "hresult":native.map(|value|value.1)}),
    );
    error("fixture-start")
}
fn spawn_fixture() -> Result<windows_child_job::Child, PlatformError> {
    breadcrumb("fixture-spawn-before");
    match windows_child_job::spawn_self_capture_fixture(Some(&|value| {
        spawn_observation("initial", value)
    })) {
        Ok(child) => {
            diagnostic(serde_json::json!({"schema":1,
                "test":"windows-owned-window-capture-fixture-launch",
                "breakaway_retry":false,"creation_flags":windows_child_job::fixture_creation_flags(false),
                "atomic_job_membership_verified":true}));
            Ok(child)
        }
        Err(original) => {
            diagnostic(
                serde_json::json!({"schema":1,"test":FAILURE_TEST,"stage":"initial-spawn-failed",
                "failure":io_evidence(&original)}),
            );
            let parent = windows_child_job::parent_spawn_snapshot();
            diagnostic(
                serde_json::json!({"schema":1,"test":FAILURE_TEST,"stage":"parent-observed","parent":parent}),
            );
            breadcrumb("control-before");
            let control = version_control_evidence();
            diagnostic(
                serde_json::json!({"schema":1,"test":FAILURE_TEST,"stage":"control-returned","control":control}),
            );
            breadcrumb("retry-before");
            let retry = windows_child_job::retry_capture_fixture_breakaway(
                &original,
                Some(&|value| spawn_observation("retry", value)),
            );
            breadcrumb("retry-returned");
            let retry_evidence = match &retry {
                None => serde_json::json!({"attempted":false}),
                Some(Ok(_)) => serde_json::json!({"attempted":true,"spawned":true,
                    "creation_flags":windows_child_job::fixture_creation_flags(true),
                    "atomic_job_membership_verified":true}),
                Some(Err(failure)) => serde_json::json!({"attempted":true,"spawned":false,
                    "creation_flags":windows_child_job::fixture_creation_flags(true),
                    "failure":io_evidence(failure)}),
            };
            let native = windows_child_job::native_failure(&original);
            diagnostic(
                serde_json::json!({"schema":1,"test":FAILURE_TEST,"passed":false,
                "stage":"spawn","io_kind":format!("{:?}",original.kind()),
                "raw_os_error":original.raw_os_error(),"native_operation":native.map(|value|value.0),
                "hresult":native.map(|value|value.1),"parent":parent,
                "self_version_control":control,"breakaway_retry":retry_evidence}),
            );
            match retry {
                Some(Ok(child)) => Ok(child),
                _ => Err(error("fixture-start")),
            }
        }
    }
}
fn io_evidence(failure: &std::io::Error) -> serde_json::Value {
    let native = windows_child_job::native_failure(failure);
    serde_json::json!({"io_kind":format!("{:?}",failure.kind()),
        "raw_os_error":failure.raw_os_error(),
        "native_operation":native.map(|value|value.0),"hresult":native.map(|value|value.1)})
}
fn version_control_evidence() -> serde_json::Value {
    match windows_child_job::spawn_self_version_control(Some(&|value| {
        spawn_observation("control", value)
    })) {
        Err(failure) => serde_json::json!({"diagnostic_only":true,"spawned":false,
            "inherits_stdio":false,"stage":"spawn","failure":io_evidence(&failure)}),
        Ok(mut child) => {
            let waited = child.wait_for_exit(Duration::from_secs(2));
            let (exit_code, job_drained, forced_stop, wait_failure, stop_failure) = match waited {
                Ok(status) => (status.code(), true, false, None, None),
                Err(failure) => {
                    let stopped = child.stop_and_wait(Duration::from_secs(1));
                    (
                        None,
                        stopped.is_ok(),
                        true,
                        Some(io_evidence(&failure)),
                        stopped.as_ref().err().map(io_evidence),
                    )
                }
            };
            // On unsuccessful drain, closing this exact non-inherited job still
            // terminates its own tree. Do not claim acknowledged cleanup then.
            drop(child);
            serde_json::json!({"diagnostic_only":true,"spawned":true,
                "inherits_stdio":false,"atomic_job_membership_verified":true,
                "stage":"wait","exit_code":exit_code,"job_drained":job_drained,
                "forced_stop":forced_stop,"wait_failure":wait_failure,"stop_failure":stop_failure})
        }
    }
}
fn helper_category(category: &str) -> bool {
    matches!(
        category,
        "identity" | "fixture-start" | "paint" | "cleanup" | "report"
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    schema: u32,
    test: String,
    pid: u32,
    hwnd: u64,
    width: u32,
    height: u32,
    painted: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Closed {
    schema: u32,
    test: String,
    window_closed: bool,
    thread_joined: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Failed {
    schema: u32,
    test: String,
    category: String,
}
enum Output {
    Ready(Ready),
    Closed(Closed),
    End,
    Invalid,
    Failed(Failed),
}

fn read_frame(reader: &mut impl BufRead) -> Result<Vec<u8>, ()> {
    let mut bytes = Vec::new();
    reader
        .take(1025)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 1024 || bytes.last() != Some(&b'\n') {
        return Err(());
    }
    Ok(bytes)
}
fn drain_output(output: impl Read + Send + 'static, sender: SyncSender<Output>) {
    let mut reader = BufReader::new(output);
    let result = (|| {
        let first = read_frame(&mut reader)?;
        if let Ok(failed) = serde_json::from_slice::<Failed>(&first) {
            return sender.send(Output::Failed(failed)).map_err(|_| ());
        }
        let ready: Ready = serde_json::from_slice(&first).map_err(|_| ())?;
        sender.send(Output::Ready(ready)).map_err(|_| ())?;
        let second = read_frame(&mut reader)?;
        if let Ok(failed) = serde_json::from_slice::<Failed>(&second) {
            return sender.send(Output::Failed(failed)).map_err(|_| ());
        }
        let closed: Closed = serde_json::from_slice(&second).map_err(|_| ())?;
        sender.send(Output::Closed(closed)).map_err(|_| ())?;
        let mut trailing = [0u8; 1];
        if reader.read(&mut trailing).map_err(|_| ())? != 0 {
            return Err(());
        }
        sender.send(Output::End).map_err(|_| ())
    })();
    if result.is_err() {
        let _ = sender.send(Output::Invalid);
    }
}
struct FixtureChild {
    child: Option<windows_child_job::Child>,
    output: Receiver<Output>,
    drain: Option<JoinHandle<()>>,
}
impl FixtureChild {
    fn start() -> Result<Self, PlatformError> {
        let mut child = spawn_fixture()?;
        drop(child.stdin.take());
        let output = child.stdout.take().ok_or_else(|| {
            start_io_failure("stdout-missing", &std::io::Error::other("missing pipe"))
        })?;
        let (sender, receiver) = mpsc::sync_channel(3);
        let drain = std::thread::Builder::new()
            .name("pickle-capture-fixture-output".into())
            .spawn(move || drain_output(output, sender))
            .map_err(|failure| start_io_failure("stdout-worker", &failure))?;
        Ok(Self {
            child: Some(child),
            output: receiver,
            drain: Some(drain),
        })
    }
    fn ready(&self) -> Result<Ready, PlatformError> {
        let value = match self.output.recv_timeout(Duration::from_secs(4)) {
            Ok(Output::Ready(value)) => value,
            other => {
                let (stage, category) = match other {
                    Ok(Output::Failed(failed))
                        if failed.schema == 1
                            && failed.test == FAILURE_TEST
                            && helper_category(&failed.category) =>
                    {
                        ("helper-failed", Some(failed.category))
                    }
                    Ok(_) => ("ready-protocol", None),
                    Err(mpsc::RecvTimeoutError::Timeout) => ("ready-timeout", None),
                    Err(mpsc::RecvTimeoutError::Disconnected) => ("ready-disconnected", None),
                };
                diagnostic(serde_json::json!({"schema":1,"test":FAILURE_TEST,
                    "passed":false,"stage":stage,"helper_category":category,
                    "child_running":self.child.as_ref().and_then(|child|child.is_running().ok())}));
                return Err(error("fixture-start"));
            }
        };
        if !self
            .child
            .as_ref()
            .ok_or_else(|| error("target"))?
            .is_running()
            .map_err(|_| error("target"))?
        {
            return Err(error("target"));
        }
        if value.schema != 1
            || value.test != READY_TEST
            || value.pid
                != self
                    .child
                    .as_ref()
                    .ok_or_else(|| error("target"))?
                    .id()
                    .map_err(|_| error("target"))?
            || value.hwnd == 0
            || value.width != WIDTH
            || value.height != HEIGHT
            || !value.painted
        {
            return Err(error("target"));
        }
        Ok(value)
    }
    fn finish(&mut self, ready: &Ready) -> Result<(), PlatformError> {
        let window = HWND(ready.hwnd as *mut _);
        if !self
            .child
            .as_ref()
            .ok_or_else(|| error("cleanup"))?
            .is_running()
            .map_err(|_| error("cleanup"))?
        {
            return Err(error("cleanup"));
        }
        verify_owner(window, ready.pid)?;
        unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            .map_err(|_| error("cleanup"))?;
        let closed = match self.output.recv_timeout(Duration::from_secs(2)) {
            Ok(Output::Closed(value)) => value,
            _ => return Err(error("cleanup")),
        };
        if closed.schema != 1
            || closed.test != CLOSED_TEST
            || !closed.window_closed
            || !closed.thread_joined
        {
            return Err(error("cleanup"));
        }
        let status = self
            .child
            .as_mut()
            .ok_or_else(|| error("cleanup"))?
            .wait_for_exit(Duration::from_secs(2))
            .map_err(|_| error("cleanup"))?;
        if !status.success() {
            return Err(error("cleanup"));
        }
        self.child.take();
        if !matches!(
            self.output.recv_timeout(Duration::from_secs(1)),
            Ok(Output::End)
        ) {
            return Err(error("cleanup"));
        }
        if let Some(drain) = self.drain.take() {
            let until = Instant::now() + Duration::from_millis(500);
            while !drain.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            if !drain.is_finished() {
                return Err(error("cleanup"));
            }
            drain.join().map_err(|_| error("cleanup"))?;
        }
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut owner)) };
        if unsafe { IsWindow(Some(window)) }.as_bool() && owner == ready.pid {
            return Err(error("cleanup"));
        }
        Ok(())
    }
    fn stop(&mut self) -> Result<(), PlatformError> {
        if let Some(mut child) = self.child.take() {
            let result = child.stop_and_wait(Duration::from_secs(2));
            drop(child);
            result.map_err(|_| error("cleanup"))?;
        }
        Ok(())
    }
}
impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.stop();
        if let Some(drain) = self.drain.take() {
            let until = Instant::now() + Duration::from_millis(500);
            while !drain.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            if drain.is_finished() {
                let _ = drain.join();
            }
        }
    }
}
fn verify_owner(window: HWND, pid: u32) -> Result<(), PlatformError> {
    let mut actual = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut actual)) };
    if pid == 0 || actual != pid || !unsafe { IsWindow(Some(window)) }.as_bool() {
        return Err(error("target"));
    }
    Ok(())
}
fn source(ready: &Ready) -> Result<CapturedSelection, PlatformError> {
    let window = HWND(ready.hwnd as *mut _);
    verify_owner(window, ready.pid)?;
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, ready.pid) }
        .map_err(|_| error("target"))?;
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    let read = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    read.map_err(|_| error("target"))?;
    let path = String::from_utf16(&path[..length as usize]).map_err(|_| error("target"))?;
    let expected = std::fs::canonicalize(std::env::current_exe().map_err(|_| error("target"))?)
        .map_err(|_| error("target"))?;
    let observed = std::fs::canonicalize(&path).map_err(|_| error("target"))?;
    if !expected
        .as_os_str()
        .eq_ignore_ascii_case(observed.as_os_str())
    {
        return Err(error("target"));
    }
    Ok(CapturedSelection {
        text: String::new(),
        app_name: "Owned window capture fixture".into(),
        app_id: format!("windows:{path}"),
        bounds: None,
        source_url: None,
        source_pid: ready.pid,
        source_window: Some(ready.hwnd),
    })
}

pub(super) fn verify_window_capture_runtime() -> Result<serde_json::Value, PlatformError> {
    let session = DiagnosticSession::start()?;
    let result = verify_window_capture_inner();
    let category = result
        .as_ref()
        .err()
        .map(|failure| match failure.0.as_str() {
            "identity" | "fixture-start" | "paint" | "target" | "capture" | "image" | "word"
            | "cleanup" | "report" => failure.0.as_str(),
            _ => "unavailable",
        });
    diagnostic(
        serde_json::json!({"schema":1,"test":"windows-owned-window-capture-breadcrumb",
        "stage":"verify-returned","succeeded":result.is_ok(),
        "failure_category":category,
        "current_standard_stderr_handle_changed":session.stderr_changed(),
        "diagnostic_write_failed":session.failed()}),
    );
    if result.is_ok() && session.failed() {
        return Err(error("report"));
    }
    result
}
fn verify_window_capture_inner() -> Result<serde_json::Value, PlatformError> {
    breadcrumb("verify-enter");
    if !super::windows_context::has_package_identity() {
        return Err(error("identity"));
    }
    let mut fixture = FixtureChild::start()?;
    let ready = fixture.ready()?;
    let source = source(&ready)?;
    let capture_diagnostic = std::sync::Arc::new(super::windows_context::CaptureDiagnostics::new());
    breadcrumb("capture-before");
    let captured = super::windows_context::capture_page_observed(
        &source,
        CancellationToken::new(),
        Some(capture_diagnostic.clone()),
    )
    .map_err(|_| error("capture"));
    let result = captured.and_then(|page| {
        let pixels = image::load_from_memory_with_format(&page.jpeg, image::ImageFormat::Jpeg)
            .map_err(|_| error("image"))?
            .to_rgb8();
        let dark = pixels
            .pixels()
            .filter(|pixel| pixel[0] < 64 && pixel[1] < 64 && pixel[2] < 64)
            .count();
        if !valid_capture_geometry(pixels.width(), pixels.height(), dark, page.jpeg.len()) {
            return Err(error("image"));
        }
        if !super::windows_capabilities::ocr_fixture_matches(&page.text) {
            return Err(error("word"));
        }
        Ok((pixels.width(), pixels.height(), dark, page.jpeg.len()))
    });
    let cleanup = fixture.finish(&ready);
    if result.is_err() || cleanup.is_err() {
        diagnostic(
            serde_json::json!({"schema":1,"test":"windows-owned-window-capture-native","passed":false,"native_capture":capture_diagnostic.snapshot()}),
        );
    }
    cleanup?;
    let (width, height, dark, jpeg_bytes) = result?;
    Ok(
        serde_json::json!({"schema":1,"test":"windows-owned-window-capture","passed":true,"package_identity":true,"production_window_capture":true,"expected_word_matched":true,"jpeg_bytes":jpeg_bytes,"width":width,"height":height,"dark_pixels":dark,"painted_before_capture":true,"owned_window_closed":true,"fixture_thread_joined":true,"fixture_process_stopped":true,"native_capture":capture_diagnostic.snapshot()}),
    )
}
fn valid_capture_geometry(width: u32, height: u32, dark: usize, jpeg_bytes: usize) -> bool {
    (WIDTH / 2..=1440).contains(&width)
        && (HEIGHT / 2..=1440).contains(&height)
        && (u64::from(width) * u64::from(HEIGHT)).abs_diff(u64::from(height) * u64::from(WIDTH))
            <= u64::from(WIDTH) * 2
        && dark > 100
        && dark < (width as usize * height as usize) / 4
        && (1..=750_000).contains(&jpeg_bytes)
}

pub(super) fn run_window_capture_fixture() -> Result<(), PlatformError> {
    let result = run_fixture();
    if let Err(failure) = &result {
        // stderr is intentionally NUL in the contained child. Report only a
        // fixed allowlisted category on its existing bounded stdout channel.
        let category = if helper_category(&failure.0) {
            &failure.0
        } else {
            "fixture-start"
        };
        let _ = emit(&Failed {
            schema: 1,
            test: FAILURE_TEST.into(),
            category: category.into(),
        });
    }
    result
}

struct PaintState {
    pixels: Vec<u8>,
    ready: Option<SyncSender<Result<Ready, PlatformError>>>,
}
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    let pointer = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *mut PaintState;
    if message == WM_PAINT && !pointer.is_null() {
        let state = unsafe { &mut *pointer };
        let mut paint = PAINTSTRUCT::default();
        let dc = unsafe { BeginPaint(window, &mut paint) };
        let mut rectangle = RECT::default();
        let client = unsafe { GetClientRect(window, &mut rectangle) }.is_ok();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: WIDTH as i32,
                biHeight: -(HEIGHT as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let painted = !dc.0.is_null()
            && client
            && rectangle.right > 0
            && rectangle.bottom > 0
            && unsafe {
                StretchDIBits(
                    dc,
                    0,
                    0,
                    rectangle.right,
                    rectangle.bottom,
                    0,
                    0,
                    WIDTH as i32,
                    HEIGHT as i32,
                    Some(state.pixels.as_ptr().cast()),
                    &info,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                )
            } > 0
            && unsafe { GdiFlush() }.as_bool();
        let ended = unsafe { EndPaint(window, &paint) }.as_bool();
        if let Some(sender) = state.ready.take() {
            let value = if painted && ended {
                Ok(Ready {
                    schema: 1,
                    test: READY_TEST.into(),
                    pid: unsafe { GetCurrentProcessId() },
                    hwnd: window.0 as u64,
                    width: WIDTH,
                    height: HEIGHT,
                    painted: true,
                })
            } else {
                Err(error("paint"))
            };
            let _ = sender.send(value);
            if !painted || !ended {
                let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
        }
        return LRESULT(0);
    }
    if message == WM_TIMER && wparam.0 == 1 || message == WM_CLOSE {
        let _ = unsafe { DestroyWindow(window) };
        return LRESULT(0);
    }
    if message == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, 0) };
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
fn owned_window(
    pixels: Vec<u8>,
    ready: SyncSender<Result<Ready, PlatformError>>,
) -> Result<(), PlatformError> {
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|_| error("fixture-start"))?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(error("fixture-start"));
    }
    struct Registration(windows::Win32::Foundation::HINSTANCE);
    impl Drop for Registration {
        fn drop(&mut self) {
            let _ = unsafe { UnregisterClassW(CLASS, Some(self.0)) };
        }
    }
    let _registration = Registration(instance.into());
    let mut state = Box::new(PaintState {
        pixels,
        ready: Some(ready),
    });
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS,
            w!("Owned capture fixture"),
            WS_POPUP | WS_VISIBLE,
            100,
            100,
            WIDTH as i32,
            HEIGHT as i32,
            None,
            None,
            Some(instance.into()),
            Some((&mut *state as *mut PaintState).cast()),
        )
    }
    .map_err(|_| error("fixture-start"))?;
    if unsafe { SetTimer(Some(window), 1, 20000, None) } == 0 {
        let _ = unsafe { DestroyWindow(window) };
        return Err(error("fixture-start"));
    }
    let _ = unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
    if !unsafe { UpdateWindow(window) }.as_bool() {
        let _ = unsafe { DestroyWindow(window) };
        return Err(error("paint"));
    }
    let mut message = MSG::default();
    loop {
        let got = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if got <= 0 {
            if unsafe { IsWindow(Some(window)) }.as_bool() {
                let _ = unsafe { DestroyWindow(window) };
            }
            if got < 0 {
                return Err(error("cleanup"));
            }
            break;
        }
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    if unsafe { IsWindow(Some(window)) }.as_bool() {
        return Err(error("cleanup"));
    }
    Ok(())
}
fn emit(value: &impl Serialize) -> Result<(), PlatformError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| error("report"))?;
    if bytes.len() > 1023 {
        return Err(error("report"));
    }
    bytes.push(b'\n');
    let mut output = std::io::stdout().lock();
    output
        .write_all(&bytes)
        .and_then(|_| output.flush())
        .map_err(|_| error("report"))
}
fn run_fixture() -> Result<(), PlatformError> {
    if !super::windows_context::has_package_identity() {
        return Err(error("identity"));
    }
    let fixture = super::windows_ocr::fixture_image().map_err(|_| error("paint"))?;
    let mut pixels = fixture.into_raw();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let (ready_sender, ready) = mpsc::sync_channel(1);
    let (closed_sender, closed) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("pickle-owned-capture-window".into())
        .spawn(move || {
            let result = owned_window(pixels, ready_sender);
            let _ = closed_sender.send(result);
        })
        .map_err(|_| error("fixture-start"))?;
    let ready = ready
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| error("fixture-start"))??;
    emit(&ready)?;
    closed
        .recv_timeout(Duration::from_secs(22))
        .map_err(|_| error("cleanup"))??;
    let until = Instant::now() + Duration::from_secs(1);
    while !thread.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    if !thread.is_finished() {
        return Err(error("cleanup"));
    }
    thread.join().map_err(|_| error("cleanup"))?;
    emit(&Closed {
        schema: 1,
        test: CLOSED_TEST.into(),
        window_closed: true,
        thread_joined: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_channel_refuses_missing_and_pseudo_standard_handles() {
        assert!(!valid_stderr_handle(std::ptr::null_mut()));
        assert!(!valid_stderr_handle(INVALID_HANDLE_VALUE.0));
        // No dereference/query: real validity is still checked by DuplicateHandle.
        assert!(valid_stderr_handle(42_usize as *mut _));
    }
    #[test]
    fn stable_sink_caps_lines_and_total_bytes_and_latches_write_failure() {
        let value = serde_json::json!({"stage":"owned"});
        let line = serde_json::to_vec(&value).unwrap().len() + 1;
        let mut sink = BoundedSink {
            output: Vec::<u8>::new(),
            remaining: line,
            failed: false,
        };
        sink.write(&value);
        assert_eq!(sink.output.len(), line);
        assert!(!sink.failed);
        sink.write(&value);
        assert!(sink.failed);
        assert_eq!(sink.output.len(), line);
        let mut sink = BoundedSink {
            output: Vec::<u8>::new(),
            remaining: DIAGNOSTIC_TOTAL_LIMIT,
            failed: false,
        };
        sink.write(&serde_json::json!({"stage":"x".repeat(DIAGNOSTIC_LINE_LIMIT)}));
        assert!(sink.failed);
        assert!(sink.output.is_empty());
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from_raw_os_error(6))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = BoundedSink {
            output: Broken,
            remaining: DIAGNOSTIC_TOTAL_LIMIT,
            failed: false,
        };
        sink.write(&value);
        assert!(sink.failed);
    }
    #[test]
    fn retained_image_gate_refuses_blank_wrong_shape_oversized_and_empty_captures() {
        assert!(valid_capture_geometry(768, 180, 1000, 5000));
        assert!(valid_capture_geometry(960, 225, 1500, 7000));
        for (w, h, ink, bytes) in [
            (768, 180, 0, 5000),
            (768, 180, 1000, 0),
            (768, 180, 1000, 750001),
            (180, 768, 1000, 5000),
            (768, 180, 768 * 180, 5000),
            (4000, 938, 1000, 5000),
        ] {
            assert!(!valid_capture_geometry(w, h, ink, bytes));
        }
    }
    #[test]
    fn helper_protocol_is_bounded_and_refuses_missing_newline_and_extra_fields() {
        let data = b"{\"schema\":1}\n";
        assert_eq!(read_frame(&mut &data[..]).unwrap(), data);
        assert!(read_frame(&mut &b"unterminated"[..]).is_err());
        assert!(read_frame(&mut std::io::Cursor::new(vec![b'x'; 1025])).is_err());
        let value = serde_json::json!({"schema":1,"test":READY_TEST,"pid":1,"hwnd":2,"width":WIDTH,"height":HEIGHT,"painted":true,"arbitrary":"rejected"});
        assert!(serde_json::from_value::<Ready>(value).is_err());
    }
    #[test]
    fn helper_failure_frame_retains_only_exact_schema_and_allowlisted_categories() {
        let mut bytes = serde_json::to_vec(&Failed {
            schema: 1,
            test: FAILURE_TEST.into(),
            category: "identity".into(),
        })
        .unwrap();
        bytes.push(b'\n');
        let (sender, receiver) = mpsc::sync_channel(3);
        drain_output(std::io::Cursor::new(bytes), sender);
        let Output::Failed(failed) = receiver.recv().unwrap() else {
            panic!("expected fixed failure frame")
        };
        assert_eq!(failed.schema, 1);
        assert_eq!(failed.test, FAILURE_TEST);
        assert!(helper_category(&failed.category));
        assert!(!helper_category("arbitrary/private/native/message"));
        assert!(
            serde_json::from_value::<Failed>(serde_json::json!({
                "schema":1,"test":FAILURE_TEST,"category":"identity","extra":"rejected"
            }))
            .is_err()
        );
    }
    #[test]
    fn control_io_evidence_is_numeric_and_never_serializes_arbitrary_error_text() {
        let value = io_evidence(&std::io::Error::from_raw_os_error(5));
        assert_eq!(value["raw_os_error"], 5);
        assert!(value["native_operation"].is_null());
        assert!(value["hresult"].is_null());
        let arbitrary = io_evidence(&std::io::Error::other("untrusted/private/path_or_text"));
        assert_eq!(arbitrary["io_kind"], "Other");
        assert!(!arbitrary.to_string().contains("private"));
        assert!(
            arbitrary
                .as_object()
                .is_some_and(|fields| fields.len() == 4)
        );
    }
}
