//! Regressions from the independent lifecycle/privacy review. No native capture or vault calls.
use crate::controller::{Activity, AppState, Panel};
use pickle_core::{SelectionSnapshot, WebReference};
use pickle_platform::{CapturedPage, CapturedSelection};
use rune::TextEdit;

fn source(app_id: &str) -> CapturedSelection {
    CapturedSelection {
        text: "A source-specific selected passage".into(),
        app_name: "Source editor".into(),
        app_id: app_id.into(),
        bounds: None,
        source_url: Some("https://example.com/article".into()),
        source_pid: 123,
        source_window: Some(456),
    }
}
fn reference() -> WebReference {
    WebReference {
        url: "https://example.com/article".into(),
        title: "Original source article".into(),
        kind: "article".into(),
        text: "Old source article body".into(),
        timestamp: None,
    }
}
fn with_context(app_id: &str) -> AppState {
    let mut state = AppState {
        capture_target: Some(source(app_id)),
        snapshot: Some(SelectionSnapshot::new(
            "Old selected passage",
            "Source editor",
            app_id,
        )),
        ..AppState::default()
    };
    state.context_state.page = Some(CapturedPage {
        jpeg: vec![1, 2, 3],
        text: "Old source screenshot OCR".into(),
        truncated: false,
    });
    state.context_state.reference = Some(reference());
    state.context_state.visual_summary = "Old source visual summary".into();
    state.context_state.audio_text = "Old source audio transcript".into();
    state.context = TextEdit::new("Old manually supplied surrounding text");
    state.settings.local_vision_model = "downloaded-vision-model".into();
    state.settings.screen_context_enabled = true;
    state
}
fn message(state: &AppState) -> String {
    match &state.activity {
        Activity::Failed(message) => message.clone(),
        _ => state.notice.clone().unwrap_or_default(),
    }
    .to_lowercase()
}

#[test]
fn committing_manual_text_cannot_reuse_another_apps_context_or_capture_target() {
    let mut state = with_context("source.editor");
    let old_context_id = state.context_state.id;
    let old_cancel = state.context_state.cancel.clone();
    state.context_state.pending = 2;
    state.source = TextEdit::new("A newly pasted passage about a different topic.");
    state.commit_source();
    assert_eq!(
        state.snapshot.as_ref().unwrap().text,
        "A newly pasted passage about a different topic."
    );
    assert_eq!(state.snapshot.as_ref().unwrap().app_id, "manual");
    assert_eq!(state.panel, Panel::Choosing);
    assert!(state.capture_target.is_none());
    assert!(!state.context_state.has_content());
    assert!(state.context.text.is_empty());
    assert!(state.context_state.reading_text().is_empty());
    assert!(state.context_state.action.is_none());
    assert_eq!(state.context_state.pending, 0);
    assert_ne!(state.context_state.id, old_context_id);
    assert!(old_cancel.is_cancelled());
}

#[test]
fn paused_visual_analysis_rejects_before_any_service_or_capture_call() {
    let mut state = with_context("source.editor");
    state.paused = true;
    state.begin_visual_analysis();
    assert!(message(&state).contains("paused"));
    assert_eq!(state.context_state.pending, 0);
    assert!(state.services.is_none());
}
#[test]
fn paused_audio_rejects_before_any_service_or_capture_call() {
    let mut state = with_context("source.editor");
    state.paused = true;
    state.begin_audio();
    assert!(message(&state).contains("paused"));
    assert_eq!(state.context_state.pending, 0);
    assert!(state.services.is_none());
}
#[test]
fn excluded_visual_analysis_rejects_before_any_upload() {
    let mut state = with_context("excluded.editor");
    state.settings.exclusions = vec!["EXCLUDED.EDITOR".into()];
    state.begin_visual_analysis();
    assert!(message(&state).contains("excluded"));
    assert_eq!(state.context_state.pending, 0);
}
#[test]
fn excluded_audio_rejects_before_recording() {
    let mut state = with_context("excluded.editor");
    state.settings.exclusions = vec!["EXCLUDED.EDITOR".into()];
    state.begin_audio();
    assert!(message(&state).contains("excluded"));
    assert_eq!(state.context_state.pending, 0);
}
#[test]
fn queued_selection_rechecks_current_exclusions_before_attaching_context() {
    let mut state = AppState::default();
    state.settings.exclusions = vec!["excluded.editor".into()];
    state.accept_capture(source("EXCLUDED.EDITOR"));
    assert!(state.snapshot.is_none());
    assert!(state.capture_target.is_none());
    assert!(!state.context_state.has_content());
    assert!(message(&state).contains("excluded"));
}
#[test]
fn rejected_settings_do_not_replace_active_connection_or_exclusion_policy() {
    let mut state = AppState::default();
    state.settings.local_consent = state.settings.destination();
    let original = state.settings.clone();
    state.local_address = TextEdit::new("https://different.example.com");
    state.local_model = TextEdit::new("x".repeat(201));
    state.exclusions = TextEdit::new("different.editor");
    state.save_settings();
    assert_eq!(state.settings, original);
    assert!(
        state
            .notice
            .as_ref()
            .is_some_and(|notice| !notice.is_empty())
    );
    assert!(state.services.is_none());
}
#[test]
fn credential_save_and_remove_require_the_visible_endpoint_to_be_applied_first() {
    for remove in [false, true] {
        let mut state = AppState::default();
        let original_account = state.settings.credential_account();
        state.local_address = TextEdit::new("https://different.example.com");
        state.save_credential(remove);
        let notice = message(&state);
        assert!(notice.contains("save") && notice.contains("settings"));
        assert_eq!(state.settings.credential_account(), original_account);
        assert!(state.services.is_none());
    }
}
#[test]
fn cancelled_context_identity_prevents_a_late_result_from_matching() {
    let mut state = with_context("source.editor");
    let context_id = state.context_state.id;
    let cancel = state.context_state.cancel.child_token();
    state.cancel();
    assert!(cancel.is_cancelled());
    assert_ne!(context_id, state.context_state.id);
    assert!(state.context_state.action.is_none());
    assert_eq!(state.context_state.pending, 0);
}
#[test]
fn pausing_rejects_a_queued_capture_without_replacing_the_current_source() {
    let mut state = with_context("source.editor");
    state.paused = true;
    let source_id = state.snapshot.as_ref().unwrap().id;
    state.accept_capture(source("different.editor"));
    assert_eq!(state.snapshot.as_ref().unwrap().id, source_id);
    assert_eq!(
        state.capture_target.as_ref().unwrap().app_id,
        "source.editor"
    );
}
#[test]
fn browser_audio_keeps_source_provenance_without_claiming_a_transcript_page() {
    let mut state = AppState::default();
    let mut snapshot = SelectionSnapshot::new("Selected passage", "Browser", "browser");
    snapshot.source_url = Some("https://example.com/video".into());
    state.snapshot = Some(snapshot);
    state.apply_audio_text("A locally transcribed sentence.".into());
    assert!(state.context_state.reference.is_none());
    assert_eq!(
        state.snapshot.as_ref().unwrap().source_url.as_deref(),
        Some("https://example.com/video")
    );
    assert_eq!(
        state.context_state.audio_text,
        "A locally transcribed sentence."
    );
    let context = state.context_state.reading_text();
    assert!(context.contains("no webpage linkage verified"));
    assert!(!context.contains("https://example.com/video"));
}
#[test]
fn audio_from_a_native_editor_remains_labeled_session_text() {
    let mut state = AppState {
        snapshot: Some(SelectionSnapshot::new(
            "Selected passage",
            "Editor",
            "editor",
        )),
        ..AppState::default()
    };
    state.apply_audio_text("A locally transcribed sentence.".into());
    assert!(state.context_state.reference.is_none());
    assert!(
        state
            .context_state
            .reading_text()
            .contains("may contain errors")
    );
    assert!(
        state
            .context_state
            .reading_text()
            .contains("A locally transcribed sentence.")
    );
}
#[test]
fn automatic_history_requires_recording_at_start_and_completion_and_skips_chart_refusals() {
    use crate::controller::should_record_result;
    use pickle_core::{QualityStatus, ReadingAction, ReadingResult};
    let mut result = ReadingResult {
        id: uuid::Uuid::new_v4(),
        action: ReadingAction::Simplify,
        text: "A completed answer".into(),
        chart: None,
        quality: QualityStatus::Checked,
        model: "fixture".into(),
        evaluator_model: None,
        elapsed: 0.0,
        repairs: 0,
        notes: vec![],
        timing: None,
    };
    assert!(should_record_result(&result, true, true));
    assert!(!should_record_result(&result, false, true));
    assert!(!should_record_result(&result, true, false));
    result.action = ReadingAction::Chart;
    result.text = "This selection cannot support a chart".into();
    assert!(!should_record_result(&result, true, true));
}
