//! Exercises the shipped executable's stdin/stdout protocol and real loopback relay.
use std::{
    io::Write,
    process::{Command, Stdio},
};

use chrono::Utc;
use pickle_providers::browser::{BrowserBridge, BrowserKind, HostRegistration};
use serde_json::{Value, json};
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
async fn native_host_executable_preserves_frames_and_enforces_origin() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join(if cfg!(windows) {
        "pickle-browser-host.exe"
    } else {
        "pickle-browser-host"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_pickle-browser-host"), &executable).unwrap();
    let origin = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
    let registration = HostRegistration {
        browser: BrowserKind::Chrome,
        allowed_origin: origin.into(),
        bridge_directory: directory.path().join("bridge"),
        application: directory.path().join("unused-app"),
    };
    std::fs::write(
        directory.path().join("registration.json"),
        serde_json::to_vec(&registration).unwrap(),
    )
    .unwrap();
    let (tx, mut rx) = unbounded_channel();
    let _bridge = BrowserBridge::start(registration.bridge_directory, tx, CancellationToken::new())
        .await
        .unwrap();
    let payload = json!({"id":Uuid::new_v4(),"capturedAt":Utc::now().timestamp_millis() as f64/1000.0,"selection":"日本語 café","bundleID":"page-controlled identity","reference":{"url":"https://example.com/article","title":"Fixture","kind":"article","text":"A source passage."}});
    let frame = serde_json::to_vec(&payload).unwrap();
    let run = move |origin: String, frame: Vec<u8>| {
        let mut child = Command::new(&executable)
            .arg(origin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin
            .write_all(&(frame.len() as u32).to_le_bytes())
            .unwrap();
        stdin.write_all(&frame).unwrap();
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let length = u32::from_le_bytes(output.stdout[..4].try_into().unwrap()) as usize;
        assert_eq!(output.stdout.len(), length + 4);
        serde_json::from_slice::<Value>(&output.stdout[4..]).unwrap()
    };
    let task = tokio::task::spawn_blocking(move || run(origin.into(), frame));
    let received = rx.recv().await.unwrap();
    assert_eq!(received.payload.selection, "日本語 café");
    assert_eq!(received.payload.browser_id, BrowserKind::Chrome.identity());
    received.reply.send(Ok(())).unwrap();
    assert_eq!(task.await.unwrap()["ok"], "true");

    let binary = directory.path().join(if cfg!(windows) {
        "pickle-browser-host.exe"
    } else {
        "pickle-browser-host"
    });
    let output = Command::new(binary)
        .arg("chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout[4..]).unwrap();
    assert_eq!(reply["error"], "Browser extension origin rejected.");
}
