//! Unicode matching admits only terms in the fixed public reading fields.
use super::*;
use rune::{Env, Runtime};

fn reader(source: &str) -> Runtime<AppState> {
    let mut state = AppState::default();
    state.sample_result();
    state.demo = false;
    state.snapshot.as_mut().unwrap().text = source.into();
    state.result = None;
    let mut rt = Runtime::new(
        state,
        crate::views::view,
        (560.0, 900.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    rt
}

#[test]
fn unicode_source_terms_request_normal_consent_and_keep_the_literal_question() {
    for (source, term) in [
        ("cafe\u{301}", "café"),
        ("Straße", "STRASSE"),
        ("sß", "ss"),
        ("ß", "ss"),
        ("café", "caf"),
        ("ΟΣ", "ος"),
        ("ﬀ", "ff"),
        ("CELL\nENERGY", "  CeLL   ENERGY  "),
    ] {
        let mut rt = reader(source);
        let snapshot = rt.state.snapshot.clone();
        rt.update(|state, tx| state.explain_term(term.into(), tx));
        let Activity::Consent(pending) = &rt.state.activity else {
            panic!("Expected term consent: {source:?} / {term:?}")
        };
        assert_eq!(pending.action, ReadingAction::FollowUp);
        assert_eq!(
            pending.question,
            format!(
                "Explain the meaning of ‘{}’ in this passage, using the surrounding context. Keep it brief; distinguish the contextual meaning from other meanings.",
                term.trim()
            )
        );
        assert_eq!(pending.label, Some(format!("Explain “{}”", term.trim())));
        assert_eq!(rt.state.snapshot, snapshot);
        assert!(rt.state.result.is_none() && rt.state.conversation.is_empty());
        assert!(
            rt.state.entries.is_empty() && rt.state.services.is_none() && !rt.state.native_enabled
        );
    }
}

#[test]
fn partial_fold_accents_offsource_and_private_context_cannot_start_a_request() {
    for (source, term) in [
        ("ß", "s"),
        ("café", "cafe"),
        ("é", "e"),
        ("ﬀ", "f"),
        ("A passage", "STRASSE"),
    ] {
        let mut rt = reader(source);
        rt.update(|s, _| {
            s.context = TextEdit::new("Straße");
            s.context_state.visual_summary = "Straße".into();
            s.context_state.audio_text = "Straße".into();
        });
        let revision = rt.state.revision;
        rt.update(|s, tx| s.explain_term(term.into(), tx));
        assert!(
            matches!(rt.state.activity, Activity::Idle),
            "{source:?} / {term:?}"
        );
        assert_eq!(rt.state.revision, revision);
        assert!(
            rt.state.last.is_none() && rt.state.notice.is_some() && rt.state.services.is_none()
        );
    }
}

#[test]
fn canonical_terms_match_raw_or_displayed_answers_without_cross_field_joins() {
    for in_conversation in [false, true] {
        for term in ["STRASSE", "**STRA**SSE"] {
            let mut rt = reader("An unrelated passage.");
            rt.update(|s, _| {
                let answer = "**Stra**ße";
                if in_conversation {
                    s.conversation.push(ConversationTurn {
                        question: "A fixed question".into(),
                        answer: answer.into(),
                        quality: QualityStatus::Uncertain,
                        label: None,
                    });
                } else {
                    s.result = Some(AppState::demonstration_result());
                    s.result.as_mut().unwrap().text = answer.into();
                }
            });
            rt.update(|s, tx| s.explain_term(term.into(), tx));
            assert!(matches!(rt.state.activity, Activity::Consent(_)), "{term}");
        }
    }
    let mut rt = reader("Stra");
    rt.update(|s, _| {
        s.result = Some(AppState::demonstration_result());
        s.result.as_mut().unwrap().text = "ße".into();
    });
    rt.update(|s, tx| s.explain_term("STRASSE".into(), tx));
    assert!(matches!(rt.state.activity, Activity::Idle));
    assert!(rt.state.last.is_none());
}

#[test]
fn matching_expansion_does_not_reduce_the_original_150_character_allowance() {
    for (source, term) in [
        ("e\u{301}".repeat(150), "é".repeat(150)),
        ("ß".repeat(150), "ß".repeat(150)),
    ] {
        let mut rt = reader(&source);
        rt.update(|s, tx| s.explain_term(term.clone(), tx));
        let Activity::Consent(pending) = &rt.state.activity else {
            panic!("Original150-character term must remain valid")
        };
        assert!(pending.question.contains(&term));
        assert_eq!(pending.label, Some(format!("Explain “{term}”")));
    }
}

#[test]
fn unicode_matching_keeps_length_busy_and_pending_settings_guards() {
    for gate in ["length", "busy", "settings"] {
        let mut rt = reader(&format!("Straße {}", "é".repeat(151)));
        let cancel = CancellationToken::new();
        rt.update(|s, _| match gate {
            "busy" => {
                s.activity = Activity::Running {
                    id: s.revision,
                    cancel: cancel.clone(),
                    progress: "Fixture".into(),
                }
            }
            "settings" => s.settings_pending = true,
            _ => {}
        });
        let revision = rt.state.revision;
        let term = if gate == "length" {
            "é".repeat(151)
        } else {
            "STRASSE".into()
        };
        rt.update(|s, tx| s.explain_term(term, tx));
        assert_eq!(rt.state.revision, revision);
        assert!(rt.state.last.is_none() && rt.state.services.is_none());
        if gate == "busy" {
            assert!(matches!(rt.state.activity, Activity::Running { .. }));
            assert!(!cancel.is_cancelled());
        } else {
            assert!(matches!(rt.state.activity, Activity::Idle));
        }
    }
}
