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
        std::fs::create_dir_all(artifacts).unwrap();
        let path = artifacts.join("windows-browser-page-native-smoke.log");
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
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if on_document_worker(CancellationToken::new(), || Ok(())).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Isolated document worker did not release admission after finishing"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
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
    ] {
        let trace_mode = if phase == "stable" { mode } else { phase };
        trace.step(trace_mode, "launching");
        let fixture = Fixture::launch(&executable, directory.path(), mode, case);
        trace.step(trace_mode, "ready");
        case += 1;
        trace.step(trace_mode, "observing");
        let result = verify(fixture.source.clone(), phase);
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
        trace.step(trace_mode, "closing");
        fixture.close();
        trace.step(trace_mode, "closed");
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
    let started = Instant::now();
    trace.step("worker-timeout", "observing");
    assert!(
        on_document_worker(CancellationToken::new(), move || {
            let _ = release_rx.recv();
            Ok(())
        })
        .is_err()
    );
    let outer_timeout_ms = started.elapsed().as_millis();
    assert!((1800..3500).contains(&outer_timeout_ms));
    assert!(
        on_document_worker(CancellationToken::new(), || Ok(())).is_err(),
        "Admission must remain held by the unfinished worker"
    );
    release_tx.send(()).unwrap();
    wait_admission_recovery();
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
    assert!(
        on_document_worker(cancel, move || {
            entered_tx.send(()).unwrap();
            let _ = release_rx.recv();
            Ok(())
        })
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_millis(1000));
    cancelling.join().unwrap();
    assert!(on_document_worker(CancellationToken::new(), || Ok(())).is_err());
    release_tx.send(()).unwrap();
    wait_admission_recovery();
    trace.step("worker-cancel", "admission-recovered");

    trace.step("fixture", "cleanup");
    directory.close().unwrap();
    let report = serde_json::json!({"schema":1,"test":"windows-owned-uia-document-provider","passed":true,
        "synthetic_provider_only":true,"real_browser_compatibility_proven":false,
        "successful_null_navigation_observed":true,
        "root_document_value_exact_url":true,"document_name_decoy_ignored":true,
        "false_root_subtree_rejected":true,"iframe_values_not_read":true,
        "omnibox_values_not_read":true,"hyperlink_values_not_read":true,
        "multiple_and_hidden_roots_rejected":true,"password_focus_rejected":true,
        "navigation_before_capture_rejected":true,"navigation_after_capture_discards_result":true,
        "document_identity_change_rejected":true,"provider_timeout_bounded":true,
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
