//! Deferred request reducers exercised without archives, providers or a window.
use super::*;
use rune::{Env, Runtime};

fn settle(runtime: &mut Runtime<AppState>) {
    for _ in 0..120 {
        runtime.frame(1.0 / 60.0);
    }
}

fn waiting() -> Runtime<AppState> {
    let mut state = AppState::default();
    state.sample_result();
    state.demo = false;
    let mut runtime = Runtime::new(
        state,
        crate::views::view,
        (560.0, 640.0),
        Env::new(true, 1.0),
    );
    runtime.update(|state, tx| {
        state.context_state.pending = 1;
        state.follow_up("Explain the controlled passage.".into(), None, tx);
    });
    settle(&mut runtime);
    assert!(matches!(runtime.state.activity, Activity::WaitingContext));
    runtime
}

fn click(runtime: &mut Runtime<AppState>, key: &str) {
    let (x, y) = runtime.find(key).unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    settle(runtime);
}

#[test]
fn late_context_consent_preserves_header_settings_and_saved_navigation() {
    for (key, route) in [
        ("open-settings", Route::Settings),
        ("open-saved", Route::Library(true)),
    ] {
        let mut runtime = waiting();
        let source = runtime.state.snapshot.clone();
        let answer = serde_json::to_value(&runtime.state.result).unwrap();
        click(&mut runtime, key);
        assert_eq!(runtime.state.route, route);
        assert!(!runtime.state.reader_following);
        runtime.update(|state, tx| {
            state.context_state.pending = 0;
            state.context_ready(tx);
        });
        settle(&mut runtime);
        assert_eq!(runtime.state.route, route);
        assert_eq!(runtime.state.panel, Panel::Expanded);
        assert!(matches!(runtime.state.activity, Activity::Consent(_)));
        assert!(!runtime.state.reader_following);
        assert_eq!(runtime.state.snapshot, source);
        assert_eq!(serde_json::to_value(&runtime.state.result).unwrap(), answer);
        assert!(runtime.state.services.is_none());
    }
}

#[test]
fn native_menu_equivalent_navigation_survives_context_and_result_completion() {
    for route in [Route::Settings, Route::Library(false)] {
        let mut runtime = waiting();
        let source = runtime.state.snapshot.clone();
        runtime.update(|state, _| {
            // The same state transition used by native Settings / History.
            state.dismiss_auto_preview();
            state.stop_reader_follow();
            state.route = route;
        });
        runtime.update(|state, tx| {
            state.context_state.pending = 0;
            state.context_ready(tx);
        });
        assert!(matches!(runtime.state.activity, Activity::Consent(_)));
        assert_eq!(runtime.state.route, route);
        // Exercise the production successful-result reducer independently of
        // transport: no services, credentials or actual consent continuation.
        runtime.update(|state, _| {
            let token = CancellationToken::new();
            state.activity = Activity::Running {
                id: state.revision,
                cancel: token.clone(),
                progress: "Controlled completion".into(),
            };
            let mut answer = state.result.clone().unwrap();
            answer.action = ReadingAction::FollowUp;
            answer.text = "A fixed completed explanation.".into();
            assert!(state.complete_reading_result(
                state.revision,
                &state.last.clone().unwrap(),
                answer
            ));
            assert!(token.is_cancelled());
        });
        settle(&mut runtime);
        assert_eq!(runtime.state.route, route);
        assert_eq!(runtime.state.snapshot, source);
        assert!(!runtime.state.reader_following);
        assert_eq!(
            runtime.state.conversation.last().unwrap().answer,
            "A fixed completed explanation."
        );
    }
}

#[test]
fn fixed_demo_context_success_does_not_reset_navigation_or_source() {
    let mut runtime = waiting();
    let source = runtime.state.snapshot.clone();
    runtime.update(|state, tx| {
        state.stop_reader_follow();
        state.route = Route::Settings;
        state.demo = true;
        state.context_state.pending = 0;
        state.context_ready(tx);
    });
    assert!(matches!(runtime.state.activity, Activity::Idle));
    assert_eq!(runtime.state.route, Route::Settings);
    assert_eq!(runtime.state.snapshot, source);
    assert!(!runtime.state.reader_following);
}

#[test]
fn pending_settings_retain_context_action_until_the_write_finishes() {
    let mut runtime = waiting();
    runtime.update(|state, tx| {
        state.settings_pending = true;
        state.stop_reader_follow();
        state.route = Route::Settings;
        state.context_state.pending = 0;
        state.context_ready(tx);
    });
    assert!(matches!(runtime.state.activity, Activity::WaitingContext));
    assert!(runtime.state.context_state.action.is_some());
    runtime.update(|state, tx| {
        // A failed write retains the previous policy and invokes context_ready
        // after clearing its pending flag, as queue_settings completion does.
        state.settings_pending = false;
        state.notice = Some("Controlled save failure; original policy retained.".into());
        state.context_ready(tx);
    });
    assert!(matches!(runtime.state.activity, Activity::Consent(_)));
    assert!(runtime.state.context_state.action.is_none());
    assert_eq!(runtime.state.route, Route::Settings);
    assert!(!runtime.state.reader_following);

    let mut cancelled = waiting();
    cancelled.update(|state, tx| {
        state.settings_pending = true;
        state.context_state.pending = 0;
        state.context_ready(tx);
        // A successful policy commit deliberately cancels the old request.
        state.cancel();
        state.settings_pending = false;
        state.context_ready(tx);
    });
    assert!(matches!(cancelled.state.activity, Activity::Idle));
    assert!(cancelled.state.context_state.action.is_none());
}

#[test]
fn cancelled_context_and_later_running_requests_cannot_restart_old_work() {
    let mut runtime = waiting();
    let old_context = runtime.state.context_state.cancel.clone();
    runtime.update(|state, tx| {
        state.cancel();
        state.route = Route::Settings;
        state.context_ready(tx);
    });
    assert!(old_context.is_cancelled());
    assert!(matches!(runtime.state.activity, Activity::Idle));
    assert_eq!(runtime.state.route, Route::Settings);
    runtime.update(|state, tx| {
        state.context_state.action = Some(Pending::action(ReadingAction::Simplify));
        let token = CancellationToken::new();
        state.activity = Activity::Running {
            id: state.revision,
            cancel: token.clone(),
            progress: "New request".into(),
        };
        state.context_ready(tx);
        assert!(!token.is_cancelled());
    });
    assert!(matches!(runtime.state.activity, Activity::Running { .. }));
    assert_eq!(runtime.state.route, Route::Settings);
}

#[test]
fn context_continuation_never_commits_a_newer_automatic_preview() {
    let mut runtime = waiting();
    runtime.update(|state, _| state.cancel());
    let source = runtime.state.snapshot.clone();
    runtime.update(|state, _| {
        state.settings.automatic_menu = true;
        assert!(state.show_auto_capture(pickle_platform::CapturedSelection {
            text: "A distinct newer preview passage.".into(),
            app_name: "Fixture".into(),
            app_id: "fixture.editor".into(),
            source_pid: 0,
            source_window: None,
            source_url: None,
            bounds: None,
        }));
        // Defensively emulate a late callback against the prior queued action.
        // Native capture normally rejects a preview while context is pending.
        state.context_state.action = Some(Pending::action(ReadingAction::Simplify));
        state.activity = Activity::WaitingContext;
        state.route = Route::Library(false);
    });
    let preview = runtime.state.display_snapshot().unwrap().id;
    runtime.update(|state, tx| state.context_ready(tx));
    assert!(matches!(runtime.state.activity, Activity::Consent(_)));
    assert_eq!(runtime.state.route, Route::Library(false));
    assert_eq!(runtime.state.panel, Panel::Choosing);
    assert_eq!(runtime.state.snapshot, source);
    assert_eq!(runtime.state.display_snapshot().unwrap().id, preview);
    assert!(runtime.state.auto_preview.is_some());
}

#[test]
fn explicit_request_still_activates_reader_and_checks_current_policy() {
    for paused in [false, true] {
        let mut runtime = waiting();
        runtime.update(|state, tx| {
            state.cancel();
            state.route = Route::Settings;
            state.panel = Panel::Choosing;
            state.paused = paused;
            state.run_action(ReadingAction::Simplify, tx);
        });
        assert_eq!(runtime.state.route, Route::Reader);
        assert_eq!(runtime.state.panel, Panel::Expanded);
        if paused {
            assert!(
                matches!(&runtime.state.activity, Activity::Failed(message) if message.contains("paused"))
            );
        } else {
            assert!(matches!(runtime.state.activity, Activity::Consent(_)));
        }
    }
}

#[test]
fn activity_and_model_teardown_cancel_only_owned_work() {
    let old = CancellationToken::new();
    let new = CancellationToken::new();
    let mut state = AppState {
        activity: Activity::Running {
            id: 1,
            cancel: old.clone(),
            progress: String::new(),
        },
        ..AppState::default()
    };
    state.activity = Activity::Running {
        id: 2,
        cancel: new.clone(),
        progress: String::new(),
    };
    assert!(old.is_cancelled());
    assert!(!new.is_cancelled());
    let context = state.context_state.cancel.clone();
    drop(state);
    assert!(new.is_cancelled());
    assert!(context.is_cancelled());
}
