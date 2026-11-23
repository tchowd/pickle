use super::*;
use std::io::Cursor;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn registry_path_encoding_is_lossless_terminated_bounded_and_counts_bytes() {
    let original: Vec<u16> = "C:\\Users\\例子\\Pickle 🥒\\com.pickle.reader.json"
        .encode_utf16()
        .collect();
    let (value, bytes) = registry_reg_sz(original.clone()).unwrap();
    assert_eq!(&value[..value.len() - 1], &original);
    assert_eq!(value.last(), Some(&0));
    assert_eq!(bytes as usize, value.len() * 2);
    // Windows OsStr is not required to be valid Unicode. Never lossy-convert
    // its raw UTF-16, or the installed registration can point to another path.
    let (unpaired, bytes) = registry_reg_sz([0x43, 0x3a, 0x5c, 0xd800]).unwrap();
    assert_eq!(unpaired, [0x43, 0x3a, 0x5c, 0xd800, 0]);
    assert_eq!(bytes, 10);
    assert!(registry_reg_sz([]).is_err());
    assert!(registry_reg_sz([0x43, 0, 0x44]).is_err());
    let (limit, bytes) = registry_reg_sz(std::iter::repeat_n(0x61, 32_766)).unwrap();
    assert_eq!(limit.len(), 32_767);
    assert_eq!(bytes, 65_534);
    assert!(registry_reg_sz(std::iter::repeat_n(0x61, 32_767)).is_err());
    // Infinite input is also bounded before allocation/validation.
    assert!(registry_reg_sz(std::iter::repeat(0x61)).is_err());
}

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
        document_url: None,
        browser_id: BrowserKind::Chrome.identity().into(),
    }
}

#[test]
fn legacy_registration_remains_readable_and_invalid_package_cannot_fall_back_to_path() {
    let registration: HostRegistration = serde_json::from_value(json!({"browser":"Chrome","allowed_origin":"chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/","bridge_directory":"bridge","application":"app"})).unwrap();
    assert!(registration.application_aumid.is_none());
    let mut registration = registration;
    registration.application_aumid = Some("malformed!Pickle --argument".into());
    assert!(
        launch_app(&registration)
            .unwrap_err()
            .to_string()
            .contains("Invalid packaged")
    );
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

#[test]
fn document_url_is_optional_exact_ephemeral_metadata_separate_from_reference() {
    let legacy = payload();
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("documentURL")
            .is_none()
    );
    let mut current = legacy.clone();
    let document = "https://example.com/Path%2Fpart?q=Caf%C3%A9&b=2#route-one";
    current.document_url = Some(document.into());
    current.validate().unwrap();
    let decoded: BrowserPayload =
        serde_json::from_value(serde_json::to_value(&current).unwrap()).unwrap();
    assert_eq!(decoded.document_url.as_deref(), Some(document));
    assert_eq!(decoded.reference.url, legacy.reference.url);
    for invalid in [
        "https://example.com/\n".to_owned(),
        "https://example.com\\path".into(),
        "https://user:pass@example.com/".into(),
        format!("https://example.com/{}", "a".repeat(4096)),
    ] {
        current.document_url = Some(invalid);
        assert!(current.validate().is_err());
    }
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
        received.try_admit().unwrap();
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
    value["documentURL"] = json!("https://example.com/current?section=2#fragment");
    value["bundleID"] = json!("malicious page identity");
    let registration = HostRegistration {
        browser: BrowserKind::Edge,
        allowed_origin: "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/".into(),
        bridge_directory: directory.path().to_path_buf(),
        application: directory.path().join("unused-app"),
        application_aumid: None,
    };
    let task = tokio::task::spawn_blocking(move || relay_native_message(value, &registration));
    let received = rx.recv().await.unwrap();
    assert_eq!(received.payload.browser_id, BrowserKind::Edge.identity());
    assert_eq!(
        received.payload.document_url.as_deref(),
        Some("https://example.com/current?section=2#fragment")
    );
    assert_eq!(
        received.payload.reference.url,
        "https://example.com/article"
    );
    received.try_admit().unwrap();
    received.reply.send(Ok(())).unwrap();
    assert_eq!(task.await.unwrap().unwrap()["ok"], "true");
}

#[tokio::test]
async fn queued_browser_request_expires_with_its_owned_relay_and_fresh_request_still_works() {
    let directory = tempfile::tempdir().unwrap();
    let (tx, mut rx) = unbounded_channel();
    let bridge = BrowserBridge::start_with_request_budget(
        directory.path().to_path_buf(),
        tx,
        CancellationToken::new(),
        Duration::from_millis(80),
    )
    .await
    .unwrap();
    let connection: Connection =
        serde_json::from_slice(&std::fs::read(directory.path().join("connection.json")).unwrap())
            .unwrap();
    let mut socket = TcpStream::connect(("127.0.0.1", connection.port))
        .await
        .unwrap();
    socket
        .write_all(
            format!(
                "{}\n",
                json!({"token":connection.token,"payload":payload()})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let queued = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(!queued.reply.is_closed());
    let preparation = queued.preparation_token();
    assert!(!preparation.is_cancelled());
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), socket.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert!(bytes.is_empty());
    assert!(queued.reply.is_closed());
    assert!(preparation.is_cancelled());
    assert!(queued.try_admit().unwrap_err().contains("expired"));
    assert!(!queued.admission.accepted());
    let consumer = tokio::spawn(async move {
        let fresh = rx.recv().await.unwrap();
        fresh.try_admit().unwrap();
        fresh.reply.send(Ok(())).unwrap();
    });
    assert_eq!(send(directory.path(), None, &payload()).await["ok"], "true");
    consumer.await.unwrap();
    bridge.stop();
}

#[test]
fn browser_admission_checks_deadline_even_with_live_reply_and_is_claimed_once() {
    let request = |deadline| {
        let (reply, receiver) = oneshot::channel();
        (
            BrowserRequest {
                payload: payload(),
                reply,
                admission: Arc::new(BrowserAdmission {
                    deadline,
                    state: AdmissionMutex::new(AdmissionState::Pending),
                    cancel: CancellationToken::new(),
                }),
            },
            receiver,
        )
    };
    let (expired, _receiver) = request(Instant::now() - Duration::from_millis(1));
    assert!(!expired.reply.is_closed());
    assert!(expired.try_admit().is_err());
    let (closed, receiver) = request(Instant::now() + Duration::from_secs(1));
    drop(receiver);
    assert!(closed.try_admit().is_err());
    let (live, _receiver) = request(Instant::now() + Duration::from_secs(1));
    live.try_admit().unwrap();
    assert!(live.try_admit().is_err());
    assert!(live.admission.accepted());
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
