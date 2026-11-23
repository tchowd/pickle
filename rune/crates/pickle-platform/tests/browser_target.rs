//! Explicit hosted-runner-only metadata fixture. No real browser, page, user
//! content, permissions, screenshots, audio, or application bundle is accessed.
#![cfg(target_os = "windows")]

use std::{
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn owned_browser_target_binding() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        eprintln!(
            "Owned native browser fixture not run: requires a hosted Windows Actions desktop."
        );
        return;
    }
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    let artifacts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts");
    let evidence = artifacts.join("windows-browser-target-native-smoke.json");
    match std::fs::remove_file(&evidence) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => panic!("Could not clear previous owned browser-target evidence"),
    }
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("chrome.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let ready = directory.path().join("ready.json");
    let mut child = OwnedChild(
        Command::new(&executable)
            .args(["--exact", "owned_browser_source_child", "--nocapture"])
            .env("PICKLE_BROWSER_TARGET_FIXTURE", &ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let metadata: serde_json::Value = loop {
        assert!(
            Instant::now() < deadline,
            "Owned browser metadata fixture was not ready"
        );
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Owned browser metadata fixture exited early"
        );
        if let Ok(bytes) = std::fs::read(&ready) {
            assert!(bytes.len() < 1_024);
            if let Ok(value) = serde_json::from_slice(&bytes) {
                break value;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(metadata["pid"].as_u64(), Some(u64::from(child.0.id())));
    let window = metadata["hwnd"].as_u64().unwrap();
    let hwnd = HWND(window as *mut _);
    loop {
        assert!(
            Instant::now() < deadline,
            "Owned browser window did not become foreground"
        );
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        assert_eq!(pid, child.0.id());
        if unsafe { GetForegroundWindow() } == hwnd {
            break;
        }
        // Only this fixture's exact PID/HWND is eligible for foreground focus.
        let _ = unsafe { SetForegroundWindow(hwnd) };
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(pickle_platform::capture_browser_target("windows-browser:edge").is_err());
    let source = pickle_platform::capture_browser_target("windows-browser:chrome").unwrap();
    assert_eq!(source.source_pid, child.0.id());
    assert_eq!(source.source_window, Some(window));
    assert_eq!(
        std::fs::canonicalize(source.app_id.strip_prefix("windows:").unwrap()).unwrap(),
        std::fs::canonicalize(&executable).unwrap()
    );
    assert!(source.text.is_empty() && source.source_url.is_none() && source.bounds.is_none());
    unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Owned metadata fixture did not close"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let window_exists = unsafe { IsWindow(Some(hwnd)) }.as_bool();
    let mut remaining_pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut remaining_pid)) };
    assert!(
        !window_exists || remaining_pid != child.0.id(),
        "Closed fixture HWND still belongs to the exact owned child"
    );
    drop(child);
    directory.close().unwrap();
    let report = serde_json::json!({
        "schema":1,"test":"windows-owned-browser-target","passed":true,
        "verified_pid_ownership":true,"verified_hwnd_ownership":true,"canonical_path_owned":true,
        "chrome_alias_accepted":true,"edge_mismatch_rejected":true,"fixture_closed":true,
        "fixture_window_no_longer_owned":true,"temporary_fixture_removed":true,
        "no_source_content_read":true,"no_url_read":true,"no_pixels_read":true,"no_audio_read":true
    });
    std::fs::create_dir_all(artifacts).unwrap();
    std::fs::write(evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("{report}");
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        LRESULT(0)
    } else {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
}

#[test]
fn owned_browser_source_child() {
    let Some(ready) = std::env::var_os("PICKLE_BROWSER_TARGET_FIXTURE") else {
        return;
    };
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    let instance = unsafe { GetModuleHandleW(None) }.unwrap();
    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: instance.into(),
        lpszClassName: w!("PickleOwnedBrowserMetadataFixture"),
        ..Default::default()
    };
    assert_ne!(unsafe { RegisterClassW(&class) }, 0);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!("Pickle owned browser metadata fixture"),
            WS_OVERLAPPEDWINDOW,
            120,
            120,
            400,
            180,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
    .unwrap();
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    let _ = unsafe { SetForegroundWindow(hwnd) };
    let metadata = serde_json::json!({"pid":std::process::id(),"hwnd":hwnd.0 as u64});
    let ready = std::path::PathBuf::from(ready);
    let staged = ready.with_extension("staged");
    std::fs::write(&staged, serde_json::to_vec(&metadata).unwrap()).unwrap();
    std::fs::rename(staged, ready).unwrap();
    let watchdog_window = hwnd.0 as usize;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        let _ = unsafe {
            PostMessageW(
                Some(HWND(watchdog_window as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        };
    });
    let mut message = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        assert!(status >= 0);
        if status == 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
