//! Real reader layout, scroll requests and pointer wheel events; no services or native windows.
use crate::{
    controller::{Activity, AppState, Panel, Pending, READER_SCROLL, READER_STATUS, Route},
    views,
};
use pickle_core::{ReadingAction, ReadingResult};
use rune::{Env, Key, KeyEvent, Mods, Runtime, TextEdit};
use tokio_util::sync::CancellationToken;

fn settle(rt: &mut Runtime<AppState>) {
    for _ in 0..180 {
        rt.frame(1.0 / 60.0);
    }
}
fn reader() -> Runtime<AppState> {
    let mut state = AppState::default();
    state.sample_result();
    state.demo = false;
    state.result.as_mut().unwrap().text = "A fixed base paragraph for the reader.\n\n".repeat(45);
    let mut rt = Runtime::new(state, views::view, (560.0, 640.0), Env::new(true, 1.0));
    rt.set_wheel_glide(false);
    settle(&mut rt);
    rt
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    settle(rt);
    let (x, y) = rt.find(key).unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}
fn source_top(rt: &Runtime<AppState>) -> f32 {
    rt.presented("source-preview").unwrap().0[1]
}
fn arm(rt: &mut Runtime<AppState>) {
    rt.update(|state, tx| {
        // An accepted explicit action can wait for captured context without
        // constructing a provider/runtime. Its viewport follows the same path.
        state.context_state.pending = 1;
        state.follow_up("Explain this reading further.".into(), None, tx);
    });
    settle(rt);
    assert!(rt.state.reader_following);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(true));
}
fn draft(rt: &mut Runtime<AppState>, paragraphs: usize) {
    rt.update(|state, _| {
        state.context_state.pending = 0;
        state.context_state.action = None;
        state.activity = Activity::Running {
            id: state.revision,
            cancel: CancellationToken::new(),
            progress: "Writing".into(),
        };
        state.draft = "A newly arriving fixed explanation.\n\n".repeat(paragraphs);
    });
    settle(rt);
}
fn complete(rt: &mut Runtime<AppState>) {
    rt.update(|state, _| {
        let mut result: ReadingResult = state.result.clone().unwrap();
        result.action = ReadingAction::FollowUp;
        result.text = state.draft.clone();
        let pending = state.last.clone().unwrap();
        assert!(state.complete_reading_result(state.revision, &pending, result));
    });
    settle(rt);
}
fn away(rt: &mut Runtime<AppState>) {
    let rect = rt.presented(READER_SCROLL).unwrap().0;
    rt.wheel(rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5, 750.0);
    settle(rt);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(false));
}

#[test]
fn explicit_reading_follows_long_draft_growth_and_the_final_turn() {
    let mut rt = reader();
    assert!(!rt.state.reader_following);
    assert!(source_top(&rt) >= 49.0);
    arm(&mut rt);
    for paragraphs in [10, 35, 65] {
        draft(&mut rt, paragraphs);
        assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(true));
    }
    complete(&mut rt);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(true));
    let (rect, _) = rt.presented("follow-up").unwrap();
    assert!(
        rect[1] >= 49.0 && rect[1] + rect[3] <= 640.0,
        "Composer is not visible: {rect:?}"
    );
    for key in ["pin-reader", "open-settings", "close"] {
        let (rect, _) = rt.presented(key).unwrap();
        assert!(rect[1] >= 0.0 && rect[1] + rect[3] <= 48.0);
    }
    assert!(rt.state.services.is_none());
}

#[test]
fn manual_scroll_away_survives_growth_and_completion_until_a_new_explicit_request() {
    let mut rt = reader();
    arm(&mut rt);
    draft(&mut rt, 20);
    away(&mut rt);
    let top = source_top(&rt);
    draft(&mut rt, 55);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(false));
    assert!((source_top(&rt) - top).abs() < 1.0);
    complete(&mut rt);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(false));
    assert!((source_top(&rt) - top).abs() < 1.0);
    arm(&mut rt);
    draft(&mut rt, 70);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(true));
}

#[test]
fn submitted_follow_up_does_not_restore_the_caret_over_a_manual_scroll_away() {
    let mut rt = reader();
    rt.update(|state, tx| {
        state.context_state.pending = 1;
        tx.scroll_to_bottom_instant(READER_SCROLL);
    });
    click(&mut rt, "follow-up");
    rt.paste("A real typed follow-up question.");
    rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    settle(&mut rt);
    assert!(matches!(rt.state.activity, Activity::WaitingContext));
    assert!(rt.state.reader_following);
    assert_eq!(
        rt.state.last.as_ref().unwrap().question,
        "A real typed follow-up question."
    );
    draft(&mut rt, 35);
    away(&mut rt);
    let top = source_top(&rt);
    complete(&mut rt);
    assert!(
        rt.find("follow-up").is_some(),
        "The composer must return after completion"
    );
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(false));
    assert!(
        (source_top(&rt) - top).abs() < 1.0,
        "The returning composer/caret moved the reader"
    );
    assert!(rt.state.services.is_none());
}

#[test]
fn explicit_online_jev_consent_reveals_agree_at_the_minimum_reader_width() {
    let mut rt = reader();
    rt.set_viewport((480.0, 640.0), 1.0);
    rt.update(|state, _| {
        state.settings.mode = "Online".into();
        state.settings.jev_enabled = true;
        state.settings.cloud_consent = false;
        state.settings.jev_consent = false;
    });
    settle(&mut rt);
    let snapshot = rt.state.snapshot.clone();
    let result = serde_json::to_value(&rt.state.result).unwrap();
    rt.update(|state, tx| {
        state.follow_up("An explicit Online follow-up question.".into(), None, tx)
    });
    settle(&mut rt);
    assert!(matches!(rt.state.activity, Activity::Consent(_)));
    assert!(!rt.state.reader_following);
    let (viewport, _) = rt.presented(READER_SCROLL).unwrap();
    let (agree, _) = rt.presented("consent-continue").unwrap();
    assert!(
        agree[0] >= viewport[0]
            && agree[0] + agree[2] <= viewport[0] + viewport[2]
            && agree[1] >= viewport[1]
            && agree[1] + agree[3] <= viewport[1] + viewport[3],
        "Online/Jev Agree must be fully reachable: viewport={viewport:?}, agree={agree:?}"
    );
    assert_eq!(rt.state.snapshot, snapshot);
    assert_eq!(serde_json::to_value(&rt.state.result).unwrap(), result);
    assert!(!rt.state.settings.cloud_consent && !rt.state.settings.jev_consent);
    assert!(rt.state.services.is_none());
}

#[test]
fn consent_is_revealed_and_async_context_outcomes_do_not_rearm_an_away_reader() {
    let mut rt = reader();
    rt.update(|state, tx| state.follow_up("A question needing consent.".into(), None, tx));
    settle(&mut rt);
    assert!(matches!(rt.state.activity, Activity::Consent(_)));
    assert!(!rt.state.reader_following);
    let (status, _) = rt.presented(READER_STATUS).unwrap();
    assert!(status[1] < 640.0 && status[1] + status[3] > 49.0);
    let (agree, _) = rt.presented("consent-continue").unwrap();
    assert!(
        agree[1] >= 49.0 && agree[1] + agree[3] <= 640.0,
        "Consent action is offscreen: {agree:?}"
    );
    rt.update(|state, tx| {
        state.context_state.pending = 1;
        state.consent(tx);
    });
    settle(&mut rt);
    assert!(rt.state.reader_following);
    draft(&mut rt, 25);
    away(&mut rt);
    rt.update(|state, tx| {
        state.activity = Activity::WaitingContext;
        state.context_state.action = Some(Pending::action(ReadingAction::FollowUp));
        state.settings.local_consent.clear();
        state.context_ready(tx);
    });
    settle(&mut rt);
    assert!(matches!(rt.state.activity, Activity::Consent(_)));
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(false));
    // An internal policy/context continuation uses start, never the user kick.
    rt.update(|state, tx| state.start(Pending::action(ReadingAction::Simplify), tx));
    settle(&mut rt);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(false));
    rt.update(|state, tx| {
        state.paused = true;
        state.run_action(ReadingAction::Expand, tx);
    });
    settle(&mut rt);
    assert!(matches!(rt.state.activity, Activity::Failed(_)));
    assert!(!rt.state.reader_following);
    let (retry, _) = rt.presented("retry-error").unwrap();
    assert!(
        retry[1] >= 49.0 && retry[1] + retry[3] <= 640.0,
        "Error recovery is offscreen: {retry:?}"
    );
    rt.update(|state, _| {
        state.paused = false;
        state.context_state.pending = 1;
    });
    click(&mut rt, "retry-error");
    assert!(rt.state.reader_following);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(true));
    assert!(rt.state.services.is_none());
}

#[test]
fn disclosure_routes_and_reset_stop_following_without_replaying_a_scroll() {
    let mut rt = reader();
    arm(&mut rt);
    draft(&mut rt, 30);
    rt.update(|_, tx| tx.scroll_to_y(READER_SCROLL, 0.0));
    settle(&mut rt);
    click(&mut rt, "toggle-full-source");
    assert!(!rt.state.reader_following);
    let top = source_top(&rt);
    draft(&mut rt, 60);
    assert!((source_top(&rt) - top).abs() < 1.0);
    click(&mut rt, "toggle-full-source");
    assert!(!rt.state.reader_following);
    assert!(source_top(&rt) >= 49.0);
    rt.update(|state, _| state.activity = Activity::Idle);
    rt.update(|state, tx| {
        state.context_state.pending = 1;
        state.follow_up("Start another reading.".into(), None, tx);
    });
    rt.frame(1.0 / 120.0);
    // Leave during a newly requested glide; a missing reader destination must
    // not replay against Settings/library or when the reader is mounted again.
    let (x, y) = rt.find("open-settings").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(!rt.state.reader_following && rt.state.route == Route::Settings);
    assert!(rt.find(READER_SCROLL).is_none());
    click(&mut rt, "back-reader");
    assert!(!rt.state.reader_following);
    click(&mut rt, "open-saved");
    assert!(!rt.state.reader_following && rt.state.route == Route::Library(true));
    assert!(rt.find(READER_SCROLL).is_none());
    click(&mut rt, "library-back");
    rt.update(|state, _| state.reset());
    settle(&mut rt);
    assert!(!rt.state.reader_following && rt.state.panel == Panel::Compose);
    assert!(rt.find(READER_SCROLL).is_none());
    let (input, _) = rt.presented("passage-input").unwrap();
    assert!(input[1] >= 49.0 && input[1] < 640.0);
}

#[test]
fn source_context_precedes_arriving_answers_and_its_disclosure_stops_following() {
    let mut rt = reader();
    let text = "Fixed reference context. ".repeat(85);
    rt.update(|state, _| {
        state.context_state.reference = Some(pickle_core::WebReference {
            url: "https://example.invalid/reference".into(),
            title: "Fixture".into(),
            kind: "article".into(),
            text: text.clone(),
            timestamp: None,
        })
    });
    arm(&mut rt);
    draft(&mut rt, 35);
    assert_eq!(rt.scroll_at_bottom(READER_SCROLL), Some(true));
    let (context, _) = rt.presented("session-context").unwrap();
    let (arrival, _) = rt.presented("answer-draft").unwrap();
    assert!(
        context[1] + context[3] <= arrival[1],
        "The live end chased a context footer instead of the answer"
    );
    let snapshot = rt.state.snapshot.clone();
    rt.update(|_, tx| tx.scroll_key_to("session-context", 0.0));
    settle(&mut rt);
    click(&mut rt, "preview-full-context");
    assert!(!rt.state.reader_following && rt.state.context_state.preview_full);
    let (control, _) = rt.presented("preview-full-context").unwrap();
    assert!(control[1] >= 49.0 && control[1] + control[3] <= 640.0);
    draft(&mut rt, 65);
    let top = rt.presented("session-context").unwrap().0[1];
    click(&mut rt, "preview-full-context");
    assert!(!rt.state.reader_following && !rt.state.context_state.preview_full);
    assert!((rt.presented("session-context").unwrap().0[1] - top).abs() < 1.0);
    assert_eq!(rt.state.snapshot, snapshot);
    assert!(rt.state.services.is_none());
}

#[test]
fn new_source_and_automatic_preview_start_at_the_source_not_the_old_tail() {
    for preview in [false, true] {
        let mut rt = reader();
        arm(&mut rt);
        draft(&mut rt, 35);
        let old_id = rt.state.snapshot.as_ref().unwrap().id;
        // Release the fake in-flight request before a source change is accepted.
        rt.update(|state, _| state.activity = Activity::Idle);
        rt.update(|state, _| {
            if preview {
                state.settings.automatic_menu = true;
                assert!(state.show_auto_capture(pickle_platform::CapturedSelection {
                    text: "A newly visible source.".into(),
                    app_name: "Editor".into(),
                    app_id: "editor".into(),
                    bounds: None,
                    source_url: None,
                    source_pid: 0,
                    source_window: None,
                }));
            } else {
                state.source = TextEdit::new("A newly pasted source.");
                state.commit_source();
            }
        });
        settle(&mut rt);
        assert!(!rt.state.reader_following);
        assert_ne!(rt.state.display_snapshot().unwrap().id, old_id);
        let (source, _) = rt.presented("source-preview").unwrap();
        assert!(
            source[1] >= 49.0 && source[1] + source[3] <= 640.0,
            "New source inherited a tail offset: {source:?}"
        );
    }
}
