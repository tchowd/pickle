//! Accepted-answer provenance without native capture, providers, or user archives.
use super::*;

fn reference(name: &str) -> WebReference {
    WebReference {
        url: format!("https://example.com/{name}"),
        title: format!("Controlled {name}"),
        kind: "video transcript".into(),
        text: "REFERENCE_BODY_MUST_NOT_BE_ARCHIVED".into(),
        timestamp: Some(12.0),
    }
}

fn state() -> AppState {
    let mut state = AppState::default();
    state.sample();
    state.demo = false;
    state.settings.history_enabled = false;
    state
}

fn begin(state: &mut AppState, pending: &Pending) -> (u64, RequestInput) {
    state.revision = state.revision.wrapping_add(1);
    let id = state.revision;
    state.activity = Activity::Running {
        id,
        cancel: CancellationToken::new(),
        progress: "Controlled request".into(),
    };
    let mut input = RequestInput::new(state.snapshot.clone().unwrap(), pending.action);
    input.reference = state.context_state.reference.clone();
    (id, input)
}

fn complete(
    state: &mut AppState,
    id: u64,
    pending: &Pending,
    input: &RequestInput,
    text: &str,
) -> bool {
    let mut result = AppState::demonstration_result();
    result.action = pending.action;
    result.text = text.into();
    state.complete_reading_result_with_reference(
        id,
        pending,
        result,
        reference_pointer(input.reference.as_ref()),
    )
}

fn accept(state: &mut AppState, pending: &Pending, name: &str, text: &str) {
    state.context_state.reference = Some(reference(name));
    let (id, input) = begin(state, pending);
    assert!(complete(state, id, pending, &input, text));
}

fn follow_up() -> Pending {
    Pending {
        question: "Explain the controlled term.".into(),
        label: Some("Explain word".into()),
        ..Pending::action(ReadingAction::FollowUp)
    }
}

#[test]
fn saved_follow_up_uses_visible_label_like_history_and_preserves_typed_questions() {
    for (question, label, expected) in [
        (
            "INTERNAL_ADJUSTMENT_PROMPT_DO_NOT_ARCHIVE",
            Some("Shorter"),
            "Shorter",
        ),
        (
            "INTERNAL_WORD_PROMPT_DO_NOT_ARCHIVE",
            Some("Explain Café"),
            "Explain Café",
        ),
        (
            "What does Café mean in this passage?",
            None,
            "What does Café mean in this passage?",
        ),
    ] {
        let mut state = state();
        let pending = Pending {
            question: question.into(),
            label: label.map(str::to_owned),
            ..Pending::action(ReadingAction::FollowUp)
        };
        accept(
            &mut state,
            &pending,
            "accepted",
            "Controlled accepted answer.",
        );
        let saved = state.current_answer_entry().unwrap();
        let mut result = AppState::demonstration_result();
        result.action = pending.action;
        result.text = saved.answer.clone();
        let history = make_entry(state.snapshot.as_ref().unwrap(), &result, &pending);
        assert_eq!(saved.question.as_deref(), Some(expected));
        assert_eq!(saved.question, history.question);

        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().join("owned-archive")).unwrap();
        repo.append(true, saved.clone()).unwrap();
        repo.append(false, history.clone()).unwrap();
        assert_eq!(repo.bookmarks().unwrap(), vec![saved]);
        assert_eq!(repo.history().unwrap(), vec![history]);
        for archive in ["bookmarks.json", "history.json"] {
            let bytes = std::fs::read_to_string(repo.directory().join(archive)).unwrap();
            assert!(!bytes.contains("INTERNAL_ADJUSTMENT_PROMPT_DO_NOT_ARCHIVE"));
            assert!(!bytes.contains("INTERNAL_WORD_PROMPT_DO_NOT_ARCHIVE"));
            assert!(bytes.contains(expected));
        }
    }
}

#[test]
fn history_off_save_keeps_request_pointer_after_late_reference_and_context_clear() {
    let mut state = state();
    let pending = Pending::action(ReadingAction::Simplify);
    state.context_state.reference = Some(reference("accepted"));
    let (id, input) = begin(&mut state, &pending);
    // A newer source-audio/context callback does not describe this request.
    state.context_state.reference = Some(reference("late-audio"));
    assert!(complete(
        &mut state,
        id,
        &pending,
        &input,
        "Accepted base answer."
    ));
    let entry = state.current_answer_entry().unwrap();
    assert_eq!(entry.reference, reference_pointer(input.reference.as_ref()));
    state.clear_context();
    assert!(state.context_state.reference.is_none());
    let saved = state.current_answer_entry().unwrap();
    assert_eq!(saved.reference, entry.reference);
    assert_eq!(saved.answer, entry.answer);
    assert!(!state.settings.history_enabled);
    assert!(state.entries.is_empty());

    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::new(directory.path().join("owned-archive")).unwrap();
    repo.append(true, saved.clone()).unwrap();
    assert_eq!(repo.bookmarks().unwrap(), vec![saved]);
    assert!(repo.history().unwrap().is_empty());
    let bytes = std::fs::read_to_string(repo.directory().join("bookmarks.json")).unwrap();
    assert!(!bytes.contains("REFERENCE_BODY_MUST_NOT_BE_ARCHIVED"));
    assert!(!bytes.contains("late-audio"));
}

#[test]
fn latest_follow_up_has_own_pointer_and_clear_restores_base() {
    let mut state = state();
    accept(
        &mut state,
        &Pending::action(ReadingAction::Simplify),
        "base",
        "Base answer.",
    );
    let base = state.current_answer_entry().unwrap();
    accept(&mut state, &follow_up(), "follow-up", "Follow-up answer.");
    state.context_state.reference = Some(reference("new-context"));
    assert_eq!(
        state.current_answer_entry().unwrap().reference,
        reference_pointer(Some(&reference("follow-up")))
    );
    state.clear_context();
    assert_eq!(
        state.current_answer_entry().unwrap().answer,
        "Follow-up answer."
    );
    state.clear_follow_ups();
    let restored = state.current_answer_entry().unwrap();
    assert_eq!(restored.answer, base.answer);
    assert_eq!(restored.reference, base.reference);

    // A later request with no reference must not inherit the removed turn's pointer.
    let pending = follow_up();
    let (id, input) = begin(&mut state, &pending);
    state.context_state.reference = Some(reference("arrived-after-start"));
    assert!(complete(
        &mut state,
        id,
        &pending,
        &input,
        "Unreferenced follow-up."
    ));
    assert!(state.current_answer_entry().unwrap().reference.is_none());
}

#[test]
fn word_before_base_can_save_and_clear_without_leaking_pointer_into_new_base() {
    let mut state = state();
    accept(&mut state, &follow_up(), "word", "First word explanation.");
    assert!(state.result.is_none());
    assert_eq!(
        state.current_answer_entry().unwrap().reference,
        reference_pointer(Some(&reference("word")))
    );
    state.clear_follow_ups();
    assert!(state.current_answer_entry().is_none());
    state.context_state.reference = None;
    let pending = Pending::action(ReadingAction::Simplify);
    let (id, input) = begin(&mut state, &pending);
    assert!(complete(
        &mut state,
        id,
        &pending,
        &input,
        "New base without reference."
    ));
    assert!(state.current_answer_entry().unwrap().reference.is_none());
}

#[test]
fn failed_cancelled_and_stale_completions_preserve_last_accepted_pointer() {
    let mut state = state();
    accept(
        &mut state,
        &Pending::action(ReadingAction::Simplify),
        "base",
        "Base answer.",
    );
    accept(&mut state, &follow_up(), "accepted-turn", "Accepted turn.");
    let accepted = state.current_answer_entry().unwrap();
    let pending = follow_up();
    state.context_state.reference = Some(reference("rejected"));
    let (old_id, old_input) = begin(&mut state, &pending);
    state.cancel();
    assert!(!complete(
        &mut state,
        old_id,
        &pending,
        &old_input,
        "Cancelled answer."
    ));
    let (new_id, _) = begin(&mut state, &pending);
    assert!(!complete(
        &mut state,
        old_id,
        &pending,
        &old_input,
        "Stale answer."
    ));
    assert_eq!(state.revision, new_id);
    state.activity = Activity::Failed("Controlled failure".into());
    assert!(!complete(
        &mut state,
        new_id,
        &pending,
        &old_input,
        "Failed answer."
    ));
    let retained = state.current_answer_entry().unwrap();
    assert_eq!(retained.answer, accepted.answer);
    assert_eq!(retained.reference, accepted.reference);
    state.clear_follow_ups();
    assert_eq!(
        state.current_answer_entry().unwrap().reference,
        reference_pointer(Some(&reference("base")))
    );
}

#[test]
fn replacement_base_reset_and_new_sources_discard_previous_answer_pointers() {
    let mut state = state();
    accept(
        &mut state,
        &Pending::action(ReadingAction::Simplify),
        "old-base",
        "Old base.",
    );
    accept(&mut state, &follow_up(), "old-turn", "Old turn.");
    accept(
        &mut state,
        &Pending::action(ReadingAction::Expand),
        "new-base",
        "Replacement base.",
    );
    assert!(state.conversation.is_empty());
    assert!(state.answer_references.follow_up.is_none());
    assert_eq!(
        state.current_answer_entry().unwrap().reference,
        reference_pointer(Some(&reference("new-base")))
    );

    for transition in ["reset", "paste", "capture"] {
        accept(&mut state, &follow_up(), "discard", "Discarded turn.");
        match transition {
            "reset" => state.reset(),
            "paste" => {
                state.source = TextEdit::new("A new controlled passage.");
                state.commit_source();
            }
            _ => state.accept_capture(pickle_platform::CapturedSelection {
                text: "A new captured passage.".into(),
                app_name: "Owned fixture".into(),
                app_id: "controlled.fixture".into(),
                bounds: None,
                source_url: None,
                source_pid: 0,
                source_window: None,
            }),
        }
        assert!(state.current_answer_entry().is_none());
        assert!(state.answer_references.base.is_none());
        assert!(state.answer_references.follow_up.is_none());
        if state.snapshot.is_none() {
            state.sample();
        }
    }
}

#[test]
fn trimming_older_turns_keeps_latest_pointer_without_parallel_metadata() {
    let mut state = state();
    let pending = follow_up();
    accept(&mut state, &pending, "older", &"A".repeat(9_000));
    accept(&mut state, &pending, "latest", &"B".repeat(9_000));
    assert_eq!(state.conversation.len(), 1);
    let entry = state.current_answer_entry().unwrap();
    assert_eq!(entry.answer, "B".repeat(9_000));
    assert_eq!(
        entry.reference,
        reference_pointer(Some(&reference("latest")))
    );
}
