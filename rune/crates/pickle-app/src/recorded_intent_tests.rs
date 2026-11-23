//! Application-only recording intent; no provider/vault/native calls.
use super::*;
use rune::{Env, Runtime};

fn runtime() -> Runtime<AppState> {
    let state = AppState {
        snapshot: Some(SelectionSnapshot::new(
            "Cells release energy to work.",
            "Fixture",
            "fixture",
        )),
        panel: Panel::Expanded,
        settings: Settings {
            history_enabled: true,
            ..Settings::default()
        },
        ..AppState::default()
    };
    let mut runtime = Runtime::new(
        state,
        crate::views::view,
        (560.0, 2000.0),
        Env::new(true, 1.0),
    );
    settle(&mut runtime);
    runtime
}
fn settle(runtime: &mut Runtime<AppState>) {
    for _ in 0..60 {
        runtime.frame(1.0 / 60.0);
    }
}
fn consent(runtime: &Runtime<AppState>) -> Pending {
    let Activity::Consent(pending) = &runtime.state.activity else {
        panic!("Expected safe no-service consent")
    };
    pending.clone()
}
fn begin(state: &mut AppState, pending: &Pending) -> (u64, RequestInput) {
    state.revision = state.revision.wrapping_add(1);
    let id = state.revision;
    state.activity = Activity::Running {
        id,
        cancel: CancellationToken::new(),
        progress: "Fixed completion".into(),
    };
    let mut input = RequestInput::new(state.snapshot.clone().unwrap(), pending.action);
    input.question = pending.question.clone();
    input.reference = Some(WebReference {
        url: "https://example.com/accepted".into(),
        title: "Accepted pointer".into(),
        kind: "article".into(),
        text: "REFERENCE_BODY_NOT_ARCHIVED".into(),
        timestamp: None,
    });
    (id, input)
}
fn result(pending: &Pending, answer: &str) -> ReadingResult {
    let mut result = AppState::demonstration_result();
    result.action = pending.action;
    result.text = answer.into();
    result
}
fn accept(runtime: &mut Runtime<AppState>, pending: &Pending, answer: &str) -> (Entry, Entry) {
    let mut entries = None;
    runtime.update(|state, _| {
        let (id, input) = begin(state, pending);
        let result = result(pending, answer);
        let history = state
            .history_entry(id, &input, &result, pending, true)
            .unwrap();
        assert!(state.complete_reading_result_with_reference(
            id,
            pending,
            result,
            reference_pointer(input.reference.as_ref())
        ));
        entries = Some((history, state.current_answer_entry().unwrap()));
    });
    settle(runtime);
    entries.unwrap()
}
fn fields(entry: &Entry) -> (&str, Option<&str>) {
    (&entry.action, entry.question.as_deref())
}

#[test]
fn simplify_explain_adjustment_and_typed_question_reopen_with_legacy_categories() {
    let mut rt = runtime();
    rt.update(|state, tx| state.run_action(ReadingAction::Simplify, tx));
    let base = consent(&rt);
    let mut records = vec![accept(&mut rt, &base, "Cells use energy.")];
    rt.update(|state, tx| state.explain_term(" \tenergy\n ".into(), tx));
    let word = consent(&rt);
    assert_eq!(word.action, ReadingAction::FollowUp);
    assert_eq!(word.label.as_deref(), Some("Explain “energy”"));
    assert!(word.question.starts_with("Explain the meaning of ‘energy’"));
    assert_eq!(
        word.intent(),
        Some(RecordedIntent::Explain("energy".into()))
    );
    // Pending/consent metadata cannot replace the accepted base answer.
    assert_eq!(
        fields(&rt.state.current_answer_entry().unwrap()),
        ("Simplify", None)
    );
    records.push(accept(&mut rt, &word, "Energy lets cells work."));
    let (x, y) = rt.find("shorter").expect("Real adjustment action");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    let shorter = consent(&rt);
    assert_eq!(
        shorter.intent(),
        Some(RecordedIntent::Adjustment("Shorter".into()))
    );
    assert_eq!(shorter.label.as_deref(), Some("Shorter"));
    assert!(shorter.question.starts_with("Make the explanation shorter"));
    records.push(accept(&mut rt, &shorter, "Cells need energy."));
    // A literal question resembling an Explain label remains a normal question.
    let question = "  Explain “energy” — why?  ";
    rt.update(|state, tx| state.follow_up(question.into(), None, tx));
    let typed = consent(&rt);
    records.push(accept(&mut rt, &typed, "Energy powers cell activity."));
    let expected = [
        ("Simplify", None),
        ("Explain", Some("energy")),
        ("Adjustment", Some("Shorter")),
        ("Follow-up", Some(question)),
    ];
    for ((history, saved), expected) in records.iter().zip(expected) {
        assert_eq!(fields(history), expected);
        assert_eq!(fields(saved), expected);
        assert_eq!(history.reference, saved.reference);
        assert_eq!(history.passage, "Cells release energy to work.");
    }
    let wire = serde_json::to_value(rt.state.conversation.last().unwrap()).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 4);
    assert_eq!(wire["question"], question);
    assert!(wire.get("recorded_intent").is_none());
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::new(dir.path().into()).unwrap();
    for (history, saved) in &records {
        repo.append(false, history.clone()).unwrap();
        repo.append(true, saved.clone()).unwrap();
    }
    drop(repo);
    let reopened = Repository::new(dir.path().into()).unwrap();
    for entries in [reopened.history().unwrap(), reopened.bookmarks().unwrap()] {
        assert_eq!(
            entries.iter().map(fields).collect::<Vec<_>>(),
            expected.into_iter().rev().collect::<Vec<_>>()
        );
    }
    for archive in ["history.json", "bookmarks.json"] {
        let bytes = std::fs::read_to_string(dir.path().join(archive)).unwrap();
        assert!(!bytes.contains("Explain the meaning of"));
        assert!(!bytes.contains("Make the explanation shorter"));
        assert!(!bytes.contains("REFERENCE_BODY_NOT_ARCHIVED"));
    }
    assert!(rt.state.services.is_none());
}

#[test]
fn only_accepted_latest_intent_survives_pending_failed_cancelled_and_stale_work() {
    let mut rt = runtime();
    let base = Pending::action(ReadingAction::Simplify);
    accept(&mut rt, &base, "Base energy answer.");
    rt.update(|state, tx| state.explain_term("energy".into(), tx));
    let word = consent(&rt);
    accept(&mut rt, &word, "Accepted energy meaning.");
    let accepted = rt.state.current_answer_entry().unwrap();
    rt.update(|state, tx| state.adjust("Other instruction".into(), "More detail".into(), tx));
    let pending = consent(&rt);
    assert_eq!(
        fields(&rt.state.current_answer_entry().unwrap()),
        fields(&accepted)
    );
    rt.update(|state, _| {
        let (id, input) = begin(state, &pending);
        state.cancel();
        state.last = Some(pending.clone());
        state.activity = Activity::Failed("Controlled failure".into());
        assert!(
            state
                .history_entry(id, &input, &result(&pending, "Late"), &pending, true)
                .is_none()
        );
        assert!(!state.complete_reading_result(id, &pending, result(&pending, "Late")));
        let saved = state.current_answer_entry().unwrap();
        assert_eq!(fields(&saved), fields(&accepted));
        assert_eq!(saved.answer, accepted.answer);
        assert_eq!(saved.reference, accepted.reference);
        state.clear_follow_ups();
        assert!(state.answer_references.follow_up_intent.is_none());
        assert_eq!(
            fields(&state.current_answer_entry().unwrap()),
            ("Simplify", None)
        );
    });
}

#[test]
fn prebase_explanation_eviction_new_base_and_new_source_cannot_leak_intent() {
    let mut rt = runtime();
    rt.update(|state, tx| state.explain_term("energy".into(), tx));
    let word = consent(&rt);
    let (_, saved) = accept(&mut rt, &word, &"A".repeat(9000));
    assert!(rt.state.result.is_none());
    assert_eq!(fields(&saved), ("Explain", Some("energy")));
    rt.update(|state, tx| state.adjust("Shorter instruction".into(), "Shorter".into(), tx));
    let adjustment = consent(&rt);
    accept(&mut rt, &adjustment, &"B".repeat(9000));
    assert_eq!(rt.state.conversation.len(), 1);
    assert_eq!(
        fields(&rt.state.current_answer_entry().unwrap()),
        ("Adjustment", Some("Shorter"))
    );
    rt.update(|state, _| {
        state.clear_follow_ups();
        assert!(state.current_answer_entry().is_none());
    });
    accept(&mut rt, &word, "Another term answer.");
    accept(
        &mut rt,
        &Pending::action(ReadingAction::Expand),
        "New base answer.",
    );
    assert!(rt.state.answer_references.follow_up_intent.is_none());
    assert_eq!(
        fields(&rt.state.current_answer_entry().unwrap()),
        ("Explain more", None)
    );
    accept(&mut rt, &word, "Discard on new source.");
    rt.update(|state, _| {
        state.source = TextEdit::new("Different source.");
        state.commit_source();
    });
    assert!(rt.state.current_answer_entry().is_none());
    assert!(rt.state.answer_references.follow_up_intent.is_none());
    accept(
        &mut rt,
        &Pending::action(ReadingAction::Simplify),
        "Different base.",
    );
    assert_eq!(
        fields(&rt.state.current_answer_entry().unwrap()),
        ("Simplify", None)
    );
    rt.update(|state, _| state.reset());
    assert!(rt.state.answer_references.follow_up_intent.is_none());
}

#[test]
fn automatic_history_requires_start_and_completion_opt_in_and_a_current_attempt() {
    let mut rt = runtime();
    let pending = Pending {
        question: "Internal term instruction".into(),
        recorded_intent: Some(RecordedIntent::Explain("energy".into())),
        ..Pending::action(ReadingAction::FollowUp)
    };
    rt.update(|state, _| {
        let (id, input) = begin(state, &pending);
        let result = result(&pending, "Accepted answer.");
        assert_eq!(
            fields(
                &state
                    .history_entry(id, &input, &result, &pending, true)
                    .unwrap()
            ),
            ("Explain", Some("energy"))
        );
        assert!(
            state
                .history_entry(id, &input, &result, &pending, false)
                .is_none()
        );
        state.settings.history_enabled = false;
        assert!(
            state
                .history_entry(id, &input, &result, &pending, true)
                .is_none()
        );
        state.settings.history_enabled = true;
        assert!(
            state
                .history_entry(id.wrapping_add(1), &input, &result, &pending, true)
                .is_none()
        );
        // Exercise the actual pause transition, which invalidates the request;
        // an unrelated appearance-save flag alone must not change policy.
        state.toggle_pause();
        assert!(state.paused);
        assert!(
            state
                .history_entry(id, &input, &result, &pending, true)
                .is_none()
        );
        state.toggle_pause();
        state.cancel();
        assert!(
            state
                .history_entry(id, &input, &result, &pending, true)
                .is_none()
        );
    });
}
