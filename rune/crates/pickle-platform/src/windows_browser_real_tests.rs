//! Hosted-only installed-browser component acceptance. No extension, user
//! profile, package permission, screenshot, OCR, login or model is involved.
use super::*;
use crate::windows_child_job;
use ::windows::{
    Win32::{
        Foundation::{CloseHandle, LPARAM},
        Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
        },
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    },
    core::{PCWSTR, PWSTR},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    io::{Read, Write},
    net::TcpListener,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread::JoinHandle,
};

// Reserved test origin: a normal domain/default-port URI, served exclusively
// by the owned loopback proxy. No DNS/hosts entry or external host is needed.
const ORIGIN: &str = "http://pickle-native.test";

struct Log {
    path: PathBuf,
    stage: &'static str,
    browser: &'static str,
    passed: bool,
    runner_image: serde_json::Value,
    browser_versions: serde_json::Map<String, serde_json::Value>,
}
impl Log {
    fn new(artifacts: &Path) -> Self {
        std::fs::create_dir_all(artifacts).unwrap();
        let path = artifacts.join("windows-browser-real-native-smoke.log");
        std::fs::write(&path, []).unwrap();
        let log = Self {
            path,
            stage: "started",
            browser: "fixture",
            passed: false,
            runner_image: serde_json::json!({"ImageOS":public_image("ImageOS"),"ImageVersion":public_image("ImageVersion")}),
            browser_versions: serde_json::Map::new(),
        };
        log.write(serde_json::json!({"stage":"runner-image","runner_image":log.runner_image}));
        log
    }
    fn write(&self, value: serde_json::Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        if bytes.len() <= 4096
            && std::fs::metadata(&self.path).unwrap().len() + bytes.len() as u64 <= 16384
        {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&self.path)
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
    }
    fn step(&mut self, browser: &'static str, stage: &'static str) {
        self.browser = browser;
        self.stage = stage;
        self.write(serde_json::json!({"browser":browser,"stage":stage}));
    }
}
impl Drop for Log {
    fn drop(&mut self) {
        if std::thread::panicking() && !self.passed {
            // No partial run can masquerade as native compatibility evidence.
            let report = serde_json::json!({"schema":1,"test":"windows-installed-browser-document","passed":false,"browser":self.browser,"stage":self.stage,"error":"assertion-or-unavailable","runner_image":self.runner_image,"browser_versions":self.browser_versions,"real_browser_component_only":true,"extension_bridge_proven":false,"screen_capture_proven":false});
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::write(
                    parent.join("windows-browser-real-native-smoke.json"),
                    serde_json::to_vec_pretty(&report).unwrap(),
                );
            }
        }
    }
}

fn public_image(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| {
        value.len() <= 128
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || " ._-".contains(character))
    })
}

#[test]
fn failed_browser_report_retains_discovered_version_and_runner_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let failure = std::panic::catch_unwind(|| {
        let mut log = Log::new(directory.path());
        log.browser_versions
            .insert("chrome".into(), serde_json::json!("138.0.1.2"));
        log.step("chrome", "document-attempt");
        panic!("Fixed synthetic failure for report retention");
    });
    assert!(failure.is_err());
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            directory
                .path()
                .join("windows-browser-real-native-smoke.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["passed"], false);
    assert_eq!(report["browser_versions"]["chrome"], "138.0.1.2");
    assert_eq!(report["stage"], "document-attempt");
    assert_eq!(
        report["runner_image"],
        serde_json::json!({"ImageOS":public_image("ImageOS"),"ImageVersion":public_image("ImageVersion")})
    );
}

struct PageServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    pages: Arc<AtomicUsize>,
    rejected: Arc<AtomicUsize>,
    connections: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}
impl PageServer {
    fn start() -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let pages = Arc::new(AtomicUsize::new(0));
        let rejected = Arc::new(AtomicUsize::new(0));
        let connections = Arc::new(AtomicUsize::new(0));
        let accepted = connections.clone();
        let halted = stop.clone();
        let seen = pages.clone();
        let blocked = rejected.clone();
        let worker = std::thread::spawn(move || {
            while !halted.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        accepted.fetch_add(1, Ordering::AcqRel);
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                        let mut bytes = Vec::new();
                        let mut buffer = [0u8; 1024];
                        let header_deadline = Instant::now() + Duration::from_secs(1);
                        while bytes.len() < 8192
                            && !bytes.windows(4).any(|part| part == b"\r\n\r\n")
                        {
                            if halted.load(Ordering::Acquire) || Instant::now() >= header_deadline {
                                break;
                            }
                            let remaining =
                                header_deadline.saturating_duration_since(Instant::now());
                            let _ = stream.set_read_timeout(Some(
                                remaining
                                    .min(Duration::from_millis(100))
                                    .max(Duration::from_millis(1)),
                            ));
                            let room = buffer.len().min(8192 - bytes.len());
                            match stream.read(&mut buffer[..room]) {
                                Ok(0) | Err(_) => break,
                                Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                            }
                        }
                        if halted.load(Ordering::Acquire) {
                            break;
                        }
                        let request = std::str::from_utf8(&bytes).unwrap_or("");
                        let line = request.lines().next().unwrap_or("");
                        let mut parts = line.split_whitespace();
                        let method = parts.next().unwrap_or("");
                        let target = parts.next().unwrap_or("");
                        let mut hosts = request
                            .lines()
                            .filter_map(|line| line.split_once(':'))
                            .filter(|(name, _)| name.eq_ignore_ascii_case("Host"))
                            .map(|(_, value)| value.trim());
                        let owned_host = hosts.next().is_some_and(|host| {
                            host.eq_ignore_ascii_case("pickle-native.test")
                                || host.eq_ignore_ascii_case("pickle-native.test:80")
                        }) && hosts.next().is_none();
                        let path = target.strip_prefix(ORIGIN).unwrap_or(target);
                        let owned = owned_host
                            && (target.starts_with('/')
                                || target.starts_with(&format!("{ORIGIN}/")))
                            && method == "GET";
                        let page = owned
                            && matches!(
                                path,
                                "/owned-page?browser=chrome"
                                    | "/owned-page?browser=edge"
                                    | "/owned-page?browser=fixture"
                            );
                        let body = if page {
                            seen.fetch_add(1, Ordering::AcqRel);
                            "<!doctype html><meta charset=utf-8><title>https://example.invalid/not-the-document#title</title><h1>Owned native document fixture</h1><p>Fixed synthetic page.</p><input value='https://example.invalid/not-the-document#input'><a href='https://example.invalid/not-the-document#link'>Decoy address</a>"
                        } else {
                            blocked.fetch_add(1, Ordering::AcqRel);
                            "Owned proxy does not forward requests."
                        };
                        let status = if page { "200 OK" } else { "403 Forbidden" };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'none'\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stop,
            pages,
            rejected,
            connections,
            worker: Some(worker),
        }
    }
    fn url(&self, browser: &str) -> String {
        format!("{ORIGIN}/owned-page?browser={browser}#route-one")
    }
    fn finish(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

#[test]
fn owned_proxy_serves_only_reserved_origin_and_exact_fixture_page() {
    let mut server = PageServer::start();
    for (request, expected) in [
        (
            "GET http://pickle-native.test/owned-page?browser=fixture HTTP/1.1\r\nHost: pickle-native.test\r\n\r\n",
            "200 OK",
        ),
        (
            "GET http://foreign.test/owned-page?browser=fixture HTTP/1.1\r\nHost: pickle-native.test\r\n\r\n",
            "403 Forbidden",
        ),
        (
            "GET http://pickle-native.test/owned-page?browser=fixture HTTP/1.1\r\nHost: foreign.test\r\n\r\n",
            "403 Forbidden",
        ),
        (
            "GET http://pickle-native.test/other-page HTTP/1.1\r\nHost: pickle-native.test\r\n\r\n",
            "403 Forbidden",
        ),
    ] {
        let mut stream = std::net::TcpStream::connect(server.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = vec![];
        stream.take(2048).read_to_end(&mut response).unwrap();
        assert!(response.starts_with(format!("HTTP/1.1 {expected}\r\n").as_bytes()));
    }
    assert_eq!(server.pages.load(Ordering::Acquire), 1);
    assert_eq!(server.rejected.load(Ordering::Acquire), 3);
    server.finish();
}

#[test]
fn owned_proxy_refuses_foreign_targets_and_stops_during_partial_headers() {
    let mut server = PageServer::start();
    let mut stream = std::net::TcpStream::connect(server.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stream.write_all(b"CONNECT external.example.invalid:443 HTTP/1.1\r\nHost: external.example.invalid\r\n\r\n").unwrap();
    let mut response = vec![];
    stream.take(2048).read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 403 Forbidden\r\n"));
    assert_eq!(server.pages.load(Ordering::Acquire), 0);
    assert_eq!(server.rejected.load(Ordering::Acquire), 1);
    let mut partial = std::net::TcpStream::connect(server.address).unwrap();
    partial
        .set_write_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    partial
        .write_all(
            b"GET /owned-page?browser=fixture HTTP/1.1\r\nHost: pickle-native.test\r\nX-Partial: ",
        )
        .unwrap();
    let began = Instant::now();
    while server.connections.load(Ordering::Acquire) < 2 {
        assert!(began.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(10));
    }
    let writer = std::thread::spawn(move || {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(3) {
            if partial.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let began = Instant::now();
    server.finish();
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "Server shutdown must not wait for an endless sequence of partial reads"
    );
    writer.join().unwrap();
}
impl Drop for PageServer {
    fn drop(&mut self) {
        self.finish();
    }
}

fn installed(browser: &str) -> Option<PathBuf> {
    let tail = if browser == "chrome" {
        "Google/Chrome/Application/chrome.exe"
    } else {
        "Microsoft/Edge/Application/msedge.exe"
    };
    ["ProgramW6432", "PROGRAMFILES", "PROGRAMFILES(X86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join(tail))
        .find(|path| path.is_file())
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn version(path: &Path) -> String {
    let path = wide(path.as_os_str());
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    assert!(
        (1..=1024 * 1024).contains(&size),
        "Installed browser version resource unavailable"
    );
    let mut bytes = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, bytes.as_mut_ptr().cast()) }
        .unwrap();
    let mut pointer = std::ptr::null_mut();
    let mut length = 0u32;
    assert!(
        unsafe {
            VerQueryValueW(
                bytes.as_ptr().cast(),
                ::windows::core::w!("\\"),
                &mut pointer,
                &mut length,
            )
        }
        .as_bool()
    );
    let offset = (pointer as usize)
        .checked_sub(bytes.as_ptr() as usize)
        .unwrap();
    assert!(
        length as usize >= std::mem::size_of::<VS_FIXEDFILEINFO>()
            && offset
                .checked_add(length as usize)
                .is_some_and(|end| end <= bytes.len())
    );
    let value = unsafe { std::ptr::read_unaligned(pointer.cast::<VS_FIXEDFILEINFO>()) };
    assert_eq!(value.dwSignature, 0xfeef04bd);
    format!(
        "{}.{}.{}.{}",
        value.dwFileVersionMS >> 16,
        value.dwFileVersionMS & 0xffff,
        value.dwFileVersionLS >> 16,
        value.dwFileVersionLS & 0xffff
    )
}

struct Browser {
    child: Option<windows_child_job::Child>,
    drain: Option<JoinHandle<()>>,
    drain_done: Option<Receiver<()>>,
    stopped: bool,
}
impl Browser {
    fn launch(executable: &Path, profile: &Path, url: &str, server: &PageServer) -> Self {
        let mut command = Command::new(executable);
        let mut profile_arg = OsString::from("--user-data-dir=");
        profile_arg.push(profile);
        // Unique profile is the documented Chromium isolation mechanism.
        // Background flags reduce ambient services; the owned HTTP(S) proxy
        // rejects nonfixture traffic without forwarding. No security, UIA,
        // renderer-accessibility, extension-debugging or CDP flags are enabled.
        command
            .arg(profile_arg)
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
                "--disable-extensions",
            ])
            .arg(format!("--proxy-server=http://{}", server.address))
            .arg("--proxy-bypass-list=<-loopback>")
            .arg("--new-window")
            .arg(url);
        let mut child = windows_child_job::spawn(&command).unwrap();
        drop(child.stdin.take());
        let mut output = child.stdout.take().unwrap();
        let (done, drain_done) = mpsc::sync_channel(1);
        let drain = std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(count) = output.read(&mut buffer) {
                if count == 0 {
                    break;
                }
            }
            drop(output);
            let _ = done.send(());
        });
        Self {
            child: Some(child),
            drain: Some(drain),
            drain_done: Some(drain_done),
            stopped: false,
        }
    }
    fn child(&self) -> &windows_child_job::Child {
        self.child.as_ref().unwrap()
    }
    fn finish(&mut self) {
        let mut child = self.child.take().unwrap();
        let stopped = child.stop_and_wait(Duration::from_secs(10));
        // Close kill-on-close ownership before any pipe join, including an
        // error path. A failed status query must not leave a writer alive.
        drop(child);
        let drained = self
            .drain_done
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .is_ok();
        if let Some(drain) = self.drain.take()
            && drained
        {
            drain.join().unwrap();
        }
        self.stopped = true;
        stopped.unwrap();
        assert!(
            drained,
            "Contained browser stdout did not reach EOF after job shutdown"
        );
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        if !self.stopped {
            let mut stopped = false;
            if let Some(mut child) = self.child.take() {
                stopped = child.stop_and_wait(Duration::from_secs(10)).is_ok();
                drop(child);
            }
            let drained = self.drain_done.take().is_some_and(|done| {
                if stopped {
                    done.recv_timeout(Duration::from_secs(2)).is_ok()
                } else {
                    done.try_recv().is_ok()
                }
            });
            if let Some(drain) = self.drain.take()
                && drained
            {
                let _ = drain.join();
            }
        }
    }
}

fn executable_path(pid: u32) -> Option<PathBuf> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut bytes = vec![0u16; 32768];
    let mut length = bytes.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(bytes.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    String::from_utf16(&bytes[..length as usize])
        .ok()
        .map(PathBuf::from)
}
struct Windows {
    pids: BTreeSet<u32>,
    found: Vec<(u64, u32)>,
}
unsafe extern "system" fn enumerate(hwnd: HWND, argument: LPARAM) -> ::windows::core::BOOL {
    let data = unsafe { &mut *(argument.0 as *mut Windows) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if data.pids.contains(&pid)
        && unsafe { IsWindowVisible(hwnd) }.as_bool()
        && unsafe { GetAncestor(hwnd, GA_ROOT) } == hwnd
    {
        data.found.push((hwnd.0 as u64, pid));
    }
    true.into()
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
struct WindowCounts {
    job_members: usize,
    visible_owned: usize,
    exact_executable: usize,
}

fn source(
    browser: &Browser,
    executable: &Path,
    counts: &mut WindowCounts,
) -> Option<CapturedSelection> {
    let pids = browser.child().job_pids().ok()?;
    counts.job_members = pids.len();
    if pids.is_empty() {
        return None;
    }
    let mut data = Windows {
        pids: pids.into_iter().collect(),
        found: vec![],
    };
    unsafe {
        EnumWindows(
            Some(enumerate),
            LPARAM((&mut data as *mut Windows) as isize),
        )
    }
    .ok()?;
    counts.visible_owned = data.found.len();
    let expected = std::fs::canonicalize(executable).ok()?;
    let mut eligible = vec![];
    for (window, pid) in data.found {
        if !browser.child().contains_pid(pid).ok()? {
            continue;
        }
        let path = executable_path(pid)?;
        if std::fs::canonicalize(&path).ok()? != expected {
            continue;
        }
        counts.exact_executable += 1;
        eligible.push(CapturedSelection {
            text: String::new(),
            app_name: "Owned installed browser".into(),
            app_id: format!("windows:{}", path.display()),
            bounds: None,
            source_url: None,
            source_pid: pid,
            source_window: Some(window),
        });
    }
    if eligible.len() == 1 {
        eligible.pop()
    } else {
        None
    }
}

fn profile_snapshot(profile: &Path) -> serde_json::Value {
    fn kind(path: &Path, directory: bool) -> Option<bool> {
        match std::fs::metadata(path) {
            Ok(metadata) => Some(if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(false),
            Err(_) => None,
        }
    }
    // Only fixed owned-profile entry metadata, never profile content/names.
    serde_json::json!({
        "root_directory":kind(profile,true),
        "default_directory":kind(&profile.join("Default"),true),
        "local_state_file":kind(&profile.join("Local State"),false),
        "first_run_file":kind(&profile.join("First Run"),false),
    })
}

fn executable_class(expected: Option<&Path>, observed: Option<&Path>) -> &'static str {
    match (expected, observed) {
        (Some(expected), Some(observed)) if expected == observed => "original-executable",
        (Some(_), Some(_)) => "other-executable",
        _ => "unavailable",
    }
}

fn startup_failure_snapshot(
    browser: &Browser,
    executable: &Path,
    profile: &Path,
    before: &serde_json::Value,
) -> serde_json::Value {
    let child = browser.child();
    // This accessor uses the original retained process handle and never calls
    // try_wait, reaps the launcher, or closes the kill-on-close job.
    let original = child.status_snapshot();
    let (status_available, running, exit_code) = match &original {
        Ok(None) => (true, Some(true), None),
        Ok(Some(status)) => (true, Some(false), status.code()),
        Err(_) => (false, None, None),
    };
    let original_id = child.id().ok();
    let expected = std::fs::canonicalize(executable).ok();
    let members = child.job_pids();
    let mut records = vec![];
    if let Ok(pids) = &members {
        for pid in pids.iter().take(16) {
            // Membership and path query share one retained process handle.
            // Do not log path, command line, environment, stderr, or memory.
            let path = child
                .member_executable(*pid)
                .ok()
                .and_then(|path| std::fs::canonicalize(path).ok());
            records.push(serde_json::json!({"original_process":Some(*pid)==original_id,"executable_class":executable_class(expected.as_deref(),path.as_deref())}));
        }
    }
    serde_json::json!({
        "stage":"startup-failure-diagnostics",
        "original_process_status_available":status_available,
        "original_process_running":running,
        "original_process_exit_code":exit_code,
        "original_process_status_os_error":original.as_ref().err().and_then(std::io::Error::raw_os_error),
        "job_members_available":members.is_ok(),
        "job_members_total":members.as_ref().ok().map(Vec::len),
        "job_members_retained":records.len(),
        "job_members_omitted":members.as_ref().ok().map(|pids|pids.len().saturating_sub(records.len())),
        "job_members":records,
        "profile_before":before,
        "profile_after":profile_snapshot(profile),
    })
}

#[test]
fn startup_diagnostics_classify_only_fixed_paths_and_profile_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let before = profile_snapshot(directory.path());
    assert_eq!(
        before,
        serde_json::json!({"root_directory":true,"default_directory":false,"local_state_file":false,"first_run_file":false})
    );
    std::fs::create_dir(directory.path().join("Default")).unwrap();
    std::fs::write(
        directory.path().join("Local State"),
        "PRIVATE_PROFILE_CONTENT",
    )
    .unwrap();
    let after = profile_snapshot(directory.path());
    assert_eq!(after["default_directory"], true);
    assert_eq!(after["local_state_file"], true);
    assert!(!serde_json::to_string(&after).unwrap().contains("PRIVATE_"));
    assert_eq!(
        executable_class(Some(Path::new("same")), Some(Path::new("same"))),
        "original-executable"
    );
    assert_eq!(
        executable_class(Some(Path::new("same")), Some(Path::new("other"))),
        "other-executable"
    );
    assert_eq!(
        executable_class(Some(Path::new("same")), None),
        "unavailable"
    );
    assert_eq!(
        executable_class(None, Some(Path::new("same"))),
        "unavailable"
    );
}

fn resolved_source(browser: &'static str, owned: &CapturedSelection) -> CapturedSelection {
    let expected = if browser == "chrome" {
        "windows-browser:chrome"
    } else {
        "windows-browser:edge"
    };
    // Same metadata-only resolver and shared flight gate as app startup. The
    // caller's 400ms wait is bounded even if an OS metadata query stalls.
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(crate::capture_browser_target(expected));
    });
    let target = receiver
        .recv_timeout(Duration::from_millis(400))
        .expect("Production browser target resolver exceeded its caller budget")
        .expect("Production browser target resolver rejected the owned foreground window");
    assert_eq!(target.source_pid, owned.source_pid);
    assert_eq!(target.source_window, owned.source_window);
    assert_eq!(target.app_id, owned.app_id);
    assert!(target.text.is_empty());
    assert!(target.source_url.is_none());
    assert!(target.bounds.is_none());
    target
}

struct Attempt {
    accepted: bool,
    captures: usize,
    elapsed_ms: u128,
    observations: Vec<serde_json::Value>,
}
fn attempt(source: CapturedSelection, expected: String) -> Result<Attempt, PlatformError> {
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let started = Instant::now();
    on_document_worker(cancel, move || {
        let reader = DocumentReader::new(&source, token.clone())?;
        let captures = Cell::new(0usize);
        let observations = RefCell::new(vec![]);
        let result = verified_capture(
            &source,
            &expected,
            &token,
            || {
                let value = reader.observe();
                let diagnostic = reader.failure_diagnostics();
                let mut record = serde_json::json!({"stage":reader.failure_stage(),"accepted":value.is_ok(),"url_matches_expected":value.as_ref().is_ok_and(|value|document_urls_match(&expected,&value.url))});
                for key in [
                    "hresult",
                    "elapsed_ms",
                    "remaining_ms",
                    "nodes",
                    "documents",
                    "control_type",
                    "custom_variant_type",
                    "custom_variant_bool",
                    "nullable_navigation_ends",
                    "node_hwnd_owner_pid",
                    "pruned_foreign_native_branches",
                    "anchored_foreign_native_branches",
                    "virtual_nodes_using_path_anchor",
                    "candidate_native_anchors",
                    "anchor_revalidations",
                    "document_pid",
                    "filtered_discovery",
                    "raw_ancestry_checks",
                    "raw_ancestry_nodes",
                    "source_pid",
                    "root_pid",
                    "node_pid",
                    "queue_len",
                    "node_depth",
                    "node_native_hwnd_present",
                    "node_hwnd_root_matches_source",
                    "node_executable_matches_source",
                    "post_rejection_metadata",
                ] {
                    record[key] = diagnostic[key].clone();
                }
                observations.borrow_mut().push(record);
                value
            },
            || {
                captures.set(captures.get() + 1);
                Ok(())
            },
        );
        Ok(Attempt {
            accepted: result.is_ok(),
            captures: captures.get(),
            elapsed_ms: started.elapsed().as_millis(),
            observations: observations.into_inner(),
        })
    })
}

#[test]
fn installed_chrome_and_edge_document_urls() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        eprintln!("Installed-browser component check not run: requires hosted Windows desktop.");
        return;
    }
    let _fixture_lock = native_browser_fixture_lock();
    let artifacts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts");
    let mut log = Log::new(&artifacts);
    let evidence = artifacts.join("windows-browser-real-native-smoke.json");
    if evidence.is_file() {
        std::fs::remove_file(&evidence).unwrap();
    }
    let directory = tempfile::tempdir().unwrap();
    let mut server = PageServer::start();
    let mut reports = vec![];
    let mut browsers = vec![];
    for name in ["chrome", "edge"] {
        log.step(name, "discovering");
        let Some(executable) = installed(name) else {
            log.step(name, "browser-unavailable");
            panic!("Required installed browser unavailable; no compatibility pass");
        };
        log.step(name, "reading-file-version");
        let version = version(&executable);
        log.browser_versions
            .insert(name.into(), serde_json::json!(version));
        log.write(serde_json::json!({"browser":name,"stage":"installed-file-version","file_version":version}));
        browsers.push((name, executable, version));
    }
    for (name, executable, version) in browsers {
        let profile = directory.path().join(format!("owned-{name}-profile"));
        std::fs::create_dir(&profile).unwrap();
        let initial_profile = profile_snapshot(&profile);
        let expected = server.url(name);
        let pages_before = server.pages.load(Ordering::Acquire);
        log.step(name, "launching");
        let mut browser = Browser::launch(&executable, &profile, &expected, &server);
        let root_pid = browser.child().id().unwrap();
        assert!(root_pid > 0);
        log.step(name, "waiting-owned-window-and-page");
        let startup = Instant::now();
        let mut selected = None;
        let mut polls = 0;
        let mut last_counts = WindowCounts::default();
        let mut last_signature = None;
        let mut readiness_records = 0;
        while startup.elapsed() < Duration::from_secs(25) {
            polls += 1;
            // A launcher may exit while its browser descendants remain in the
            // exact held job. Do not call Child::try_wait (which closes it).
            let mut counts = WindowCounts::default();
            let candidate = source(&browser, &executable, &mut counts);
            let page_seen = server.pages.load(Ordering::Acquire) > pages_before;
            let profile_seen = profile.join("Default").is_dir();
            let signature = (counts, page_seen, profile_seen);
            if last_signature != Some(signature) && readiness_records < 8 {
                log.write(serde_json::json!({"browser":name,"stage":"startup-readiness","elapsed_ms":startup.elapsed().as_millis(),"poll":polls,"owned_job_members":counts.job_members,"visible_owned_windows":counts.visible_owned,"exact_executable_windows":counts.exact_executable,"owned_page_seen":page_seen,"owned_profile_directory_observed":profile_seen}));
                readiness_records += 1;
            }
            last_signature = Some(signature);
            last_counts = counts;
            if let Some(source) = candidate
                && page_seen
                && profile_seen
            {
                selected = Some(source);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        log.write(serde_json::json!({"browser":name,"stage":"startup-result","elapsed_ms":startup.elapsed().as_millis(),"polls":polls,"owned_window_found":selected.is_some(),"owned_profile_directory_observed":profile.join("Default").is_dir(),"page_requests":server.pages.load(Ordering::Acquire)-pages_before,"owned_job_members":last_counts.job_members,"visible_owned_windows":last_counts.visible_owned,"exact_executable_windows":last_counts.exact_executable}));
        if selected.is_none() {
            log.write(startup_failure_snapshot(
                &browser,
                &executable,
                &profile,
                &initial_profile,
            ));
        }
        let source = selected.expect(
            "Owned installed browser did not expose one owned visible window and load its page",
        );
        let startup_ms = startup.elapsed().as_millis();
        let hwnd = HWND(source.source_window.unwrap() as *mut _);
        let mut attempts = vec![];
        let mut accepted = false;
        let mut native_source = None;
        for number in 0..3 {
            log.step(name, "document-attempt");
            assert!(browser.child().contains_pid(source.source_pid).unwrap());
            let _ = unsafe { SetForegroundWindow(hwnd) };
            let focus = Instant::now();
            while unsafe { GetForegroundWindow() } != hwnd
                && focus.elapsed() < Duration::from_secs(2)
            {
                let _ = unsafe { SetForegroundWindow(hwnd) };
                std::thread::sleep(Duration::from_millis(25));
            }
            assert_eq!(
                unsafe { GetForegroundWindow() },
                hwnd,
                "Exact owned browser window must be foreground"
            );
            log.step(name, "binding-production-target");
            let target = resolved_source(name, &source);
            assert!(browser.child().contains_pid(target.source_pid).unwrap());
            native_source = Some(target.clone());
            let began = Instant::now();
            let outcome = attempt(target, expected.clone());
            let value = match outcome {
                Ok(value) => {
                    let record = serde_json::json!({"attempt":number+1,"first_document_attempt":number==0,"accepted":value.accepted,"captures":value.captures,"elapsed_ms":value.elapsed_ms,"observations":value.observations});
                    accepted = value.accepted;
                    assert!(value.captures <= 1);
                    if accepted {
                        assert_eq!(value.captures, 1);
                        assert_eq!(value.observations.len(), 3);
                        assert!(value.observations.iter().all(|observation| {
                            observation["accepted"] == true
                                && observation["url_matches_expected"] == true
                                && observation["documents"] == 1
                                && observation["filtered_discovery"] == true
                                && observation["raw_ancestry_checks"]
                                    .as_u64()
                                    .is_some_and(|count| count >= 2)
                        }));
                    }
                    if value.observations.first().is_some_and(|observation| {
                        observation["accepted"] == true
                            && observation["url_matches_expected"] == false
                    }) {
                        assert_eq!(value.captures, 0);
                    }
                    record
                }
                Err(_) => {
                    serde_json::json!({"attempt":number+1,"first_document_attempt":number==0,"accepted":false,"elapsed_ms":began.elapsed().as_millis(),"error":"worker-unavailable"})
                }
            };
            log.write(serde_json::json!({"browser":name,"stage":"document-result","result":value}));
            attempts.push(value);
            if accepted {
                break;
            }
            if number < 2 {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        assert!(
            accepted,
            "Installed browser document adapter unavailable after recorded bounded attempts"
        );
        log.step(name, "mismatch-check");
        let mismatch = attempt(
            native_source.unwrap(),
            expected.replace("#route-one", "#route-two"),
        )
        .unwrap();
        assert!(!mismatch.accepted);
        assert_eq!(mismatch.captures, 0);
        assert!(
            mismatch
                .observations
                .iter()
                .any(|value| value["accepted"] == true && value["url_matches_expected"] == false),
            "A mismatch must be an actual successful native URL observation, not unavailability"
        );
        log.step(name, "stopping");
        browser.finish();
        drop(browser);
        log.step(name, "stopped");
        let first_accepted = attempts
            .first()
            .is_some_and(|value| value["accepted"] == true);
        reports.push(serde_json::json!({"browser":name,"file_version":version,"owned_profile":true,"owned_profile_directory_observed":true,"atomic_job_containment":true,"pid_hwnd_path_membership_verified":true,"resolver_bound":true,"startup_ms":startup_ms,"attempts":attempts,"passed_first_document_attempt":first_accepted,"accepted_after_retry":!first_accepted,"expected_url_accepted":true,"mismatched_fragment_refused_without_capture":true,"mismatch_observations":mismatch.observations,"misleading_page_title_input_link":true,"owned_processes_stopped":true,"forced_accessibility_flags":false,"devtools_or_cdp_used":false}));
    }
    log.step("fixture", "stopping-server");
    server.finish();
    let pages = server.pages.load(Ordering::Acquire);
    let rejected = server.rejected.load(Ordering::Acquire);
    drop(server);
    log.step("fixture", "removing-profiles");
    directory.close().unwrap();
    let report = serde_json::json!({"schema":1,"test":"windows-installed-browser-document","passed":true,"runner_image":{"ImageOS":public_image("ImageOS"),"ImageVersion":public_image("ImageVersion")},"browsers":reports,"real_browser_component_only":true,"extension_bridge_proven":false,"screen_capture_proven":false,"no_pixels_read":true,"no_model_calls":true,"test_pages_served_by_owned_loopback_proxy":true,"reserved_test_origin_default_http_port":true,"browser_http_proxy_no_forwarding":true,"loopback_page_requests":pages,"rejected_proxy_requests":rejected,"all_owned_processes_stopped":true,"server_joined":true,"temporary_profiles_removed":true,"no_permissions_package_firewall_or_system_proxy_changes":true});
    std::fs::write(evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    log.step("fixture", "passed");
    log.passed = true;
}
