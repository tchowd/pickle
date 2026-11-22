use crate::{
    controller::{Activity, AppState, Panel, Route},
    views,
};
use rune::{Env, Runtime};

fn runtime() -> Runtime<AppState> {
    Runtime::new(
        AppState::default(),
        views::view,
        (560.0, 720.0),
        Env::new(true, 1.0),
    )
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("Missing actionable element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
}
#[test]
fn paste_action_requires_a_real_bounded_passage() {
    let mut rt = runtime();
    click(&mut rt, "use-passage");
    assert!(rt.state.snapshot.is_none());
    assert!(matches!(rt.state.activity, Activity::Failed(_)));
    rt.state.source = rune::TextEdit::new("A real selected passage.");
    click(&mut rt, "use-passage");
    assert_eq!(
        rt.state.snapshot.as_ref().unwrap().text,
        "A real selected passage."
    );
    assert!(rt.state.result.is_none());
    assert_eq!(rt.state.panel, Panel::Choosing);
}
#[test]
fn sample_can_be_read_without_services_credentials_or_network() {
    let mut rt = runtime();
    click(&mut rt, "sample");
    assert!(rt.state.demo);
    assert!(rt.state.result.is_none());
    click(&mut rt, "simplify");
    assert!(
        rt.state
            .result
            .as_ref()
            .unwrap()
            .model
            .contains("demonstration")
    );
    assert!(rt.state.entries.is_empty());
    assert!(rt.state.services.is_none());
}
#[test]
fn live_action_cannot_bypass_destination_consent() {
    let mut rt = runtime();
    rt.state.source = rune::TextEdit::new("This may help some people.");
    rt.state.commit_source();
    click(&mut rt, "simplify");
    assert!(matches!(rt.state.activity, Activity::Consent(_)));
    assert!(rt.state.result.is_none());
    assert!(rt.find("consent-continue").is_some());
}
#[test]
fn reset_cancels_the_attempt_and_clears_source_context() {
    let mut state = AppState::default();
    state.sample_result();
    state.context = rune::TextEdit::new("Sensitive surrounding text.");
    let cancel = tokio_util::sync::CancellationToken::new();
    state.activity = Activity::Running {
        id: state.revision,
        cancel: cancel.clone(),
        progress: "Writing".into(),
    };
    state.draft = "An incomplete answer".into();
    state.reset();
    assert!(cancel.is_cancelled());
    assert!(state.snapshot.is_none());
    assert!(state.result.is_none());
    assert!(state.context.text.is_empty());
    assert!(state.draft.is_empty());
    assert!(!state.demo);
}
#[test]
fn online_is_blocked_in_strict_offline_mode_before_any_services() {
    let mut rt = runtime();
    rt.state.source = rune::TextEdit::new("A passage.");
    rt.state.commit_source();
    rt.state.settings.mode = "Online".into();
    rt.state.settings.strict_offline = true;
    click(&mut rt, "simplify");
    assert!(matches!(&rt.state.activity, Activity::Failed(message) if message.contains("Offline")));
}
#[test]
fn saved_answers_screen_never_repopulates_the_source() {
    let mut rt = runtime();
    rt.state.sample_result();
    let source_id = rt.state.snapshot.as_ref().unwrap().id;
    click(&mut rt, "open-saved");
    assert!(rt.state.route == Route::Library(true));
    assert_eq!(rt.state.snapshot.as_ref().unwrap().id, source_id);
    assert!(rt.state.services.is_none());
}
