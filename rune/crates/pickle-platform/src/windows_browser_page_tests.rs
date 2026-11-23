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
    cell::Cell,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
};

const EXPECTED: &str = "https://example.invalid/Path%2Fpart?q=Caf%C3%A9&b=2#route-one";
const COMMAND: u32 = WM_APP + 200;

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

fn compile_provider(directory: &Path) -> PathBuf {
    let framework = PathBuf::from(std::env::var_os("WINDIR").unwrap())
        .join("Microsoft.NET/Framework64/v4.0.30319");
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
    let output = compiler
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../native/windows/BrowserDocumentFixture.cs"),
        )
        .stdin(Stdio::null())
        .output()
        .unwrap();
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

fn verify(source: CapturedSelection, phase: &'static str) -> (bool, usize, usize) {
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
                reader.observe()
            },
            || {
                captured.set(captured.get() + 1);
                if phase == "after-navigation" {
                    command(&source, 1)?;
                }
                Ok(()) // No pixels, capture permissions, OCR or package identity.
            },
        );
        Ok((outcome.is_ok(), captured.get(), reads.get()))
    })
    .unwrap()
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
    match std::fs::remove_file(&evidence) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => panic!("Could not remove previous synthetic document report"),
    }
    let directory = tempfile::tempdir().unwrap();
    let executable = compile_provider(directory.path());
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
        let fixture = Fixture::launch(&executable, directory.path(), mode, case);
        case += 1;
        let outcome = verify(fixture.source.clone(), phase);
        assert_eq!(outcome.0, success, "{mode}/{phase}");
        assert_eq!(outcome.1, captures, "{mode}/{phase}");
        let counts = fixture.command(3).unwrap();
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
            assert_eq!(outcome.2, 3);
        }
        fixture.close();
    }

    // A genuine delayed external provider must fail within the outer bound;
    // stopping that exact owned process releases any outstanding COM request.
    let mut slow = Fixture::launch(&executable, directory.path(), "slow", case);
    let source = slow.source.clone();
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let started = Instant::now();
    assert!(
        on_document_worker(token, move || DocumentReader::new(&source, worker_token)?
            .observe())
        .is_err()
    );
    let provider_timeout_ms = started.elapsed().as_millis();
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

    // Deterministic outer timeout and cancellation while the isolated MTA is
    // actually live, independent of UIA's own short RPC transaction timeouts.
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let started = Instant::now();
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

    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let cancelling = std::thread::spawn(move || {
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        signal.cancel();
    });
    let started = Instant::now();
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

    directory.close().unwrap();
    let report = serde_json::json!({"schema":1,"test":"windows-owned-uia-document-provider","passed":true,
        "synthetic_provider_only":true,"real_browser_compatibility_proven":false,
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
    println!("{report}");
}
