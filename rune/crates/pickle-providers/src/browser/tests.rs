use super::*;
use std::io::Cursor;
use tokio::sync::mpsc::unbounded_channel;

fn payload() -> BrowserPayload {
    BrowserPayload {
        id: Uuid::new_v4(),
        captured_at: Utc::now().timestamp_millis() as f64 / 1000.0,
        selection: "Selected passage".into(),
        reference: WebReference {
            url: "https://example.com/article".into(),
            title: "Fixture article".into(),
            kind: "article".into(),
            text: "Public source text".into(),
            timestamp: None,
        },
        browser_id: BrowserKind::Chrome.identity().into(),
    }
}

#[test]
fn native_framing_preserves_unicode_and_bounds_allocations() {
    let expected = json!({"text":"日本語 café"});
    let mut output = vec![];
    write_frame(&mut output, expected.clone()).unwrap();
    assert_eq!(
        u32::from_le_bytes(output[..4].try_into().unwrap()) as usize,
        output.len() - 4
    );
    assert_eq!(read_frame(&mut Cursor::new(output)).unwrap(), expected);
    assert!(read_frame(&mut Cursor::new((90_001u32).to_le_bytes())).is_err());
    assert!(read_frame(&mut Cursor::new([1, 0, 0, 0])).is_err());
    assert!(read_frame(&mut Cursor::new([0, 0, 0, 0])).is_err());
}

#[test]
fn payload_validation_rejects_expiry_identity_and_oversized_selection() {
    let good = payload();
    assert!(good.validate().is_ok());
    let mut value = good.clone();
    value.captured_at -= 61.0;
    assert!(value.validate().is_err());
    value = good.clone();
    value.captured_at = f64::NAN;
    assert!(value.validate().is_err());
    value = good.clone();
    value.browser_id = "unregistered-browser".into();
    assert!(value.validate().is_err());
    value = good;
    value.selection = "a".repeat(12_001);
    assert!(value.validate().is_err());
}

async fn send(directory: &Path, token: Option<&str>, payload: &BrowserPayload) -> Value {
    let connection: Connection =
        serde_json::from_slice(&std::fs::read(directory.join("connection.json")).unwrap()).unwrap();
    let mut socket = TcpStream::connect(("127.0.0.1", connection.port))
        .await
        .unwrap();
    let bytes = format!(
        "{}\n",
        json!({"token":token.unwrap_or(&connection.token),"payload":payload})
    );
    socket.write_all(bytes.as_bytes()).await.unwrap();
    let mut response = vec![];
    socket.read_to_end(&mut response).await.unwrap();
    serde_json::from_slice(&response).unwrap()
}

#[tokio::test]
async fn authenticated_bridge_delivers_context_and_rejects_replays() {
    let directory = tempfile::tempdir().unwrap();
    let (tx, mut rx) = unbounded_channel();
    let bridge = BrowserBridge::start(directory.path().to_path_buf(), tx, CancellationToken::new())
        .await
        .unwrap();
    let expected = payload();
    let id = expected.id;
    let consumer = tokio::spawn(async move {
        let received = rx.recv().await.unwrap();
        assert_eq!(received.payload.id, id);
        received.reply.send(Ok(())).unwrap();
    });
    let response = send(directory.path(), None, &expected).await;
    assert_eq!(response["ok"], "true");
    consumer.await.unwrap();
    assert!(
        send(directory.path(), None, &expected).await["error"]
            .as_str()
            .unwrap()
            .contains("already used")
    );
    assert!(
        send(directory.path(), Some("incorrect"), &payload()).await["error"]
            .as_str()
            .unwrap()
            .contains("rejected")
    );
    bridge.stop();
    assert!(!directory.path().join("connection.json").exists());
}

#[tokio::test]
async fn application_policy_can_reject_browser_context() {
    let directory = tempfile::tempdir().unwrap();
    let (tx, mut rx) = unbounded_channel();
    let _bridge =
        BrowserBridge::start(directory.path().to_path_buf(), tx, CancellationToken::new())
            .await
            .unwrap();
    tokio::spawn(async move {
        rx.recv()
            .await
            .unwrap()
            .reply
            .send(Err("Browser context is disabled in Settings.".into()))
            .unwrap();
    });
    let response = send(directory.path(), None, &payload()).await;
    assert_eq!(
        response["error"],
        "Browser context is disabled in Settings."
    );
}

#[tokio::test]
async fn native_host_relay_overwrites_page_supplied_browser_identity() {
    let directory = tempfile::tempdir().unwrap();
    let (tx, mut rx) = unbounded_channel();
    let _bridge =
        BrowserBridge::start(directory.path().to_path_buf(), tx, CancellationToken::new())
            .await
            .unwrap();
    let mut value = serde_json::to_value(payload()).unwrap();
    value["bundleID"] = json!("malicious page identity");
    let registration = HostRegistration {
        browser: BrowserKind::Edge,
        allowed_origin: "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/".into(),
        bridge_directory: directory.path().to_path_buf(),
        application: directory.path().join("unused-app"),
    };
    let task = tokio::task::spawn_blocking(move || relay_native_message(value, &registration));
    let received = rx.recv().await.unwrap();
    assert_eq!(received.payload.browser_id, BrowserKind::Edge.identity());
    received.reply.send(Ok(())).unwrap();
    assert_eq!(task.await.unwrap().unwrap()["ok"], "true");
}

#[tokio::test]
async fn old_bridge_does_not_remove_another_instance_connection_file() {
    let directory = tempfile::tempdir().unwrap();
    let (tx, _) = unbounded_channel();
    let bridge = BrowserBridge::start(directory.path().to_path_buf(), tx, CancellationToken::new())
        .await
        .unwrap();
    private_json(
        &directory.path().join("connection.json"),
        &Connection {
            port: 1234,
            token: "another-token".into(),
        },
    )
    .unwrap();
    drop(bridge);
    assert!(directory.path().join("connection.json").exists());
}

#[test]
fn installation_rejects_bad_extension_or_missing_packaged_host_before_writes() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent");
    assert!(
        install_browser_host(
            BrowserKind::Chrome,
            "bad",
            &absent,
            &absent,
            directory.path()
        )
        .is_err()
    );
    assert!(
        install_browser_host(
            BrowserKind::Chrome,
            &"a".repeat(32),
            &absent,
            &absent,
            directory.path()
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn bridge_configuration_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let (tx, _) = unbounded_channel();
    let _bridge =
        BrowserBridge::start(directory.path().to_path_buf(), tx, CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(
        std::fs::metadata(directory.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(directory.path().join("connection.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
