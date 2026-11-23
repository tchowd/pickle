//! The only subprocesses are copies of this crate's own built test executable.
use super::*;
use std::{
    io::{Read, Write},
    process::Stdio,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        TerminateProcess,
    },
};

const FIXTURE: &[u8] = b"fixed-owned-pipe-fixture";

#[test]
fn quoting_preserves_empty_quotes_backslashes_unicode_and_raw_windows_units() {
    let mut command = Command::new(r"C:\owned fixture\probe.exe");
    command.args(["", "a b", "a\"b", "trailing\\", "例子🥒"]);
    let quoted = String::from_utf16(
        &command_line(&command).unwrap()[..command_line(&command).unwrap().len() - 1],
    )
    .unwrap();
    assert_eq!(
        quoted,
        "\"C:\\owned fixture\\probe.exe\" \"\" \"a b\" \"a\\\"b\" \"trailing\\\\\" \"例子🥒\""
    );
    use std::os::windows::ffi::OsStringExt;
    let units = std::ffi::OsString::from_wide(&[0x61, 0xd800, 0x62]);
    command = Command::new(r"C:\owned\probe.exe");
    command.arg(units);
    let line = command_line(&command).unwrap();
    assert!(line.windows(3).any(|value| value == [0x61, 0xd800, 0x62]));
    command.arg("bad\0argument");
    assert!(command_line(&command).is_err());
    let mut command = Command::new(r"C:\owned\probe.exe");
    command.arg("x".repeat(32_767));
    assert!(command_line(&command).is_err());
}

fn directory() -> Option<std::path::PathBuf> {
    std::env::var_os("PICKLE_JOB_FIXTURE_DIRECTORY").map(Into::into)
}
fn wait_for(path: &Path) {
    let began = Instant::now();
    while !path.is_file() {
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "Owned fixture readiness timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn hold_bounded() {
    std::thread::sleep(Duration::from_secs(25));
}
fn publish_pid(path: &Path) {
    let staged = path.with_extension("partial");
    std::fs::write(&staged, std::process::id().to_string()).unwrap();
    std::fs::rename(staged, path).unwrap();
}

#[test]
fn fixture_grandchild() {
    let Some(directory) = directory() else { return };
    publish_pid(&directory.join("grandchild"));
    hold_bounded();
}

#[test]
fn fixture_child() {
    let Some(directory) = directory() else { return };
    let mut input = vec![0; FIXTURE.len()];
    std::io::stdin().read_exact(&mut input).unwrap();
    assert_eq!(input, FIXTURE, "Only a fixed owned pipe challenge is used");
    let leaves_pipe = std::env::var("PICKLE_JOB_FIXTURE_MODE").unwrap() == "finished";
    let mut descendant = Command::new(std::env::current_exe().unwrap());
    descendant
        .args([
            "--exact",
            "windows_child_job::tests::fixture_grandchild",
            "--nocapture",
        ])
        .stdin(Stdio::null())
        .stdout(if leaves_pipe {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null());
    // The OS inherits job membership, never the unnamed job's owning handle.
    let descendant = Parent(descendant.spawn().unwrap());
    wait_for(&directory.join("grandchild"));
    publish_pid(&directory.join("child"));
    if leaves_pipe {
        wait_for(&directory.join("exit-child"));
        // Simulate a transcriber leaving a worker behind after normal exit.
        // Only this owned fixture process handle is deliberately retained.
        std::mem::forget(descendant);
        return;
    }
    hold_bounded();
}

#[test]
fn fixture_parent() {
    let Some(directory) = directory() else { return };
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "windows_child_job::tests::fixture_child",
        "--nocapture",
    ]);
    let mut child = spawn(&command).unwrap();
    // This is the same boundary as transcription: input is unavailable until
    // successful atomic containment and the defensive membership verification.
    child.stdin.take().unwrap().write_all(FIXTURE).unwrap();
    wait_for(&directory.join("child"));
    let child_pid: u32 = std::fs::read_to_string(directory.join("child"))
        .unwrap()
        .parse()
        .unwrap();
    let grandchild_pid: u32 = std::fs::read_to_string(directory.join("grandchild"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(child.id().unwrap(), child_pid);
    assert!(child.status_snapshot().unwrap().is_none());
    assert!(child.job.is_some());
    assert!(child.contains_pid(child_pid).unwrap());
    assert_eq!(
        std::fs::canonicalize(child.member_executable(child_pid).unwrap()).unwrap(),
        std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
    );
    assert!(child.member_executable(std::process::id()).is_err());
    assert!(child.contains_pid(grandchild_pid).unwrap());
    assert!(!child.contains_pid(std::process::id()).unwrap());
    let members = child.job_pids().unwrap();
    assert!(members.contains(&child_pid) && members.contains(&grandchild_pid));
    assert!(!members.contains(&std::process::id()));
    let mode = std::env::var("PICKLE_JOB_FIXTURE_MODE").unwrap();
    if mode == "close" || mode == "finished" || mode == "stop" {
        if mode == "stop" {
            wait_for(&directory.join("close"));
            let mut output = child.stdout.take().unwrap();
            child.stop_and_wait(Duration::from_secs(5)).unwrap();
            assert!(child.job.is_none());
            assert!(child.contains_pid(std::process::id()).is_err());
            assert!(child.job_pids().is_err());
            let mut discarded = Vec::new();
            output.read_to_end(&mut discarded).unwrap();
        } else if mode == "close" {
            wait_for(&directory.join("close"));
            drop(child);
        } else {
            let began = Instant::now();
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                assert!(began.elapsed() < Duration::from_secs(10));
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(child.job.is_none());
            // Descendants must no longer keep the transcript pipe open.
            let mut output = Vec::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_end(&mut output)
                .unwrap();
        }
        std::fs::write(directory.join("closed"), b"job handle closed").unwrap();
        wait_for(&directory.join("finish"));
    } else {
        hold_bounded();
    }
}

struct Parent(std::process::Child);
impl Drop for Parent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Descendant(OwnedHandle);
impl Drop for Descendant {
    fn drop(&mut self) {
        // Error cleanup still addresses only the handle opened for our owned
        // fixture PID, never an executable name or a subsequently reused PID.
        if unsafe { WaitForSingleObject(raw(&self.0), 0) } != WAIT_OBJECT_0 {
            let _ = unsafe { TerminateProcess(raw(&self.0), 1) };
        }
    }
}
fn opened_fixture(path: &Path) -> Descendant {
    wait_for(path);
    let id: u32 = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
            false,
            id,
        )
    }
    .unwrap();
    Descendant(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}
fn lifecycle(mode: &str) {
    let root = tempfile::tempdir().unwrap();
    // Exercise CreateProcessW's exact Unicode executable name and argv[0]
    // quoting rather than relying on a whitespace-free CI checkout path.
    let executable = root.path().join("owned child fixture 🥒.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let mut parent = Parent(
        Command::new(&executable)
            .args([
                "--exact",
                "windows_child_job::tests::fixture_parent",
                "--nocapture",
            ])
            .env("PICKLE_JOB_FIXTURE_DIRECTORY", root.path())
            .env("PICKLE_JOB_FIXTURE_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let child = opened_fixture(&root.path().join("child"));
    let grandchild = opened_fixture(&root.path().join("grandchild"));
    assert!(parent.0.try_wait().unwrap().is_none());
    if mode == "close" || mode == "finished" || mode == "stop" {
        let signal = if mode == "close" || mode == "stop" {
            "close"
        } else {
            "exit-child"
        };
        std::fs::write(root.path().join(signal), b"finish owned child").unwrap();
        wait_for(&root.path().join("closed"));
        assert!(
            parent.0.try_wait().unwrap().is_none(),
            "The parent remains alive when its job handle closes"
        );
    } else {
        parent.0.kill().unwrap();
        parent.0.wait().unwrap();
    }
    for handle in [&child, &grandchild] {
        assert_eq!(
            unsafe { WaitForSingleObject(raw(&handle.0), 5000) },
            WAIT_OBJECT_0,
            "Owned child/descendant survived job close or forced parent termination"
        );
    }
    if mode == "close" || mode == "finished" || mode == "stop" {
        std::fs::write(root.path().join("finish"), b"exit owned fixture").unwrap();
        let began = Instant::now();
        loop {
            if let Some(status) = parent.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn closing_owned_job_terminates_child_and_descendant_with_parent_still_alive() {
    lifecycle("close");
}

#[test]
fn bounded_stop_confirms_exact_job_empty_root_signaled_and_pipe_closed() {
    lifecycle("stop");
}

#[test]
fn forced_parent_death_terminates_child_and_descendant_without_an_inherited_job_handle() {
    lifecycle("die");
}

#[test]
fn exited_transcriber_cannot_leave_a_descendant_holding_the_transcript_pipe() {
    lifecycle("finished");
}

#[test]
fn unsupported_command_configuration_fails_before_spawning() {
    let mut command = Command::new("relative.exe");
    assert!(spawn(&command).is_err());
    command = Command::new(std::env::current_exe().unwrap());
    command.env("PICKLE_JOB_UNUSED", "test only");
    assert!(spawn(&command).is_err());
}

#[test]
fn natural_exit_retains_job_until_empty_and_reports_zero_without_terminating_child() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg("--list"); // Lists this owned test executable's static tests; runs none.
    let mut child = spawn(&command).unwrap();
    assert!(child.id().unwrap() > 0);
    drop(child.stdin.take());
    let mut output = child.stdout.take().unwrap();
    let drain = std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut total = 0;
        loop {
            let count = output.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            total += count;
            assert!(total < 128 * 1024, "Owned test listing exceeded its bound");
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.status_snapshot().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "Owned listing did not exit");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(status.code(), Some(0));
    assert!(child.job.is_some(), "A status snapshot must retain the job");
    assert_eq!(child.status_snapshot().unwrap().unwrap().code(), Some(0));
    assert!(child.job.is_some());
    assert!(
        child
            .wait_for_exit(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
            .success()
    );
    assert!(child.job.is_none());
    assert!(!child.is_running().unwrap());
    let until = Instant::now() + Duration::from_secs(1);
    while !drain.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        drain.is_finished(),
        "Owned stdout did not close after natural exit"
    );
    drain.join().unwrap();
}
