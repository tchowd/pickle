use super::*;
use pickle_providers::browser::{BrowserBridge, BrowserKind, BrowserRequest};
use rune::{Env, Runtime};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tokio_util::sync::CancellationToken;

async fn queued_request() -> (tempfile::TempDir, BrowserBridge, BrowserRequest, TcpStream) {
    queued_request_with("Controlled Café selection.", uuid::Uuid::new_v4()).await
}
async fn queued_request_with(
    selection: &str,
    capture_id: uuid::Uuid,
) -> (tempfile::TempDir, BrowserBridge, BrowserRequest, TcpStream) {
    queued_document_request(selection, capture_id, None).await
}
async fn queued_document_request(
    selection: &str,
    capture_id: uuid::Uuid,
    document_url: Option<&str>,
) -> (tempfile::TempDir, BrowserBridge, BrowserRequest, TcpStream) {
    let directory = tempfile::tempdir().unwrap();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    let bridge = BrowserBridge::start(
        directory.path().to_owned(),
        sender,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let connection: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("connection.json")).unwrap())
            .unwrap();
    let mut socket = TcpStream::connect(("127.0.0.1", connection["port"].as_u64().unwrap() as u16))
        .await
        .unwrap();
    let message = serde_json::json!({"token":connection["token"],"payload":{
        "id":capture_id,"capturedAt":chrono::Utc::now().timestamp_millis() as f64/1000.0,
        "selection":selection,"bundleID":BrowserKind::Chrome.identity(),"documentURL":document_url,
        "reference":{"url":"https://example.com/owned-video","title":"Owned caption fixture","kind":"video captions","text":"[0:12] Controlled caption context.","timestamp":12.0}
    }});
    socket
        .write_all(format!("{message}\n").as_bytes())
        .await
        .unwrap();
    let request = tokio::time::timeout(std::time::Duration::from_secs(1), requests.recv())
        .await
        .unwrap()
        .unwrap();
    (directory, bridge, request, socket)
}
fn native_target() -> pickle_platform::CapturedSelection {
    pickle_platform::CapturedSelection {
        text: String::new(),
        app_name: "Owned native fixture".into(),
        app_id: if cfg!(windows) {
            r"windows:C:\OwnedBrowser\chrome.exe".into()
        } else {
            BrowserKind::Chrome.identity().into()
        },
        bounds: None,
        source_url: None,
        source_pid: std::process::id().wrapping_add(1),
        source_window: Some(42),
    }
}
fn runtime() -> Runtime<AppState> {
    let mut state = AppState::default();
    state.sample_result();
    state.settings.web_context_enabled = true;
    Runtime::new(
        state,
        crate::views::view,
        (560.0, 640.0),
        Env::new(true, 1.0),
    )
}
async fn reply(socket: &mut TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        socket.read_to_end(&mut bytes),
    )
    .await
    .unwrap()
    .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn cancelled_queued_browser_request_cannot_reset_context_reader_or_confirmation() {
    let (_directory, bridge, request, mut socket) = queued_request().await;
    let mut runtime = runtime();
    runtime.state.route = Route::Library(true);
    runtime.state.library_delete_confirmation = Some(false);
    runtime.state.context_state.pending = 1;
    runtime.state.capture_target = Some(native_target());
    let snapshot = runtime.state.snapshot.clone();
    let answer = serde_json::to_value(&runtime.state.result).unwrap();
    let context_id = runtime.state.context_state.id;
    let cancel = runtime.state.context_state.cancel.clone();
    let revision = runtime.state.revision;
    let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
    bridge.stop();
    let mut bytes = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        socket.read_to_end(&mut bytes),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(bytes.is_empty() && request.reply.is_closed());
    let target = resolve_browser_target(|| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        Ok(native_target())
    })
    .await;
    runtime.update(move |state, tx| complete_browser_request(state, request, target, attempt, tx));
    assert_eq!(runtime.state.snapshot, snapshot);
    assert_eq!(serde_json::to_value(&runtime.state.result).unwrap(), answer);
    assert_eq!(runtime.state.route, Route::Library(true));
    assert_eq!(runtime.state.library_delete_confirmation, Some(false));
    assert_eq!(runtime.state.context_state.id, context_id);
    assert_eq!(runtime.state.context_state.pending, 1);
    assert!(!cancel.is_cancelled());
    assert_eq!(runtime.state.revision, revision);
}

#[tokio::test]
async fn accepted_browser_reference_retains_only_verified_app_audio_scope_without_ocr() {
    let (_directory, _bridge, request, mut socket) = queued_request().await;
    let mut runtime = runtime();
    runtime.state.settings.screen_context_enabled = true;
    let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
    runtime.update(move |state, tx| {
        complete_browser_request(state, request, Ok(native_target()), attempt, tx)
    });
    assert_eq!(reply(&mut socket).await["ok"], "true");
    let snapshot = runtime.state.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.text, "Controlled Café selection.");
    assert_eq!(snapshot.method, "Browser extension");
    assert_eq!(snapshot.app_id, BrowserKind::Chrome.identity());
    assert_eq!(
        runtime.state.capture_target.as_ref().unwrap().source_window,
        Some(42)
    );
    assert!(
        runtime
            .state
            .capture_target
            .as_ref()
            .unwrap()
            .source_url
            .is_none()
    );
    let reference = serde_json::to_value(&runtime.state.context_state.reference).unwrap();
    assert_eq!(reference["kind"], "video captions");
    assert_eq!(reference["timestamp"], 12.0);
    runtime.update(|state, _| state.collect_context());
    assert_eq!(
        serde_json::to_value(&runtime.state.context_state.reference).unwrap(),
        reference
    );
    assert!(runtime.state.context_state.page.is_none());
    assert_eq!(runtime.state.context_state.pending, 0);
    let notice = runtime.state.context_state.notice.as_deref().unwrap();
    assert!(
        notice.contains("Screen context is unavailable") && notice.contains("other playing tabs")
    );
    assert!(runtime.state.services.is_none());
}

#[tokio::test]
async fn unavailable_or_wrong_native_identity_never_reuses_old_capture_target() {
    for wrong in [false, true] {
        let (_directory, _bridge, request, mut socket) = queued_request().await;
        let mut runtime = runtime();
        runtime.state.capture_target = Some(native_target());
        let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
        let target = if wrong {
            let mut target = native_target();
            target.source_url = Some("https://example.com/page-supplied-url".into());
            Ok(target)
        } else {
            Err(pickle_platform::PlatformError(
                "Controlled unavailable source".into(),
            ))
        };
        runtime
            .update(move |state, tx| complete_browser_request(state, request, target, attempt, tx));
        assert_eq!(reply(&mut socket).await["ok"], "true");
        assert!(runtime.state.capture_target.is_none());
        assert!(runtime.state.context_state.reference.is_some());
        assert!(
            runtime
                .state
                .context_state
                .notice
                .as_deref()
                .unwrap()
                .contains("Audio is unavailable for this capture")
        );
        assert!(
            !runtime
                .state
                .context_state
                .notice
                .as_deref()
                .unwrap()
                .contains("Screen context")
        );
    }
}

#[tokio::test]
async fn delayed_resolution_cannot_replace_a_newer_source_or_bypass_changed_policy() {
    for policy_change in [false, true] {
        let (_directory, _bridge, request, mut socket) = queued_request().await;
        let mut runtime = runtime();
        let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
        if policy_change {
            runtime.state.paused = true;
        } else {
            runtime.state.sample();
        }
        let snapshot = runtime.state.snapshot.clone();
        let context_id = runtime.state.context_state.id;
        runtime.update(move |state, tx| {
            complete_browser_request(state, request, Ok(native_target()), attempt, tx)
        });
        assert!(reply(&mut socket).await["error"].is_string());
        assert_eq!(runtime.state.snapshot, snapshot);
        assert_eq!(runtime.state.context_state.id, context_id);
        assert!(runtime.state.context_state.reference.is_none());
    }
}

#[tokio::test]
async fn latest_browser_intent_wins_both_completion_orders_even_with_identical_capture_ids() {
    for newest_first in [false, true] {
        let capture_id = uuid::Uuid::new_v4();
        let (_dir_a, _bridge_a, older, mut socket_a) =
            queued_request_with("Older controlled intent.", capture_id).await;
        let (_dir_b, _bridge_b, newer, mut socket_b) =
            queued_request_with("Newest controlled intent.", capture_id).await;
        let mut runtime = runtime();
        let original = runtime.state.snapshot.clone();
        let a = stage_browser_intent(&mut runtime.state, &older).unwrap();
        let b = stage_browser_intent(&mut runtime.state, &newer).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(a.revision, b.revision);
        assert_eq!(runtime.state.snapshot, original);
        if newest_first {
            runtime.update(move |state, tx| {
                complete_browser_request(state, newer, Ok(native_target()), b, tx)
            });
            assert_eq!(reply(&mut socket_b).await["ok"], "true");
            runtime.update(move |state, tx| {
                complete_browser_request(state, older, Ok(native_target()), a, tx)
            });
        } else {
            runtime.update(move |state, tx| {
                complete_browser_request(state, older, Ok(native_target()), a, tx)
            });
            assert_eq!(runtime.state.snapshot, original);
            assert_eq!(runtime.state.revision, b.revision);
            runtime.update(move |state, tx| {
                complete_browser_request(state, newer, Ok(native_target()), b, tx)
            });
            assert_eq!(reply(&mut socket_b).await["ok"], "true");
        }
        assert!(reply(&mut socket_a).await["error"].is_string());
        assert_eq!(
            runtime.state.snapshot.as_ref().unwrap().text,
            "Newest controlled intent."
        );
        assert!(runtime.state.context_state.browser_attempt.is_none());
    }
}

#[tokio::test]
async fn native_resolver_wait_is_bounded_and_does_not_execute_on_ui_thread() {
    let owner = std::thread::current().id();
    let began = std::time::Instant::now();
    let target = resolve_browser_target(move || {
        assert_ne!(std::thread::current().id(), owner);
        std::thread::sleep(std::time::Duration::from_millis(600));
        Ok(native_target())
    })
    .await;
    assert!(target.unwrap_err().0.contains("timed out"));
    assert!(began.elapsed() < std::time::Duration::from_secs(2));
}

#[tokio::test]
async fn app_scoped_audio_preserves_captions_and_enters_requests_as_separately_labeled_context() {
    let (_directory, _bridge, request, mut socket) = queued_request().await;
    let mut runtime = runtime();
    let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
    runtime.update(move |state, tx| {
        complete_browser_request(state, request, Ok(native_target()), attempt, tx)
    });
    assert_eq!(reply(&mut socket).await["ok"], "true");
    let reference = serde_json::to_value(&runtime.state.context_state.reference).unwrap();
    let snapshot = runtime.state.snapshot.clone();
    runtime.update(|state, _| {
        state.apply_audio_text("Controlled app audio, possibly from another tab.".into())
    });
    assert_eq!(
        serde_json::to_value(&runtime.state.context_state.reference).unwrap(),
        reference
    );
    assert_eq!(runtime.state.snapshot, snapshot);
    assert_eq!(
        runtime.state.context_state.audio_text,
        "Controlled app audio, possibly from another tab."
    );
    let mut input = pickle_core::RequestInput::new(
        runtime.state.snapshot.clone().unwrap(),
        pickle_core::ReadingAction::Simplify,
    );
    input.reference = runtime.state.context_state.reference.clone();
    input.page_context = runtime.state.context_state.reading_text();
    input.validate().unwrap();
    assert_eq!(serde_json::to_value(&input.reference).unwrap(), reference);
    assert!(
        input
            .page_context
            .contains("other browser tabs or app audio")
    );
    assert!(input.page_context.contains("no webpage linkage verified"));
    assert!(
        input
            .page_context
            .contains("Controlled app audio, possibly from another tab.")
    );
    assert!(
        !input.page_context.contains("https://") && !input.page_context.contains("video captions")
    );
    let source = input.reading_source();
    assert!(source.contains("[0:12] Controlled caption context."));
    assert_eq!(source.matches("https://example.com/owned-video").count(), 1);
    assert_eq!(input.reference.as_ref().unwrap().kind, "video captions");
    assert_eq!(input.reference.as_ref().unwrap().timestamp, Some(12.0));
    let old_id = runtime.state.context_state.id;
    let old_cancel = runtime.state.context_state.cancel.clone();
    runtime.state.context_state.pending = 1;
    runtime.update(|state, _| state.cancel());
    assert!(old_cancel.is_cancelled());
    runtime.update(move |state, tx| {
        assert!(!state.complete_audio_capture(old_id, Ok("Cancelled new recording.".into()), tx))
    });
    assert_eq!(
        serde_json::to_value(&runtime.state.context_state.reference).unwrap(),
        reference
    );
    assert_eq!(
        runtime.state.context_state.audio_text,
        "Controlled app audio, possibly from another tab."
    );
    assert_eq!(runtime.state.context_state.pending, 0);
}

#[test]
fn app_audio_never_creates_a_page_reference_even_with_an_exposed_native_url() {
    let mut runtime = runtime();
    runtime.state.snapshot.as_mut().unwrap().method = "Accessibility".into();
    runtime.state.snapshot.as_mut().unwrap().source_url =
        Some("https://example.com/native-page".into());
    runtime.update(|state, _| state.apply_audio_text("A controlled native-app transcript.".into()));
    assert!(runtime.state.context_state.reference.is_none());
    assert_eq!(
        runtime.state.context_state.audio_text,
        "A controlled native-app transcript."
    );
    assert!(
        !runtime
            .state
            .context_state
            .reading_text()
            .contains("https://example.com/native-page")
    );
    assert_eq!(
        runtime
            .state
            .snapshot
            .as_ref()
            .unwrap()
            .source_url
            .as_deref(),
        Some("https://example.com/native-page")
    );
}

#[test]
fn audio_completion_requires_the_live_context_generation_and_does_not_clear_newer_work() {
    let mut runtime = runtime();
    let old_id = runtime.state.context_state.id;
    runtime.state.context_state.pending = 1;
    runtime.update(move |state, tx| {
        assert!(state.complete_audio_capture(
            old_id,
            Ok("Accepted app-scoped transcript.".into()),
            tx
        ))
    });
    assert_eq!(
        runtime.state.context_state.audio_text,
        "Accepted app-scoped transcript."
    );
    runtime.update(move |state, tx| {
        assert!(!state.complete_audio_capture(old_id, Ok("Duplicate callback.".into()), tx))
    });
    assert_eq!(
        runtime.state.context_state.audio_text,
        "Accepted app-scoped transcript."
    );
    // Cancel/reset uses the production token generation and clears old audio.
    runtime.update(|state, _| {
        state.clear_context();
        state.sample();
    });
    let new_id = runtime.state.context_state.id;
    assert_ne!(old_id, new_id);
    runtime.state.context_state.pending = 1;
    runtime.state.context_state.notice = Some("New context work remains active.".into());
    runtime.state.notice = Some("New operation status".into());
    let cancel = runtime.state.context_state.cancel.clone();
    runtime.update(move |state, tx| {
        assert!(!state.complete_audio_capture(
            old_id,
            Ok("Stale audio from old source.".into()),
            tx
        ))
    });
    assert_eq!(runtime.state.context_state.id, new_id);
    assert_eq!(runtime.state.context_state.pending, 1);
    assert!(runtime.state.context_state.audio_text.is_empty());
    assert!(runtime.state.context_state.reference.is_none());
    assert_eq!(
        runtime.state.context_state.notice.as_deref(),
        Some("New context work remains active.")
    );
    assert_eq!(
        runtime.state.notice.as_deref(),
        Some("New operation status")
    );
    assert!(!cancel.is_cancelled());
}

fn prepared_page() -> pickle_platform::CapturedPage {
    pickle_platform::CapturedPage {
        jpeg: Vec::new(),
        text: "Owned browser-window OCR fixture.".into(),
        truncated: false,
    }
}

#[tokio::test]
async fn verified_document_page_is_ephemeral_and_preserves_the_independent_caption_reference() {
    let document = "https://example.com/document%2Froute?q=2#fragment-only-for-matching";
    let (_directory, _bridge, request, mut socket) = queued_document_request(
        "Controlled Café selection.",
        uuid::Uuid::new_v4(),
        Some(document),
    )
    .await;
    let mut runtime = runtime();
    runtime.state.settings.screen_context_enabled = true;
    let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
    runtime.update(move |state, tx| {
        complete_browser_request(
            state,
            request,
            BrowserPreparation {
                target: Ok(native_target()),
                page: Some(prepared_page()),
            },
            attempt,
            tx,
        )
    });
    assert_eq!(reply(&mut socket).await["ok"], "true");
    assert_eq!(
        runtime.state.context_state.page.as_ref().unwrap().text,
        "Owned browser-window OCR fixture."
    );
    let reference = serde_json::to_value(&runtime.state.context_state.reference).unwrap();
    assert_eq!(reference["url"], "https://example.com/owned-video");
    assert_eq!(reference["timestamp"], 12.0);
    runtime.update(|state, _| state.collect_context());
    assert_eq!(
        serde_json::to_value(&runtime.state.context_state.reference).unwrap(),
        reference
    );
    assert!(runtime.state.context_state.page.is_some());
    assert!(
        runtime
            .state
            .context_state
            .notice
            .as_deref()
            .unwrap()
            .contains("recognition errors")
    );
    let mut input = pickle_core::RequestInput::new(
        runtime.state.snapshot.clone().unwrap(),
        pickle_core::ReadingAction::Simplify,
    );
    input.reference = runtime.state.context_state.reference.clone();
    input.page_context = runtime.state.context_state.reading_text();
    input.validate().unwrap();
    let serialized = serde_json::to_string(&input).unwrap();
    assert!(!serialized.contains(document));
    assert!(serialized.contains("Owned browser-window OCR fixture."));
    assert_eq!(
        runtime
            .state
            .snapshot
            .as_ref()
            .unwrap()
            .source_url
            .as_deref(),
        Some("https://example.com/owned-video")
    );
    assert!(
        runtime
            .state
            .capture_target
            .as_ref()
            .unwrap()
            .source_url
            .is_none()
    );
}

#[tokio::test]
async fn prepared_page_requires_opt_in_live_policy_and_a_new_verified_target() {
    for mode in 0..5 {
        let (_directory, _bridge, request, mut socket) = queued_document_request(
            "Controlled source.",
            uuid::Uuid::new_v4(),
            if mode == 0 {
                None
            } else {
                Some("https://example.com/page#one")
            },
        )
        .await;
        let mut runtime = runtime();
        runtime.state.settings.screen_context_enabled = true;
        let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
        let prior = runtime.state.snapshot.clone();
        let target = if mode == 1 {
            Err(pickle_platform::PlatformError(
                "Owned unavailable fixture".into(),
            ))
        } else {
            Ok(native_target())
        };
        if mode == 2 {
            runtime.state.settings.screen_context_enabled = false;
        }
        if mode == 3 {
            runtime.state.paused = true;
        }
        if mode == 4 {
            runtime.update(|state, _| state.clear_context());
        }
        runtime.update(move |state, tx| {
            complete_browser_request(
                state,
                request,
                BrowserPreparation {
                    target,
                    page: Some(prepared_page()),
                },
                attempt,
                tx,
            )
        });
        let response = reply(&mut socket).await;
        if mode >= 3 {
            assert!(response["error"].is_string());
            assert_eq!(runtime.state.snapshot, prior);
        } else {
            assert_eq!(response["ok"], "true");
            assert!(runtime.state.context_state.reference.is_some());
        }
        assert!(runtime.state.context_state.page.is_none());
    }
}

#[tokio::test]
async fn slow_optional_page_preparation_is_cancelled_and_text_still_admitted() {
    let (_directory, _bridge, request, mut socket) = queued_document_request(
        "Controlled text fallback.",
        uuid::Uuid::new_v4(),
        Some("https://example.com/page#one"),
    )
    .await;
    let mut runtime = runtime();
    runtime.state.settings.screen_context_enabled = true;
    let attempt = stage_browser_intent(&mut runtime.state, &request).unwrap();
    let parent = runtime
        .state
        .context_state
        .browser_preparation
        .as_ref()
        .unwrap()
        .clone();
    let worker_cancel = parent.child_token();
    let observed_cancel = worker_cancel.clone();
    let (finished_sender, finished) = tokio::sync::oneshot::channel();
    let page = prepare_browser_page(
        worker_cancel,
        std::time::Duration::from_millis(20),
        move |cancel| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while !cancel.is_cancelled() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let _ = finished_sender.send(cancel.is_cancelled());
            Ok(prepared_page())
        },
    )
    .await;
    assert!(page.is_none() && observed_cancel.is_cancelled());
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), finished)
            .await
            .unwrap()
            .unwrap()
    );
    assert!(!parent.is_cancelled());
    runtime.update(move |state, tx| {
        complete_browser_request(
            state,
            request,
            BrowserPreparation {
                target: Ok(native_target()),
                page,
            },
            attempt,
            tx,
        )
    });
    assert_eq!(reply(&mut socket).await["ok"], "true");
    assert_eq!(
        runtime.state.snapshot.as_ref().unwrap().text,
        "Controlled text fallback."
    );
    assert!(runtime.state.context_state.page.is_none());
}

#[tokio::test]
async fn newer_browser_intent_and_source_reset_cancel_preparation_without_changing_source() {
    let (_a, _ba, older, _sa) = queued_request().await;
    let (_b, _bb, newer, _sb) = queued_request().await;
    let mut runtime = runtime();
    let source = runtime.state.snapshot.clone();
    stage_browser_intent(&mut runtime.state, &older).unwrap();
    let older_cancel = runtime
        .state
        .context_state
        .browser_preparation
        .as_ref()
        .unwrap()
        .clone();
    stage_browser_intent(&mut runtime.state, &newer).unwrap();
    assert!(older_cancel.is_cancelled());
    let newer_cancel = runtime
        .state
        .context_state
        .browser_preparation
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(runtime.state.snapshot, source);
    assert!(!newer_cancel.is_cancelled());
    runtime.update(|state, _| state.clear_context());
    assert!(newer_cancel.is_cancelled());
    assert_eq!(runtime.state.snapshot, source);
}
