//! Hosted-only native GUARD proof. B's real UIA descendant is injected as a
//! stale observation; this does NOT claim Windows GetFocusedElement returned B.
use super::*;
use ::windows::Win32::Foundation::{LPARAM, WPARAM};
use serde::Deserialize;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
};

const COMMAND: u32 = WM_APP + 0x142;
const EXPECTED: &str = "Café selected only in window A.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    pid: u32,
    window_a: u64,
    window_b: u64,
}

fn trace(path: &Path, stage: &'static str, values: serde_json::Value) {
    let bytes =
        serde_json::to_vec(&serde_json::json!({"schema":1,"stage":stage,"values":values})).unwrap();
    assert!(bytes.len() < 2048);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    assert!(file.metadata().unwrap().len() + (bytes.len() as u64) < 16384);
    file.write_all(&bytes).unwrap();
    file.write_all(b"\n").unwrap();
}
fn framework_path(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap();
    let value = canonical.to_str().unwrap();
    PathBuf::from(if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
    })
}
fn compile_fixture(directory: &Path) -> PathBuf {
    let directory = framework_path(directory);
    let source = directory.join("SelectionOriginFixture.cs");
    std::fs::write(
        &source,
        include_bytes!("../../../native/windows/SelectionOriginFixture.cs"),
    )
    .unwrap();
    let framework = framework_path(
        &PathBuf::from(std::env::var_os("WINDIR").unwrap())
            .join("Microsoft.NET/Framework64/v4.0.30319"),
    );
    let executable = directory.join("PickleSelectionOriginFixture.exe");
    let mut command = Command::new(framework.join("csc.exe"));
    command
        .args([
            "/nologo",
            "/target:winexe",
            "/platform:x64",
            "/langversion:5",
            "/warnaserror+",
        ])
        .arg(format!("/out:{}", executable.display()));
    for reference in [
        "System.dll",
        "System.Core.dll",
        "System.Drawing.dll",
        "System.Windows.Forms.dll",
    ] {
        command.arg(format!("/r:{}", framework.join(reference).display()));
    }
    for reference in [
        "WindowsBase.dll",
        "UIAutomationClient.dll",
        "UIAutomationTypes.dll",
        "UIAutomationProvider.dll",
    ] {
        command.arg(format!(
            "/r:{}",
            framework.join("WPF").join(reference).display()
        ));
    }
    let output = command.arg(source).stdin(Stdio::null()).output().unwrap();
    assert!(output.stdout.len() + output.stderr.len() < 16000);
    assert!(
        output.status.success(),
        "Owned selection-origin fixture C# typecheck failed"
    );
    executable
}
fn owner(window: HWND) -> u32 {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    pid
}
fn command(window: HWND, pid: u32, code: usize) -> usize {
    assert!(unsafe { IsWindow(Some(window)) }.as_bool());
    assert_eq!(owner(window), pid);
    let mut result = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            window,
            COMMAND,
            WPARAM(code),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            500,
            Some(&mut result),
        )
    };
    assert_ne!(sent.0, 0, "Owned fixture command did not acknowledge");
    result
}
fn counters(window: HWND, pid: u32) -> [usize; 4] {
    let value = command(window, pid, 2);
    assert!(
        value <= u32::MAX as usize,
        "Owned counter exceeded its fixed bound"
    );
    [
        value & 255,
        (value >> 8) & 255,
        (value >> 16) & 255,
        (value >> 24) & 255,
    ]
}
fn delta(after: [usize; 4], before: [usize; 4]) -> [usize; 4] {
    std::array::from_fn(|index| after[index].checked_sub(before[index]).unwrap())
}
struct Fixture {
    child: Option<super::super::windows_child_job::Child>,
    source: CapturedSelection,
    window_b: u64,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.stop_and_wait(Duration::from_secs(2));
        }
    }
}
impl Fixture {
    fn launch(executable: &Path, directory: &Path) -> Self {
        let ready_path = directory.join("ready.json");
        let mut start = Command::new(executable);
        start.arg(&ready_path);
        let mut child = super::super::windows_child_job::spawn(&start).unwrap();
        drop(child.stdin.take());
        drop(child.stdout.take());
        let mut fixture = Self {
            child: Some(child),
            source: CapturedSelection {
                text: String::new(),
                app_name: "Owned source-origin fixture".into(),
                app_id: String::new(),
                bounds: None,
                source_url: None,
                source_pid: 0,
                source_window: None,
            },
            window_b: 0,
        };
        let until = Instant::now() + Duration::from_secs(10);
        let ready: Ready = loop {
            assert!(
                Instant::now() < until,
                "Owned selection-origin fixture readiness timed out"
            );
            assert!(fixture.child.as_ref().unwrap().is_running().unwrap());
            if let Ok(bytes) = std::fs::read(&ready_path) {
                assert!(bytes.len() < 1024);
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    break value;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let pid = fixture.child.as_ref().unwrap().id().unwrap();
        assert_eq!(ready.pid, pid);
        assert_ne!(ready.window_a, ready.window_b);
        assert_ne!(ready.window_a, 0);
        assert_ne!(ready.window_b, 0);
        for handle in [ready.window_a, ready.window_b] {
            let hwnd = HWND(handle as *mut _);
            assert!(unsafe { IsWindow(Some(hwnd)) }.as_bool());
            assert_eq!(owner(hwnd), pid);
        }
        let process =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.unwrap();
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        let identified = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut size,
            )
        };
        let _ = unsafe { CloseHandle(process) };
        identified.unwrap();
        let actual = String::from_utf16(&path[..size as usize]).unwrap();
        assert_eq!(
            std::fs::canonicalize(&actual).unwrap(),
            std::fs::canonicalize(executable).unwrap()
        );
        fixture.source.source_pid = pid;
        fixture.source.source_window = Some(ready.window_a);
        fixture.source.app_id = format!("windows:{}", actual.to_lowercase());
        fixture.window_b = ready.window_b;
        loop {
            assert!(
                Instant::now() < until,
                "Owned A window did not become foreground"
            );
            command(HWND(ready.window_a as *mut _), pid, 1);
            if unsafe { GetForegroundWindow() } == HWND(ready.window_a as *mut _) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fixture
    }
    fn close(&mut self) {
        let hwnd = HWND(self.source.source_window.unwrap() as *mut _);
        assert_eq!(owner(hwnd), self.source.source_pid);
        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
        let status = self
            .child
            .as_mut()
            .unwrap()
            .wait_for_exit(Duration::from_secs(3))
            .unwrap();
        assert!(status.success());
        for handle in [self.source.source_window.unwrap(), self.window_b] {
            let window = HWND(handle as *mut _);
            assert!(
                !unsafe { IsWindow(Some(window)) }.as_bool()
                    || owner(window) != self.source.source_pid
            );
        }
        // Retain the exited process handle through both HWND-owner checks so
        // its PID cannot be recycled while proving exact-window cleanup.
        self.child.take();
    }
}

/// Inject only the stale focus observation. All origin checks, password/raw
/// navigation, text access, deadlines and publication use production methods.
struct InjectedFocus<'a> {
    native: NativeSelectionReader<'a>,
    focused: IUIAutomationElement,
    root_a: IUIAutomationElement,
    root_b: IUIAutomationElement,
    compared_b_root_to_a_unequal: bool,
    selected_calls: usize,
}
impl SelectionReader for InjectedFocus<'_> {
    type Node = IUIAutomationElement;
    fn validate_source(&mut self) -> Result<(), PlatformError> {
        self.native.validate_source()
    }
    fn root(&mut self) -> Result<Self::Node, PlatformError> {
        self.native.root()
    }
    fn focused(&mut self) -> Result<Self::Node, PlatformError> {
        if self
            .native
            .rpc(|| unsafe { self.focused.CurrentProcessId() })? as u32
            != self.native.source.source_pid
        {
            return Err(origin_error());
        }
        Ok(self.focused.clone())
    }
    fn same(&mut self, left: &Self::Node, right: &Self::Node) -> Result<bool, PlatformError> {
        let same = self.native.same(left, right)?;
        // Observe the actual production proof's operands. A generic provider
        // timeout must not count as evidence of the intended wrong-root gate.
        if !same
            && self.native.same(left, &self.root_b)?
            && self.native.same(right, &self.root_a)?
        {
            self.compared_b_root_to_a_unequal = true;
        }
        Ok(same)
    }
    fn password(&mut self, node: &Self::Node) -> Result<bool, PlatformError> {
        self.native.password(node)
    }
    fn parent(&mut self, node: &Self::Node) -> Result<Self::Node, PlatformError> {
        self.native.parent(node)
    }
    fn selected_text(&mut self, node: &Self::Node) -> Result<String, PlatformError> {
        self.selected_calls += 1;
        self.native.selected_text(node)
    }
}
fn reader<'a>(ui: &IUIAutomation, source: &'a CapturedSelection) -> NativeSelectionReader<'a> {
    NativeSelectionReader {
        ui: ui.clone(),
        settings: ui.cast().unwrap(),
        walker: unsafe { ui.RawViewWalker() }.unwrap(),
        source,
        hwnd: HWND(source.source_window.unwrap() as *mut _),
        deadline: Instant::now() + Duration::from_secs(2),
    }
}
fn leaf(
    reader: &NativeSelectionReader<'_>,
    window: HWND,
) -> (IUIAutomationElement, IUIAutomationElement) {
    let root = reader
        .rpc(|| unsafe { reader.ui.ElementFromHandle(window) })
        .unwrap();
    assert_eq!(
        reader
            .rpc(|| unsafe { root.CurrentAutomationId() })
            .unwrap(),
        "owned-origin-1"
    );
    assert_eq!(
        reader
            .rpc(|| unsafe { root.CurrentNativeWindowHandle() })
            .unwrap(),
        window
    );
    assert_eq!(
        reader.rpc(|| unsafe { root.CurrentProcessId() }).unwrap() as u32,
        reader.source.source_pid
    );
    let layout = reader
        .rpc(|| unsafe { reader.walker.GetFirstChildElement(&root) })
        .unwrap();
    assert_eq!(
        reader
            .rpc(|| unsafe { layout.CurrentAutomationId() })
            .unwrap(),
        "owned-origin-2"
    );
    assert_eq!(
        reader
            .rpc(|| unsafe { layout.CurrentNativeWindowHandle() })
            .unwrap()
            .0,
        std::ptr::null_mut()
    );
    let leaf = reader
        .rpc(|| unsafe { reader.walker.GetFirstChildElement(&layout) })
        .unwrap();
    assert_eq!(
        reader
            .rpc(|| unsafe { leaf.CurrentAutomationId() })
            .unwrap(),
        "owned-origin-3"
    );
    assert_eq!(
        reader
            .rpc(|| unsafe { leaf.CurrentNativeWindowHandle() })
            .unwrap()
            .0,
        std::ptr::null_mut()
    );
    assert_eq!(
        reader.rpc(|| unsafe { leaf.CurrentProcessId() }).unwrap() as u32,
        reader.source.source_pid
    );
    (root, leaf)
}
fn native_checks(source: &CapturedSelection, window_b: u64, log: &Path) -> serde_json::Value {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .unwrap();
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _com = Com;
    let ui: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.unwrap();
    let a = HWND(source.source_window.unwrap() as *mut _);
    let b = HWND(window_b as *mut _);
    let pid = source.source_pid;
    let preflight = reader(&ui, source);
    let (root_a, leaf_a) = leaf(&preflight, a);
    let (root_b, leaf_b) = leaf(&preflight, b);
    assert!(
        !preflight
            .rpc(|| unsafe { ui.CompareElements(&root_a, &root_b) })
            .unwrap()
            .as_bool(),
        "Actual A/B window roots must have different UIA identity"
    );
    let until = Instant::now() + Duration::from_secs(2);
    let mut focus_attempts = 0;
    loop {
        focus_attempts += 1;
        let probe = reader(&ui, source);
        if let Ok(focus) = probe.rpc(|| unsafe { ui.GetFocusedElement() })
            && probe
                .rpc(|| unsafe { ui.CompareElements(&focus, &leaf_a) })
                .unwrap()
                .as_bool()
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "Ordinary focused A descendant was not published"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(unsafe { GetForegroundWindow() }, a);
    assert_eq!(owner(a), pid);
    assert_eq!(owner(b), pid);
    let before_a = counters(a, pid);
    let before_b = counters(b, pid);
    assert_eq!(&before_a[1..], &[0, 0, 0]);
    assert_eq!(&before_b[1..], &[0, 0, 0]);
    trace(
        log,
        "negative-before",
        serde_json::json!({"foreground_a":true,"same_pid":true,"focus_b_injected":true,"native_roots_distinct":true}),
    );
    let mut negative = InjectedFocus {
        native: reader(&ui, source),
        focused: leaf_b,
        root_a,
        root_b,
        compared_b_root_to_a_unequal: false,
        selected_calls: 0,
    };
    assert!(
        read_bound_selection(&mut negative).is_err(),
        "B-origin selection must not be admitted for A"
    );
    assert_eq!(negative.selected_calls, 0);
    assert!(
        negative.compared_b_root_to_a_unequal,
        "Production proof must reach B's actual root and compare it unequal to retained A root"
    );
    let negative_a = delta(counters(a, pid), before_a);
    let negative_b = delta(counters(b, pid), before_b);
    assert_eq!(negative_a, [0; 4]);
    assert_eq!(negative_b, [0; 4]);
    trace(
        log,
        "negative-refused",
        serde_json::json!({"a_delta":negative_a,"b_delta":negative_b,"production_text_calls":0,"compared_b_root_to_a_unequal":negative.compared_b_root_to_a_unequal}),
    );
    let mut positive = reader(&ui, source);
    assert!(
        read_bound_selection(&mut positive).is_ok_and(|text| text == EXPECTED),
        "Ordinary A selection must return exact owned text"
    );
    let positive_a = delta(counters(a, pid), before_a);
    assert!(positive_a[0] >= 1);
    assert_eq!(&positive_a[1..], &[1, 1, 0]);
    assert_eq!(delta(counters(b, pid), before_b), [0; 4]);
    trace(
        log,
        "positive-admitted",
        serde_json::json!({"actual_focus_a":true,"text_exact":true,"a_delta":positive_a}),
    );
    command(a, pid, 3);
    let before_password = counters(a, pid);
    let mut protected = reader(&ui, source);
    assert!(
        read_bound_selection(&mut protected).is_err(),
        "Protected virtual ancestor must refuse"
    );
    assert_eq!(delta(counters(a, pid), before_password), [0; 4]);
    assert_eq!(delta(counters(b, pid), before_b), [0; 4]);
    serde_json::json!({"schema":1,"test":"windows-two-window-selection-origin-guard","passed":true,
        "same_pid":true,"distinct_native_windows":true,"canonical_executable_verified":true,
        "foreground_a":true,"focused_b_injected":true,"os_stale_focus_delivery_proven":false,
        "negative_refused_before_text":true,"negative_compared_b_root_to_a_unequal":negative.compared_b_root_to_a_unequal,
        "native_roots_distinct":true,"negative_a_counters_delta":negative_a,"negative_b_counters_delta":negative_b,
        "positive_actual_focused_a":true,"positive_virtual_native_hwnd_zero":true,"positive_text_exact":true,
        "positive_a_counters_delta":positive_a,"focus_readiness_attempts":focus_attempts,
        "password_ancestor_refused_before_text":true})
}

#[test]
fn hosted_owned_two_window_selection_origin_guard() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        eprintln!(
            "Native selection-origin guard not run: requires hosted Windows Actions desktop."
        );
        return;
    }
    let _desktop = crate::browser_page::native_desktop_fixture_lock();
    let artifacts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();
    let report_path = artifacts.join("windows-selection-origin-native-smoke.json");
    let log = artifacts.join("windows-selection-origin-native-smoke.log");
    for path in [&report_path, &log] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => panic!("Could not clear previous owned origin evidence"),
        }
    }
    let directory = tempfile::tempdir().unwrap();
    trace(&log, "compiling", serde_json::json!({}));
    let executable = compile_fixture(directory.path());
    trace(&log, "launching", serde_json::json!({}));
    let mut fixture = Fixture::launch(&executable, directory.path());
    trace(
        &log,
        "observing",
        serde_json::json!({"same_pid":true,"distinct_windows":true}),
    );
    let source = fixture.source.clone();
    let window_b = fixture.window_b;
    let worker_log = log.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("pickle-selection-origin-native-test".into())
        .spawn(move || {
            let result = native_checks(&source, window_b, &worker_log);
            let _ = sender.send(result);
        })
        .unwrap();
    let result = receiver.recv_timeout(Duration::from_secs(10));
    // Close the actual fixture and confirm its exact job is empty even when an
    // assertion or UIA timeout prevented the worker from returning a result.
    fixture.close();
    let until = Instant::now() + Duration::from_secs(1);
    while !worker.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        worker.is_finished(),
        "Owned UIA test worker did not finish; not joining indefinitely"
    );
    assert!(
        worker.join().is_ok(),
        "Owned native origin guard assertion failed"
    );
    let mut report = result.expect("Owned native origin guard exceeded its bounded wait");
    drop(fixture);
    directory.close().unwrap();
    report["ui_worker_joined"] = serde_json::json!(true);
    report["owned_windows_closed"] = serde_json::json!(true);
    report["fixture_process_stopped"] = serde_json::json!(true);
    report["fixture_job_drained"] = serde_json::json!(true);
    report["fixture_directory_removed"] = serde_json::json!(true);
    trace(
        &log,
        "cleaned",
        serde_json::json!({"worker_joined":true,"job_drained":true}),
    );
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    assert!(bytes.len() < 4096);
    std::fs::write(report_path, bytes).unwrap();
}
