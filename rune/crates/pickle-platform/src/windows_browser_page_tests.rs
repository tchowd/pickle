//! Owned synthetic UIA provider coverage, separate from real-browser acceptance.
use super::*;
use ::windows::{
    Win32::{
        Foundation::{CloseHandle, LPARAM, WPARAM},
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    },
    core::PWSTR,
};
use std::{
    cell::{Cell, RefCell},
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
};

const EXPECTED: &str = "https://example.invalid/Path%2Fpart?q=Caf%C3%A9&b=2#route-one";
const COMMAND: u32 = WM_APP + 200;

struct Trace {
    path: PathBuf,
    mode: &'static str,
    stage: &'static str,
}
impl Trace {
    fn new(artifacts: &Path) -> Self {
        Self::named(artifacts, "windows-browser-page-native-smoke.log")
    }
    fn named(artifacts: &Path, name: &'static str) -> Self {
        std::fs::create_dir_all(artifacts).unwrap();
        let path = artifacts.join(name);
        std::fs::write(&path, []).unwrap();
        let mut trace = Self {
            path,
            mode: "fixture",
            stage: "started",
        };
        trace.step("fixture", "started");
        trace
    }
    fn append(&self, error: Option<&'static str>) -> std::io::Result<()> {
        let line = if let Some(error) = error {
            format!("mode={} stage={} error={error}\n", self.mode, self.stage)
        } else {
            format!("mode={} stage={}\n", self.mode, self.stage)
        };
        if std::fs::metadata(&self.path)?.len() + line.len() as u64 > 16 * 1024 {
            return Ok(());
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(&self.path)?
            .write_all(line.as_bytes())
    }
    fn step(&mut self, mode: &'static str, stage: &'static str) {
        self.mode = mode;
        self.stage = stage;
        self.append(None).unwrap();
    }
    fn metadata(&self, stage: &'static str, values: serde_json::Value) {
        // Only typed scalar diagnostics from this owned provider are retained.
        // Even an accidental future diagnostic string cannot log source text.
        let mut safe = serde_json::Map::new();
        safe.insert("mode".into(), serde_json::json!(self.mode));
        safe.insert("stage".into(), serde_json::json!(stage));
        for key in [
            "observation_index",
            "capture_count",
            "accepted",
            "elapsed_ms",
            "remaining_ms",
            "cancelled",
            "hresult",
            "nodes",
            "documents",
            "nullable_navigation_ends",
            "last_control_type",
            "control_type",
            "custom_variant_type",
            "custom_variant_bool",
            "document_reads",
            "nested_reads",
            "edit_reads",
            "link_reads",
            "url_matches_expected",
            "pid_matches_source",
            "window_matches_source",
            "identity_present",
            "url_utf8_bytes",
            "foreign_child_navigations",
            "root_pid",
            "node_pid",
            "unanchored_foreign_child_navigations",
            "pruned_foreign_native_branches",
            "anchored_foreign_native_branches",
            "virtual_nodes_using_path_anchor",
            "candidate_native_anchors",
            "anchor_revalidations",
            "document_pid",
            "node_hwnd_owner_pid",
            "node_executable_matches_source",
            "helper_document_reads",
            "helper_nested_reads",
            "helper_edit_reads",
            "helper_link_reads",
            "helper_root_child_navigation_delta",
            "raw_ancestry_checks",
            "raw_ancestry_nodes",
            "filtered_discovery",
            "focus_snapshot_available",
            "native_snapshot_available",
            "get_object_requests",
            "all_get_object_requests",
            "client_get_object_requests",
            "other_get_object_requests",
            "aliased_root_get_object_requests",
            "get_focus_entries",
            "get_focus_returns",
            "set_focus_requests",
            "focus_property_requests",
            "native_focus_requests",
            "shown_published",
            "foreground_owned",
            "native_focus_owned",
            "internal_focus_present",
        ] {
            if let Some(value) = values
                .get(key)
                .filter(|value| value.is_number() || value.is_boolean())
            {
                safe.insert(key.into(), value.clone());
            }
        }
        let mut bytes = serde_json::to_vec(&safe).unwrap();
        assert!(bytes.len() < 4096);
        bytes.push(b'\n');
        if std::fs::metadata(&self.path).unwrap().len() + bytes.len() as u64 > 16 * 1024 {
            return;
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(&self.path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let _ = self.append(Some("assertion-or-unavailable"));
        }
    }
}

struct Fixture {
    child: Child,
    source: CapturedSelection,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
impl Fixture {
    fn launch(executable: &Path, directory: &Path, mode: &str, case: usize) -> Self {
        let ready = directory.join(format!("ready-{case}.json"));
        let entered = directory.join(format!("entered-{case}"));
        let child = Command::new(executable)
            .arg(mode)
            .arg(&ready)
            .arg(entered)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let mut fixture = Self {
            child,
            source: CapturedSelection {
                text: String::new(),
                app_name: "Owned synthetic UIA provider".into(),
                app_id: String::new(),
                bounds: None,
                source_url: None,
                source_pid: 0,
                source_window: None,
            },
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let metadata: serde_json::Value = loop {
            assert!(
                Instant::now() < deadline,
                "Owned UIA provider did not become ready"
            );
            assert!(
                fixture.child.try_wait().unwrap().is_none(),
                "Owned UIA provider exited early"
            );
            if let Ok(bytes) = std::fs::read(&ready) {
                assert!(bytes.len() < 1024);
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    break value;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let pid = fixture.child.id();
        assert_eq!(metadata["pid"].as_u64(), Some(u64::from(pid)));
        let window = metadata["hwnd"].as_u64().unwrap();
        let hwnd = HWND(window as *mut _);
        let mut actual_pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut actual_pid)) };
        assert_eq!(actual_pid, pid);
        let process =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.unwrap();
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        let observed = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut size,
            )
        };
        let _ = unsafe { CloseHandle(process) };
        observed.unwrap();
        let path = String::from_utf16(&path[..size as usize]).unwrap();
        assert_eq!(
            std::fs::canonicalize(&path).unwrap(),
            std::fs::canonicalize(executable).unwrap()
        );
        fixture.source.source_pid = pid;
        fixture.source.source_window = Some(window);
        fixture.source.app_id = format!("windows:{path}");
        loop {
            assert!(
                Instant::now() < deadline,
                "Owned UIA provider window did not receive foreground focus"
            );
            assert!(fixture.child.try_wait().unwrap().is_none());
            let _ = unsafe { SetForegroundWindow(hwnd) };
            fixture.command(4).unwrap();
            if unsafe { GetForegroundWindow() } == hwnd {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        fixture
    }
    fn command(&self, code: usize) -> Result<usize, PlatformError> {
        command(&self.source, code)
    }
    fn close(mut self) {
        let hwnd = HWND(self.source.source_window.unwrap() as *mut _);
        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Owned UIA provider did not close"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        assert!(!unsafe { IsWindow(Some(hwnd)) }.as_bool() || owner != self.child.id());
    }
}

fn command(source: &CapturedSelection, code: usize) -> Result<usize, PlatformError> {
    let hwnd = HWND(source.source_window.unwrap() as *mut _);
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != source.source_pid {
        return Err(unavailable());
    }
    let mut result = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            COMMAND,
            WPARAM(code),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK | SMTO_ERRORONEXIT,
            250,
            Some(&mut result),
        )
    };
    if sent.0 == 0 {
        Err(unavailable())
    } else {
        Ok(result)
    }
}

fn focus_snapshot(callbacks: Option<usize>, native: Option<usize>) -> serde_json::Value {
    let mut values = serde_json::json!({
        "focus_snapshot_available":callbacks.is_some(),
        "native_snapshot_available":native.is_some(),
    });
    if let Some(bits) = callbacks {
        for (index, key) in [
            "get_object_requests",
            "get_focus_entries",
            "get_focus_returns",
            "set_focus_requests",
            "all_get_object_requests",
            "client_get_object_requests",
            "other_get_object_requests",
            "aliased_root_get_object_requests",
        ]
        .into_iter()
        .enumerate()
        {
            values[key] = serde_json::json!((bits >> (index * 8)) & 0xff);
        }
    }
    if let Some(bits) = native {
        values["native_focus_requests"] = serde_json::json!(bits & 0xff);
        values["focus_property_requests"] = serde_json::json!((bits >> 8) & 0xff);
        for (index, key) in [
            "shown_published",
            "foreground_owned",
            "native_focus_owned",
            "internal_focus_present",
        ]
        .into_iter()
        .enumerate()
        {
            values[key] = serde_json::json!(bits & (1 << (16 + index)) != 0);
        }
    }
    values
}

#[test]
fn synthetic_focus_diagnostics_preserve_unknown_and_saturated_callback_counts() {
    let present = focus_snapshot(Some(0xff060504ff030201), Some(0x000f0504));
    assert_eq!(present["get_object_requests"], 1);
    assert_eq!(present["get_focus_entries"], 2);
    assert_eq!(present["get_focus_returns"], 3);
    assert_eq!(present["set_focus_requests"], 255);
    assert_eq!(present["all_get_object_requests"], 4);
    assert_eq!(present["client_get_object_requests"], 5);
    assert_eq!(present["other_get_object_requests"], 6);
    assert_eq!(present["aliased_root_get_object_requests"], 255);
    assert_eq!(present["native_focus_requests"], 4);
    assert_eq!(present["focus_property_requests"], 5);
    for key in [
        "shown_published",
        "foreground_owned",
        "native_focus_owned",
        "internal_focus_present",
    ] {
        assert_eq!(present[key], true);
    }
    let unavailable = focus_snapshot(None, None);
    assert_eq!(
        unavailable,
        serde_json::json!({"focus_snapshot_available":false,"native_snapshot_available":false})
    );
    let not_focused = focus_snapshot(Some(0), Some(0x00010000));
    assert_eq!(not_focused["shown_published"], true);
    assert_eq!(not_focused["foreground_owned"], false);
    assert_eq!(not_focused["get_focus_entries"], 0);
}

fn framework_path(path: &Path) -> PathBuf {
    // Framework csc predates extended Win32 paths and can misparse mixed
    // separators/parent components. Only an owned existing temp directory is
    // canonicalized; use its normal drive/UNC form for every compiler argument.
    let canonical = std::fs::canonicalize(path).unwrap();
    let value = canonical.to_str().unwrap();
    let plain = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
    };
    PathBuf::from(plain)
}

fn compile_provider(directory: &Path) -> PathBuf {
    let directory = framework_path(directory);
    let source = directory.join("BrowserDocumentFixture.cs");
    // Embed only this checked-in synthetic fixture at Rust compile time. Runtime
    // compilation no longer depends on repository cwd or slash interpretation.
    std::fs::write(
        &source,
        include_bytes!("../../../native/windows/BrowserDocumentFixture.cs"),
    )
    .unwrap();
    let framework = framework_path(
        &PathBuf::from(std::env::var_os("WINDIR").unwrap())
            .join("Microsoft.NET/Framework64/v4.0.30319"),
    );
    let executable = directory.join("PickleDocumentFixture.exe");
    let mut compiler = Command::new(framework.join("csc.exe"));
    compiler
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
        compiler.arg(format!("/r:{}", framework.join(reference).display()));
    }
    for reference in [
        "WindowsBase.dll",
        "UIAutomationClient.dll",
        "UIAutomationTypes.dll",
        "UIAutomationProvider.dll",
    ] {
        compiler.arg(format!(
            "/r:{}",
            framework.join("WPF").join(reference).display()
        ));
    }
    let output = compiler.arg(source).stdin(Stdio::null()).output().unwrap();
    assert!(
        output.stdout.len() + output.stderr.len() < 16000,
        "Unexpected compiler output size"
    );
    assert!(
        output.status.success(),
        "Owned C# provider typecheck failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

struct ObservationDiagnostic {
    stage: &'static str,
    values: serde_json::Value,
}
struct Verification {
    accepted: bool,
    captures: usize,
    reads: usize,
    diagnostics: Vec<ObservationDiagnostic>,
}
fn verify(source: CapturedSelection, phase: &'static str) -> Result<Verification, PlatformError> {
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    on_document_worker(cancel, move || {
        let reader = DocumentReader::new(&source, worker_cancel.clone())?;
        // Prove that the owned custom provider, not a generic WinForms/MSAA
        // fallback tree, is published before exercising the production reader.
        let id = unsafe { reader.root.CurrentAutomationId() }.map_err(|_| unavailable())?;
        if id != "fixture-root" {
            return Err(unavailable());
        }
        let captured = Cell::new(0usize);
        let reads = Cell::new(0usize);
        let diagnostics = RefCell::new(Vec::new());
        let outcome = verified_capture(
            &source,
            EXPECTED,
            &worker_cancel,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 2 {
                    if phase == "before-navigation" {
                        command(&source, 1)?;
                    }
                    if phase == "identity-change" {
                        command(&source, 2)?;
                    }
                }
                let result = reader.observe();
                let mut values = reader.failure_diagnostics();
                values["observation_index"] = serde_json::json!(reads.get());
                values["capture_count"] = serde_json::json!(captured.get());
                values["accepted"] = serde_json::json!(result.is_ok());
                if let Ok(observation) = &result {
                    values["url_matches_expected"] =
                        serde_json::json!(document_urls_match(EXPECTED, &observation.url));
                    values["pid_matches_source"] =
                        serde_json::json!(observation.pid == source.source_pid);
                    values["window_matches_source"] =
                        serde_json::json!(Some(observation.window) == source.source_window);
                    values["identity_present"] =
                        serde_json::json!(!observation.identity.is_empty());
                    values["url_utf8_bytes"] = serde_json::json!(observation.url.len());
                }
                diagnostics.borrow_mut().push(ObservationDiagnostic {
                    stage: reader.failure_stage(),
                    values,
                });
                result
            },
            || {
                captured.set(captured.get() + 1);
                if phase == "after-navigation" {
                    command(&source, 1)?;
                }
                Ok(()) // No pixels, capture permissions, OCR or package identity.
            },
        );
        Ok(Verification {
            accepted: outcome.is_ok(),
            captures: captured.get(),
            reads: reads.get(),
            diagnostics: diagnostics.into_inner(),
        })
    })
}

fn wait_admission_recovery() {
    wait_admission_recovery_until(Instant::now() + Duration::from_secs(5));
}

fn wait_admission_recovery_until(deadline: Instant) {
    loop {
        assert!(
            Instant::now() < deadline,
            "Isolated document worker did not release admission after finishing"
        );
        if on_document_worker(CancellationToken::new(), || Ok(())).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn observed_worker_lifecycle_distinguishes_admission_teardown_and_unfinished_object_drop() {
    // The same ACTIVE and fixture lock are used by the actual browser/provider
    // checks. These gates never hold admission while either owns foreground.
    let _fixture_lock = native_browser_fixture_lock();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let probe = DocumentWorkerProbe::default();
    let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let work_called = called.clone();
    assert!(
        on_document_worker_observed(cancelled, probe.clone(), move || {
            work_called.store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        })
        .is_err()
    );
    assert!(!called.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(probe.snapshot().caller_exit, WorkerCallerExit::PreCancelled);
    assert_eq!(probe.snapshot().mta_hresult, None);
    assert_eq!(probe.snapshot().event_count, 0);

    let probe = DocumentWorkerProbe::default();
    assert_eq!(
        on_document_worker_observed(CancellationToken::new(), probe.clone(), || Ok(37)).unwrap(),
        37
    );
    let delivered_snapshot = probe.snapshot();
    assert_eq!(
        delivered_snapshot.caller_exit,
        WorkerCallerExit::DeliveredOk
    );
    // Do not turn a received result into an assertion that the sender already
    // released its flight. Either ordering is valid on the actual OS threads.
    assert_eq!(
        delivered_snapshot.json()["partial"],
        !delivered_snapshot.lifecycle_complete()
    );
    assert!(probe.wait_released(Duration::from_secs(5)));
    let complete = probe.snapshot();
    assert!(complete.mta_hresult.is_some_and(|status| status >= 0));
    assert_eq!(complete.receiver_connected, Some(true));
    assert!(!complete.events_truncated);
    assert_eq!(
        &complete.events[..complete.event_count],
        &[
            WorkerPhase::SpawnRequested,
            WorkerPhase::ThreadStarted,
            WorkerPhase::MtaInitializing,
            WorkerPhase::MtaInitReturned,
            WorkerPhase::WorkRunning,
            WorkerPhase::WorkReturned,
            WorkerPhase::ComTeardownEntering,
            WorkerPhase::ComTeardownReturned,
            WorkerPhase::SendEntering,
            WorkerPhase::SendReturned,
            WorkerPhase::DeliveryResultDropEntering,
            WorkerPhase::DeliveryResultDropReturned,
            WorkerPhase::FlightReleased,
        ]
    );

    let probe = DocumentWorkerProbe::default();
    assert!(
        on_document_worker_observed::<()>(CancellationToken::new(), probe.clone(), || Err(
            unavailable()
        ))
        .is_err()
    );
    assert!(probe.wait_released(Duration::from_secs(5)));
    let failed = probe.snapshot();
    assert_eq!(failed.caller_exit, WorkerCallerExit::DeliveredError);
    assert!(failed.events[..failed.event_count].contains(&WorkerPhase::ComTeardownReturned));

    let probe = DocumentWorkerProbe::default();
    assert!(
        on_document_worker_observed::<()>(CancellationToken::new(), probe.clone(), || {
            panic!("owned worker unwind fixture");
        })
        .is_err()
    );
    assert!(probe.wait_released(Duration::from_secs(5)));
    let unwound = probe.snapshot();
    assert_eq!(unwound.caller_exit, WorkerCallerExit::Disconnected);
    assert!(unwound.events[..unwound.event_count].contains(&WorkerPhase::ComTeardownReturned));
    assert!(!unwound.events[..unwound.event_count].contains(&WorkerPhase::WorkReturned));

    // A real work-local destructor blocks before work() returns. This is a
    // synthetic resource gate, not a claim to simulate a blocked COM release.
    struct OwnedDropGate {
        probe: DocumentWorkerProbe,
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Drop for OwnedDropGate {
        fn drop(&mut self) {
            self.probe.task(WorkerTaskPhase::ObjectDropEntering, None);
            let _ = self.entered.send(());
            let _ = self.release.recv();
            self.probe.task(WorkerTaskPhase::ObjectDropReturned, None);
        }
    }
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let probe = DocumentWorkerProbe::default();
    let task_probe = probe.clone();
    let result = on_document_worker_observed(CancellationToken::new(), probe.clone(), move || {
        let _owned = OwnedDropGate {
            probe: task_probe,
            entered: entered_tx,
            release: release_rx,
        };
        Ok(())
    });
    let entered = entered_rx.recv_timeout(Duration::from_secs(1));
    let partial = probe.snapshot();
    let release_sent = release_tx.send(());
    let released = probe.wait_released(Duration::from_secs(5));
    assert!(entered.is_ok());
    assert!(release_sent.is_ok());
    assert!(released);
    assert!(result.is_err());
    assert_eq!(partial.caller_exit, WorkerCallerExit::Deadline);
    assert_eq!(partial.worker_phase, WorkerPhase::WorkRunning);
    assert_eq!(partial.task_phase, WorkerTaskPhase::ObjectDropEntering);
    let json = partial.json();
    assert_eq!(json["partial"], true);
    assert_eq!(json["lifecycle_complete"], false);
    for private in ["reads", "capture_count", "url", "text", "pid", "path"] {
        assert!(json.get(private).is_none());
    }
    assert!(serde_json::to_vec(&json).unwrap().len() < 2048);
    assert_eq!(probe.snapshot().caller_exit, WorkerCallerExit::Deadline);
    assert_eq!(
        probe.snapshot().task_phase,
        WorkerTaskPhase::ObjectDropReturned
    );
}

#[test]
fn owned_native_document_provider() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        eprintln!(
            "Synthetic native document provider not run: requires hosted Windows Actions desktop."
        );
        return;
    }
    let _native = native_browser_fixture_lock();
    let artifacts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts");
    let evidence = artifacts.join("windows-browser-page-native-smoke.json");
    let mut trace = Trace::new(&artifacts);
    match std::fs::remove_file(&evidence) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => panic!("Could not remove previous synthetic document report"),
    }
    let directory = tempfile::tempdir().unwrap();
    trace.step("fixture", "compiling");
    let executable = compile_provider(directory.path());
    trace.step("fixture", "compiled");
    let mut case = 0;
    let mut ownership_cases = vec![];
    for (mode, phase, success, captures) in [
        ("stable", "stable", true, 1),
        ("wrong-value", "stable", false, 0),
        ("false-root", "stable", false, 0),
        ("decoys-only", "stable", false, 0),
        ("omnibox-only", "stable", false, 0),
        ("link-only", "stable", false, 0),
        ("multiple-roots", "stable", false, 0),
        ("hidden-root", "stable", false, 0),
        ("password", "stable", false, 0),
        ("stable", "before-navigation", false, 0),
        ("stable", "after-navigation", false, 1),
        ("stable", "identity-change", false, 0),
        ("foreign-pane", "stable", false, 0),
        ("foreign-document-only", "stable", false, 0),
        ("foreign-wrapper-main", "stable", false, 0),
        ("foreign-root", "stable", false, 0),
        ("wide-root", "stable", false, 0),
    ] {
        let trace_mode = if phase == "stable" { mode } else { phase };
        trace.step(trace_mode, "launching");
        let fixture = Fixture::launch(&executable, directory.path(), mode, case);
        trace.step(trace_mode, "ready");
        case += 1;
        // Read-only WM_APP frames do not query UIA, URL, or Value and do not
        // gate/retry the following single production attempt. They distinguish
        // a pumping native owner from first COM/provider callback readiness.
        if case == 1 {
            trace.metadata(
                "provider-focus-before",
                focus_snapshot(fixture.command(9).ok(), fixture.command(10).ok()),
            );
        }
        trace.step(trace_mode, "observing");
        let result = verify(fixture.source.clone(), phase);
        if case == 1 {
            trace.metadata(
                "provider-focus-after",
                focus_snapshot(fixture.command(9).ok(), fixture.command(10).ok()),
            );
        }
        if result.is_err() {
            trace.step(trace_mode, "adapter-unavailable");
        }
        let outcome = result.unwrap();
        trace.step(
            trace_mode,
            if outcome.accepted {
                "accepted"
            } else {
                "rejected"
            },
        );
        for observation in &outcome.diagnostics {
            trace.metadata(observation.stage, observation.values.clone());
        }
        let provider_counts = fixture.command(3);
        if provider_counts.is_err() {
            trace.step(trace_mode, "provider-counters-unavailable");
        }
        let counts = provider_counts.unwrap();
        trace.metadata("provider-counters",serde_json::json!({"observation_index":outcome.reads,"capture_count":outcome.captures,"document_reads":counts&0xff,"nested_reads":(counts>>8)&0xff,"edit_reads":(counts>>16)&0xff,"link_reads":(counts>>24)&0xff}));
        assert_eq!(
            outcome.accepted, success,
            "{mode}/{phase}, observations={}, captures={}",
            outcome.reads, outcome.captures
        );
        assert_eq!(outcome.captures, captures, "{mode}/{phase}");
        assert_eq!(
            (counts >> 8) & 0xff,
            0,
            "No iframe/false-root nested document Value may be read"
        );
        assert_eq!(
            (counts >> 16) & 0xff,
            0,
            "No omnibox/edit Value may be read"
        );
        assert_eq!((counts >> 24) & 0xff, 0, "No hyperlink Value may be read");
        if matches!(mode, "false-root" | "omnibox-only" | "link-only") {
            assert!(
                outcome.diagnostics.iter().any(|observation| {
                    observation.stage == "document-count"
                        && observation.values["documents"].as_u64() == Some(0)
                        && observation.values["filtered_discovery"] == true
                }),
                "{mode}: focused discovery must retain non-ControlView boundaries"
            );
        }
        if success {
            assert!(
                counts & 0xff >= 3,
                "Accepted capture needs first,pre,post document Value reads"
            );
            assert_eq!(outcome.reads, 3);
            assert!(
                outcome.diagnostics.iter().all(|observation| {
                    observation.values["nullable_navigation_ends"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                }),
                "Accepted observations must execute native successful-null traversal ends"
            );
        }
        if mode.starts_with("foreign-") || mode == "wide-root" {
            assert_eq!(
                counts & 0xff,
                0,
                "{mode}: rejecting traversal must not read the main document Value"
            );
            let child_navigations = fixture.command(5).unwrap();
            trace.metadata(
                "ownership-branch-counters",
                serde_json::json!({"foreign_child_navigations":child_navigations}),
            );
            // Foreign/unknown PID nodes are retained by focused discovery,
            // even when generic same-PID Pane wrappers are filtered out.
            assert_eq!(child_navigations, 0, "{mode}: no foreign descendants");
            let observed_boundary = if mode == "wide-root" {
                outcome.diagnostics.iter().any(|observation| {
                    observation.stage == "traversal-node-limit"
                        && observation.values["nodes"].as_u64() == Some(193)
                })
            } else {
                let (stage, pid_key) = if mode == "foreign-root" {
                    ("root-pid", "root_pid")
                } else {
                    ("traversal-pid", "node_pid")
                };
                outcome.diagnostics.iter().any(|observation| {
                    observation.stage == stage
                        && observation.values[pid_key].as_u64()
                            == Some(u64::from(fixture.source.source_pid) + 1)
                })
            };
            assert!(
                observed_boundary,
                "{mode}: refusal must reach the intended native PID/width boundary"
            );
            ownership_cases.push(serde_json::json!({"mode":mode,"accepted":outcome.accepted,"capture_count":outcome.captures,"document_reads":counts&0xff,"nested_reads":(counts>>8)&0xff,"foreign_child_navigations":child_navigations,"intended_boundary_observed":observed_boundary}));
        }
        trace.step(trace_mode, "closing");
        fixture.close();
        trace.step(trace_mode, "closed");
    }

    // Synthetic provider graph, backed by a genuinely separate owned process/
    // window. All but the explicit wrong-executable case use the same canonical
    // EXE. UIA resolves the real helper HWND's provider, whose counters must
    // prove the cross-process reads. This remains synthetic, not a browser.
    let mut native_anchor_cases = vec![];
    {
        // Keep each trace bounded without starving later anchor failures of
        // diagnostics after the existing refusal/timeout corpus fills its log.
        let mut trace = Trace::named(&artifacts, "windows-browser-page-anchors-native-smoke.log");
        for mode in [
            "native-embedded-anchor",
            "native-outside-anchor",
            "native-sibling-escape",
            "native-owner-mismatch",
            "native-wrong-executable",
        ] {
            trace.step(mode, "launching");
            let source_case = case;
            let fixture = Fixture::launch(&executable, directory.path(), mode, case);
            case += 1;
            let helper_executable = if mode == "native-wrong-executable" {
                let copy = directory.path().join("OwnedDifferentPathFixture.exe");
                std::fs::copy(&executable, &copy).unwrap();
                assert_ne!(
                    std::fs::canonicalize(&executable).unwrap(),
                    std::fs::canonicalize(&copy).unwrap()
                );
                assert_eq!(
                    std::fs::read(&executable).unwrap(),
                    std::fs::read(&copy).unwrap()
                );
                copy
            } else {
                executable.clone()
            };
            let helper_mode = match mode {
                // An empty anchored branch is required: a global PID leak
                // would then expose exactly the separate fake sibling Doc.
                "native-sibling-escape" => "native-helper-empty",
                "native-owner-mismatch" => "native-helper-owner-mismatch",
                _ => "native-helper",
            };
            let helper = Fixture::launch(&helper_executable, directory.path(), helper_mode, case);
            case += 1;
            assert_ne!(fixture.source.source_pid, helper.source.source_pid);
            let same_executable = fixture.source.app_id == helper.source.app_id;
            assert_eq!(same_executable, mode != "native-wrong-executable");
            let source_hwnd = HWND(fixture.source.source_window.unwrap() as *mut _);
            let helper_hwnd = HWND(helper.source.source_window.unwrap() as *mut _);
            let embedded = mode != "native-outside-anchor";
            if embedded {
                let style = unsafe { GetWindowLongPtrW(helper_hwnd, GWL_STYLE) } as u32;
                unsafe {
                    SetWindowLongPtrW(
                        helper_hwnd,
                        GWL_STYLE,
                        ((style | WS_CHILD.0) & !WS_POPUP.0) as isize,
                    );
                    // SetParent may return successful NULL for a formerly
                    // top-level window. Observe its new parent/root explicitly.
                    let _ = SetParent(helper_hwnd, Some(source_hwnd));
                }
                assert_eq!(unsafe { GetParent(helper_hwnd) }.unwrap(), source_hwnd);
                assert_ne!(
                    unsafe { GetWindowLongPtrW(helper_hwnd, GWL_STYLE) } as u32 & WS_CHILD.0,
                    0
                );
            }
            let mut native_owner = 0;
            unsafe { GetWindowThreadProcessId(helper_hwnd, Some(&mut native_owner)) };
            assert_eq!(native_owner, helper.source.source_pid);
            assert_eq!(
                unsafe { GetAncestor(helper_hwnd, GA_ROOT) },
                if embedded { source_hwnd } else { helper_hwnd }
            );
            std::fs::write(
                directory
                    .path()
                    .join(format!("entered-{source_case}.native-anchor")),
                format!(
                    "{}\n{}",
                    helper.source.source_pid,
                    helper.source.source_window.unwrap()
                ),
            )
            .unwrap();
            fixture.command(6).unwrap();
            let focused = Instant::now();
            loop {
                fixture.command(4).unwrap();
                if unsafe { GetForegroundWindow() } == source_hwnd {
                    break;
                }
                assert!(focused.elapsed() < Duration::from_secs(2));
                std::thread::sleep(Duration::from_millis(20));
            }
            let helper_navigations_before = helper.command(8).unwrap();
            trace.step(mode, "observing");
            let outcome = verify(fixture.source.clone(), "stable").unwrap();
            for observation in &outcome.diagnostics {
                trace.metadata(observation.stage, observation.values.clone());
            }
            let counts = fixture.command(3).unwrap();
            let child_navigations = fixture.command(5).unwrap();
            let unanchored_navigations = fixture.command(7).unwrap();
            let helper_counts = helper.command(3).unwrap();
            let helper_navigation_delta = helper
                .command(8)
                .unwrap()
                .checked_sub(helper_navigations_before)
                .unwrap();
            trace.metadata("native-anchor-counters",serde_json::json!({"accepted":outcome.accepted,"capture_count":outcome.captures,"document_reads":counts&0xff,"nested_reads":(counts>>8)&0xff,"foreign_child_navigations":child_navigations,"unanchored_foreign_child_navigations":unanchored_navigations,"helper_document_reads":helper_counts&0xff,"helper_nested_reads":(helper_counts>>8)&0xff,"helper_edit_reads":(helper_counts>>16)&0xff,"helper_link_reads":(helper_counts>>24)&0xff,"helper_root_child_navigation_delta":helper_navigation_delta}));
            let success = matches!(mode, "native-embedded-anchor" | "native-outside-anchor");
            assert_eq!(outcome.accepted, success, "{mode}");
            assert_eq!(outcome.captures, usize::from(success), "{mode}");
            assert_eq!((counts >> 8) & 0xff, 0, "{mode}: no decoy Document Value");
            assert_eq!((counts >> 16) & 0xff, 0);
            assert_eq!((counts >> 24) & 0xff, 0);
            assert_eq!(
                (helper_counts >> 8) & 0xff,
                0,
                "No helper iframe/decoy Value"
            );
            assert_eq!((helper_counts >> 16) & 0xff, 0, "No helper edit Value");
            assert_eq!((helper_counts >> 24) & 0xff, 0, "No helper hyperlink Value");
            if success {
                assert_eq!(outcome.reads, 3);
                assert!(outcome.diagnostics.iter().all(|observation| {
                    observation.values["filtered_discovery"] == true
                        && observation.values["raw_ancestry_checks"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                }));
            }
            assert_eq!(
                counts & 0xff,
                if mode == "native-outside-anchor" {
                    3
                } else {
                    0
                },
                "{mode}: only the outside-anchor case may read the source main Doc"
            );
            assert_eq!(
                helper_counts & 0xff,
                if mode == "native-embedded-anchor" {
                    3
                } else {
                    0
                },
                "{mode}: only the embedded anchor may read the actual helper Doc"
            );
            match mode {
                "native-embedded-anchor" => {
                    assert!(
                        helper_navigation_delta >= 1,
                        "Actual helper provider traversed"
                    );
                    assert!(outcome.diagnostics.iter().all(|observation| {
                        observation.values["anchored_foreign_native_branches"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                            && observation.values["virtual_nodes_using_path_anchor"]
                                .as_u64()
                                .is_some_and(|count| count > 0)
                            && observation.values["candidate_native_anchors"]
                                .as_u64()
                                .is_some_and(|count| count >= 1)
                            && observation.values["anchor_revalidations"]
                                .as_u64()
                                .is_some_and(|count| count >= 2)
                            && observation.values["document_pid"].as_u64()
                                == Some(u64::from(helper.source.source_pid))
                    }));
                }
                "native-outside-anchor" => {
                    assert_eq!(child_navigations, 0, "Outside-root branch not traversed");
                    assert!(outcome.diagnostics.iter().all(|observation| {
                        observation.values["pruned_foreign_native_branches"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                    }));
                }
                "native-sibling-escape" => {
                    assert_eq!(
                        unanchored_navigations, 0,
                        "Sibling cannot borrow anchor PID"
                    );
                    assert!(outcome.diagnostics.iter().any(|observation| {
                        observation.stage == "traversal-pid"
                            && observation.values["node_pid"].as_u64()
                                == Some(u64::from(helper.source.source_pid))
                            && observation.values["anchored_foreign_native_branches"]
                                .as_u64()
                                .is_some_and(|count| count > 0)
                    }));
                }
                "native-owner-mismatch" => {
                    assert_eq!(
                        child_navigations, 0,
                        "Mismatched-owner branch not traversed"
                    );
                    assert_eq!(unanchored_navigations, 0);
                    assert!(outcome.diagnostics.iter().any(|observation| {
                        observation.values["node_hwnd_owner_pid"].as_u64()
                            == Some(u64::from(helper.source.source_pid))
                            && observation.values["node_pid"].as_u64()
                                != Some(u64::from(helper.source.source_pid))
                    }));
                }
                "native-wrong-executable" => {
                    assert_eq!(
                        child_navigations, 0,
                        "Different-executable branch not traversed"
                    );
                    assert_eq!(unanchored_navigations, 0);
                    assert!(outcome.diagnostics.iter().any(|observation| {
                        observation.stage == "native-anchor-binding"
                            && observation.values["node_executable_matches_source"] == false
                            && observation.values["node_hwnd_owner_pid"].as_u64()
                                == Some(u64::from(helper.source.source_pid))
                            && observation.values["node_pid"].as_u64()
                                == Some(u64::from(helper.source.source_pid))
                    }));
                }
                _ => unreachable!(),
            }
            // Recheck real native ownership before teardown rather than recording
            // command-line intent as proof of embedding or source containment.
            let mut after_owner = 0;
            unsafe { GetWindowThreadProcessId(helper_hwnd, Some(&mut after_owner)) };
            assert_eq!(after_owner, helper.source.source_pid);
            assert_eq!(
                unsafe { GetAncestor(helper_hwnd, GA_ROOT) },
                if embedded { source_hwnd } else { helper_hwnd }
            );
            trace.step(mode, "closing-helper");
            helper.close();
            fixture.close();
            trace.step(mode, "closed");
            native_anchor_cases.push(serde_json::json!({"mode":mode,"accepted":outcome.accepted,"capture_count":outcome.captures,"source_document_reads":counts&0xff,"source_nested_reads":(counts>>8)&0xff,"source_foreign_child_navigations":child_navigations,"source_unanchored_foreign_child_navigations":unanchored_navigations,"helper_document_reads":helper_counts&0xff,"helper_nested_reads":(helper_counts>>8)&0xff,"helper_root_child_navigation_delta":helper_navigation_delta,"actual_helper_provider_counters_checked":true,"distinct_owned_pid_observed":true,"canonical_same_executable_observed":same_executable,"canonical_different_executable_path_observed":!same_executable,"native_hwnd_owner_observed":true,"native_root_matches_source":embedded,"helper_and_source_closed":true}));
        }
    }

    // A genuine delayed external provider must fail within the outer bound;
    // stopping that exact owned process releases any outstanding COM request.
    trace.step("slow", "launching");
    let mut slow = Fixture::launch(&executable, directory.path(), "slow", case);
    trace.step("slow", "ready");
    let source = slow.source.clone();
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let started = Instant::now();
    trace.step("slow", "observing");
    assert!(
        on_document_worker(token, move || DocumentReader::new(&source, worker_token)?
            .observe())
        .is_err()
    );
    let provider_timeout_ms = started.elapsed().as_millis();
    trace.step("slow", "unavailable");
    assert!(
        provider_timeout_ms < 3500,
        "Native provider timeout exceeded bounded caller wait"
    );
    assert!(
        directory.path().join(format!("entered-{case}")).is_file(),
        "The delayed provider's Value getter must actually have been entered"
    );
    slow.child.kill().unwrap();
    slow.child.wait().unwrap();
    drop(slow);
    wait_admission_recovery();
    trace.step("slow", "stopped-and-admission-recovered");

    // Deterministic outer timeout and cancellation while the isolated MTA is
    // actually live, independent of UIA's own short RPC transaction timeouts.
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let timeout_probe = DocumentWorkerProbe::default();
    let timeout_token = CancellationToken::new();
    let started = Instant::now();
    trace.step("worker-timeout", "observing");
    let outcome =
        on_document_worker_observed(timeout_token.clone(), timeout_probe.clone(), move || {
            let _ = release_rx.recv();
            Ok(())
        });
    let outer_timeout_ms = started.elapsed().as_millis();
    let timeout_snapshot = timeout_probe.snapshot();
    let busy_probe = DocumentWorkerProbe::default();
    let entered_busy = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered_busy_work = entered_busy.clone();
    let busy =
        on_document_worker_observed(CancellationToken::new(), busy_probe.clone(), move || {
            entered_busy_work.store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        });
    // Always release owned delayed work before asserting its recorded state.
    release_tx.send(()).unwrap();
    let recovery_deadline = Instant::now() + Duration::from_secs(5);
    let released =
        timeout_probe.wait_released(recovery_deadline.saturating_duration_since(Instant::now()));
    wait_admission_recovery_until(recovery_deadline);
    assert!(outcome.is_err());
    assert!((1800..3500).contains(&outer_timeout_ms));
    assert!(
        !timeout_token.is_cancelled(),
        "Observation must not change the direct caller's token semantics"
    );
    assert_eq!(timeout_snapshot.caller_exit, WorkerCallerExit::Deadline);
    assert_eq!(timeout_snapshot.worker_phase, WorkerPhase::WorkRunning);
    assert_eq!(timeout_snapshot.json()["partial"], true);
    assert!(
        busy.is_err(),
        "Admission must remain held by the unfinished worker"
    );
    assert!(!entered_busy.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(busy_probe.snapshot().caller_exit, WorkerCallerExit::Busy);
    assert_eq!(busy_probe.snapshot().event_count, 0);
    assert!(released);
    assert_eq!(
        timeout_probe.snapshot().caller_exit,
        WorkerCallerExit::Deadline
    );
    assert_eq!(timeout_probe.snapshot().receiver_connected, Some(false));
    trace.step("worker-timeout", "admission-recovered");

    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let cancelling = std::thread::spawn(move || {
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        signal.cancel();
    });
    let started = Instant::now();
    trace.step("worker-cancel", "observing");
    let cancel_probe = DocumentWorkerProbe::default();
    let outcome = on_document_worker_observed(cancel, cancel_probe.clone(), move || {
        entered_tx.send(()).unwrap();
        let _ = release_rx.recv();
        Ok(())
    });
    let cancel_elapsed = started.elapsed();
    let cancel_snapshot = cancel_probe.snapshot();
    let cancelling_result = cancelling.join();
    let busy = on_document_worker(CancellationToken::new(), || Ok(()));
    release_tx.send(()).unwrap();
    let recovery_deadline = Instant::now() + Duration::from_secs(5);
    let released =
        cancel_probe.wait_released(recovery_deadline.saturating_duration_since(Instant::now()));
    wait_admission_recovery_until(recovery_deadline);
    assert!(outcome.is_err());
    assert!(cancel_elapsed < Duration::from_millis(1000));
    cancelling_result.unwrap();
    assert!(busy.is_err());
    assert_eq!(cancel_snapshot.caller_exit, WorkerCallerExit::Cancelled);
    assert_eq!(cancel_snapshot.worker_phase, WorkerPhase::WorkRunning);
    assert!(cancel_snapshot.json()["partial"].as_bool().unwrap());
    assert!(released);
    assert_eq!(
        cancel_probe.snapshot().caller_exit,
        WorkerCallerExit::Cancelled
    );
    trace.step("worker-cancel", "admission-recovered");

    trace.step("fixture", "cleanup");
    directory.close().unwrap();
    let report = serde_json::json!({"schema":1,"test":"windows-owned-uia-document-provider","passed":true,
        "synthetic_provider_only":true,"real_browser_compatibility_proven":false,
        "successful_null_navigation_observed":true,
        "root_document_value_exact_url":true,"document_name_decoy_ignored":true,
        "false_root_subtree_rejected":true,"iframe_values_not_read":true,
        "noncontrol_document_edit_hyperlink_boundaries_retained":true,
        "omnibox_values_not_read":true,"hyperlink_values_not_read":true,
        "multiple_and_hidden_roots_rejected":true,"password_focus_rejected":true,
        "navigation_before_capture_rejected":true,"navigation_after_capture_discards_result":true,
        "document_identity_change_rejected":true,"provider_timeout_bounded":true,
        "virtual_foreign_pid_cases":ownership_cases,"virtual_foreign_pid_is_not_real_external_process":true,
        "foreign_unknown_owner_branches_refused_before_document_value":true,
        "virtual_foreign_pid_boundaries_retained_without_descendant_navigation":true,
        "wide_tree_limit_refused_before_document_value":true,
        "synthetic_provider_real_owned_native_anchor_cases":native_anchor_cases,
        "provider_timeout_ms":provider_timeout_ms,"outer_timeout_ms":outer_timeout_ms,
        "isolated_worker_timeout_and_cancellation":true,"singleflight_held_until_actual_worker_return":true,
        "owned_children_stopped":true,"temporary_fixture_removed":true,
        "no_pixels_read":true,"no_audio_read":true,"no_user_archives_opened":true,
        "no_package_or_permission_changes":true,"no_network_requests":true});
    std::fs::create_dir_all(artifacts).unwrap();
    std::fs::write(evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    trace.step("fixture", "passed");
    println!("{report}");
}

#[test]
fn framework_compiler_source_path_is_owned_absolute_and_not_verbatim() {
    let directory = tempfile::tempdir().unwrap();
    let clean = framework_path(directory.path());
    assert!(clean.is_absolute());
    assert!(!clean.to_str().unwrap().starts_with(r"\\?\"));
    assert!(!clean.to_str().unwrap().contains('/'));
    let source = clean.join("fixture.cs");
    std::fs::write(&source, b"owned compile fixture").unwrap();
    assert_eq!(
        std::fs::read(directory.path().join("fixture.cs")).unwrap(),
        b"owned compile fixture"
    );
}
