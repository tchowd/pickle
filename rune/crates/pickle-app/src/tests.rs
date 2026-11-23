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

#[cfg(test)]
mod reader_parity {
    use super::*;
    use crate::controller::Pending;
    use pickle_core::{ConversationTurn, QualityStatus, ReadingAction};
    use rune::{Key, KeyEvent, Mods, TextEdit};
    use tokio_util::sync::CancellationToken;

    fn reader() -> Runtime<AppState> {
        let mut state = AppState::default();
        state.sample_result();
        state.demo = false;
        let mut rt = Runtime::new(state, views::view, (560.0, 2000.0), Env::new(true, 1.0));
        rt.on_key = Some(crate::on_key);
        rt.frame(0.0);
        rt
    }
    fn turn() -> ConversationTurn {
        ConversationTurn {
            question: "A prior question".into(),
            answer: "A prior answer".into(),
            label: None,
            quality: QualityStatus::NotChecked(QualityStatus::OFFLINE.into()),
        }
    }
    fn key(rt: &mut Runtime<AppState>, key: Key) {
        rt.key(KeyEvent {
            key,
            mods: Mods {
                // Rune normalizes the primary modifier to `cmd` on every OS.
                cmd: true,
                ctrl: !cfg!(target_os = "macos"),
                ..Mods::default()
            },
        });
        rt.frame(0.0);
    }
    fn entry() -> crate::storage::Entry {
        crate::storage::Entry {
            id: "parity".into(),
            recorded_at: "2026-01-01T00:00:00Z".into(),
            app_name: "Fixture".into(),
            app_id: "fixture".into(),
            action: "Simplify".into(),
            question: None,
            passage: "A fixed passage".into(),
            answer: "**Important** and `literal`.\n\nA second paragraph.".into(),
            quality: None,
            chart: None,
            reference: None,
        }
    }

    #[test]
    fn disabled_library_next_keeps_previous_one_click_away() {
        for bookmarks in [false, true] {
            let records: Vec<_> = (0..21)
                .map(|index| crate::storage::Entry {
                    id: format!("page-{index}"),
                    ..entry()
                })
                .collect();
            let state = AppState {
                route: Route::Library(bookmarks),
                entries: records.clone().into(),
                bookmarks: records.clone().into(),
                library_page: 1,
                ..AppState::default()
            };
            let mut rt = Runtime::new(state, views::view, (560.0, 900.0), Env::new(true, 1.0));
            rt.frame(0.0);
            assert!(rt.find("entry-page-20").is_some());
            for _ in 0..60 {
                rt.frame(1.0 / 60.0);
            }
            assert_eq!(rt.state.library_page, 1);
            for _ in 0..3 {
                click(&mut rt, "library-next");
                assert_eq!(rt.state.library_page, 1);
            }
            click(&mut rt, "library-previous");
            assert_eq!(rt.state.library_page, 0);
            assert!(rt.find("entry-page-0").is_some());
            assert!(rt.find("entry-page-20").is_none());
            rt.update(|_, tx| tx.scroll_into_view("library-next"));
            click(&mut rt, "library-next");
            for _ in 0..60 {
                rt.frame(1.0 / 60.0);
            }
            assert_eq!(rt.state.library_page, 1);
            click(&mut rt, "library-previous");
            assert_eq!(rt.state.library_page, 0);
            assert_eq!(rt.state.entries, records);
            assert_eq!(rt.state.bookmarks, records);
            assert!(rt.state.services.is_none());
        }
    }

    #[test]
    fn library_search_resets_page_only_after_actual_text_change() {
        let records: Vec<_> = (0..21)
            .map(|index| crate::storage::Entry {
                id: format!("search-{index}"),
                ..entry()
            })
            .collect();
        let state = AppState {
            route: Route::Library(false),
            entries: records.into(),
            library_page: 1,
            ..AppState::default()
        };
        let mut rt = Runtime::new(state, views::view, (560.0, 900.0), Env::new(true, 1.0));
        click(&mut rt, "search-library");
        assert_eq!(rt.state.library_page, 1);
        rt.key(KeyEvent {
            key: Key::Left,
            mods: Mods::default(),
        });
        rt.frame(0.0);
        key(&mut rt, Key::Command('a'));
        assert!(rt.state.search.text.is_empty());
        assert_eq!(rt.state.library_page, 1);
        rt.paste("fixed");
        rt.frame(0.0);
        assert_eq!(rt.state.search.text, "fixed");
        assert_eq!(rt.state.library_page, 0);
        rt.update(|state, _| {
            state.complete_library_search_fixture(false);
        });
        rt.frame(0.0);
        assert!(rt.find("entry-search-0").is_some());
        assert!(rt.state.services.is_none());
    }

    #[test]
    fn copy_buttons_use_displayed_plain_text_without_touching_source_answers() {
        let expected = "Important and literal.\n\nA second paragraph.";
        for latest_turn in [false, true] {
            let mut rt = reader();
            let clipboard = rune::clipboard::memory();
            rt.use_clipboard(&clipboard);
            rt.update(|state, _| {
                if latest_turn {
                    state.conversation.push(ConversationTurn {
                        answer: entry().answer,
                        ..turn()
                    });
                } else {
                    state.result.as_mut().unwrap().text = entry().answer;
                }
            });
            let source = rt.state.snapshot.clone();
            let raw_answer = rt.state.answer().to_owned();
            click(&mut rt, "copy-answer");
            assert_eq!(clipboard.text().unwrap().as_str(), expected);
            assert_eq!(rt.state.answer(), raw_answer);
            assert_eq!(rt.state.snapshot, source);
        }
        for bookmarks in [false, true] {
            for detail in [false, true] {
                let mut rt = reader();
                let clipboard = rune::clipboard::memory();
                rt.use_clipboard(&clipboard);
                rt.update(|state, _| {
                    state.route = Route::Library(bookmarks);
                    state.entries = vec![entry()].into();
                    state.bookmarks = vec![entry()].into();
                    state.library_selected = detail.then(|| "parity".into());
                });
                click(
                    &mut rt,
                    if detail {
                        "copy-saved-detail"
                    } else {
                        "copy-parity"
                    },
                );
                assert_eq!(clipboard.text().unwrap().as_str(), expected);
                assert_eq!(rt.state.entries[0].answer, entry().answer);
                assert_eq!(rt.state.bookmarks[0].answer, entry().answer);
                assert!(rt.state.services.is_none());
            }
        }
    }

    #[test]
    fn action_shortcuts_and_buttons_cannot_replace_running_or_waiting_readings() {
        for waiting in [false, true] {
            for (command, button) in [('1', "simplify"), ('2', "expand"), ('3', "chart")] {
                let mut rt = reader();
                let cancel = CancellationToken::new();
                rt.update(|state, _| {
                    state.draft = "A live draft".into();
                    state.activity = if waiting {
                        Activity::WaitingContext
                    } else {
                        Activity::Running {
                            id: state.revision,
                            cancel: cancel.clone(),
                            progress: "Writing".into(),
                        }
                    };
                });
                let revision = rt.state.revision;
                let snapshot = rt.state.snapshot.clone();
                click(&mut rt, button);
                key(&mut rt, Key::Command(command));
                assert!(!cancel.is_cancelled());
                assert_eq!(rt.state.revision, revision);
                assert_eq!(rt.state.snapshot, snapshot);
                assert_eq!(rt.state.draft, "A live draft");
                assert!(if waiting {
                    matches!(rt.state.activity, Activity::WaitingContext)
                } else {
                    matches!(rt.state.activity, Activity::Running { .. })
                });
                assert!(rt.state.services.is_none());
            }
        }
        for (command, expected) in [
            ('1', ReadingAction::Simplify),
            ('2', ReadingAction::Expand),
            ('3', ReadingAction::Chart),
        ] {
            let mut rt = reader();
            key(&mut rt, Key::Command(command));
            assert!(
                matches!(&rt.state.activity, Activity::Consent(pending) if pending.action == expected)
            );
        }
    }

    #[test]
    fn clear_follow_ups_retains_the_reading_context_and_invalidates_deleted_turn_consent() {
        let mut rt = reader();
        rt.update(|state, _| {
            state.conversation = vec![turn(); pickle_core::limits::TURNS];
            state.question = TextEdit::new("An unsent draft");
            state.context = TextEdit::new("Manual context");
            state.context_state.visual_summary = "Retained visual context".into();
            state.context_state.audio_text = "Retained audio context".into();
            state.last = Some(Pending::action(ReadingAction::FollowUp));
            state.activity = Activity::Consent(state.last.clone().unwrap());
        });
        let snapshot = rt.state.snapshot.clone();
        let result = serde_json::to_value(&rt.state.result).unwrap();
        let settings = rt.state.settings.clone();
        let context_id = rt.state.context_state.id;
        let context_cancel = rt.state.context_state.cancel.clone();
        let revision = rt.state.revision;
        click(&mut rt, "clear-follow-ups");
        assert!(rt.state.conversation.is_empty());
        assert_eq!(rt.state.snapshot, snapshot);
        assert_eq!(serde_json::to_value(&rt.state.result).unwrap(), result);
        assert_eq!(rt.state.settings, settings);
        assert_eq!(rt.state.context.text, "Manual context");
        assert_eq!(
            rt.state.context_state.visual_summary,
            "Retained visual context"
        );
        assert_eq!(rt.state.context_state.audio_text, "Retained audio context");
        assert_eq!(rt.state.context_state.id, context_id);
        assert!(!context_cancel.is_cancelled());
        assert_eq!(rt.state.question.text, "An unsent draft");
        assert!(rt.state.revision > revision);
        assert_eq!(
            rt.state.last.as_ref().unwrap().action,
            ReadingAction::Simplify
        );
        assert!(matches!(rt.state.activity, Activity::Idle));
        rt.update(|state, tx| state.consent(tx));
        assert!(matches!(rt.state.activity, Activity::Idle));
        assert_eq!(rt.state.settings, settings);
        rt.update(|state, tx| state.follow_up("A fresh question".into(), None, tx));
        assert!(matches!(rt.state.activity, Activity::Consent(_)));
        assert!(rt.state.services.is_none());
    }

    #[test]
    fn clear_and_term_actions_keep_busy_settings_and_six_turn_limits() {
        for gate in [0, 1, 2] {
            let mut rt = reader();
            let cancel = CancellationToken::new();
            rt.update(|state, _| {
                state.conversation = vec![turn(); pickle_core::limits::TURNS];
                match gate {
                    0 => {
                        state.activity = Activity::Running {
                            id: state.revision,
                            cancel: cancel.clone(),
                            progress: "Writing".into(),
                        }
                    }
                    1 => state.activity = Activity::WaitingContext,
                    _ => state.settings_pending = true,
                }
            });
            let revision = rt.state.revision;
            rt.update(|state, tx| {
                state.clear_follow_ups();
                state.explain_term("evidence".into(), tx);
            });
            assert_eq!(rt.state.conversation.len(), pickle_core::limits::TURNS);
            assert_eq!(rt.state.revision, revision);
            assert!(!cancel.is_cancelled());
        }
        let mut rt = reader();
        rt.update(|state, tx| {
            state.conversation = vec![turn(); pickle_core::limits::TURNS];
            state.explain_term("evidence".into(), tx);
        });
        assert!(
            matches!(&rt.state.activity, Activity::Failed(error) if error.contains("six follow-ups"))
        );
        assert_eq!(rt.state.conversation.len(), pickle_core::limits::TURNS);
    }

    #[test]
    fn invalid_or_private_context_terms_cannot_trigger_an_action_or_consent() {
        for term in [
            "".into(),
            "   ".into(),
            "not-in-source".into(),
            "é".repeat(151),
            "private-context-only".into(),
        ] {
            let mut rt = reader();
            rt.update(|state, _| {
                state.result = None;
                state.context = TextEdit::new("private-context-only");
                state.context_state.visual_summary = "private-context-only".into();
                state.context_state.audio_text = "private-context-only".into();
            });
            let revision = rt.state.revision;
            rt.update(|state, tx| state.explain_term(term, tx));
            assert_eq!(rt.state.revision, revision);
            assert!(matches!(rt.state.activity, Activity::Idle));
            assert!(rt.state.last.is_none());
            assert!(rt.state.notice.is_some());
            assert!(rt.state.services.is_none());
        }
        let mut rt = reader();
        rt.update(|state, tx| {
            state.result.as_mut().unwrap().text = "Use **cell**\nenergy carefully.".into();
            state.explain_term("  CeLL   ENERGY  ".into(), tx);
        });
        assert!(
            matches!(&rt.state.activity, Activity::Consent(pending) if pending.action == ReadingAction::FollowUp && pending.label.as_deref() == Some("Explain “CeLL   ENERGY”"))
        );
        let mut rt = reader();
        rt.update(|state, tx| {
            state.result = None;
            state.snapshot.as_mut().unwrap().text = "é".repeat(150);
            state.explain_term("é".repeat(150), tx);
        });
        assert!(matches!(rt.state.activity, Activity::Consent(_)));
    }

    #[test]
    fn selected_source_copies_and_explains_before_a_base_answer() {
        let mut rt = reader();
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|state, _| state.result = None);
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        let source = rt
            .elements()
            .into_iter()
            .find(|element| element.text.as_deref() == Some(crate::controller::SAMPLE))
            .unwrap();
        // Use the same font/style/scale as the source, rather than assuming
        // macOS glyph advances or where a Windows font's first line sits.
        let scale = rt.env.scale;
        let layout = rt.text.layout_rich(
            crate::controller::SAMPLE,
            &[],
            rune::tok::TextStyle::new(14.0, 400.0).lh(21.0),
            scale,
            Some(source.rect[2] * scale / source.scale),
        );
        let boxes = rune::text::range_boxes(&layout, 0, "The".len(), false);
        assert_eq!(boxes.len(), 1, "The first word must occupy one shaped line");
        let word = boxes[0].0;
        let inset = (word.x1 - word.x0) * 0.01;
        let word_y = ((word.y0 + word.y1) * 0.5) as f32;
        let start_x = (word.x0 + inset) as f32;
        let end_x = (word.x1 - inset) as f32;
        assert_eq!(
            rune::text::cursor_from_point(&layout, start_x, word_y).index(),
            0
        );
        assert_eq!(
            rune::text::cursor_from_point(&layout, end_x, word_y).index(),
            3
        );
        let to_window = source.scale / scale;
        let (x, end_x, y) = (
            source.rect[0] + start_x * to_window,
            source.rect[0] + end_x * to_window,
            source.rect[1] + word_y * to_window,
        );
        assert_eq!(
            rt.element_at(x, y).as_ref().map(|picked| picked.path()),
            Some(source.path.as_str()),
            "The shaped word point must hit the actual source text: {source:?} {word:?}"
        );
        rt.pointer_down(x, y);
        rt.pointer_move(end_x, y);
        rt.pointer_up(end_x, y);
        rt.frame(0.0);
        let selected = rt.selected_text();
        assert_eq!(
            selected.as_deref(),
            Some("The"),
            "Shaped source drag did not select its exact first word: source={source:?}, word={word:?}, points=({x},{y})..({end_x},{y}), anchor={:?}, hit={:?}, events={:?}",
            rt.text_selection(),
            rt.element_at(x, y),
            rt.events
        );
        let selected = selected.unwrap();
        key(&mut rt, Key::Command('c'));
        assert_eq!(clipboard.text().unwrap().trim(), selected);
        let snapshot = rt.state.snapshot.clone();
        click(&mut rt, "explain-selection");
        assert!(
            matches!(&rt.state.activity, Activity::Consent(pending) if pending.label == Some(format!("Explain “{selected}”")))
        );
        assert_eq!(rt.state.snapshot, snapshot);
        assert!(rt.state.result.is_none() && rt.state.services.is_none());
    }

    #[test]
    fn follow_up_completion_preserves_an_unsent_term_edited_while_busy() {
        let mut rt = reader();
        let mut pending = Pending::action(ReadingAction::FollowUp);
        pending.question = "Explain the passage in more detail.".into();
        pending.label = Some("More detail".into());
        let mut result = rt.state.result.clone().unwrap();
        result.action = ReadingAction::FollowUp;
        result.text = "A completed follow-up answer.".into();
        rt.update(|state, _| {
            state.term = TextEdit::new("A previous term");
            state.activity = Activity::Running {
                id: state.revision,
                cancel: CancellationToken::new(),
                progress: "Writing".into(),
            };
        });
        let snapshot = rt.state.snapshot.clone();
        click(&mut rt, "explain-term");
        key(&mut rt, Key::Command('a'));
        rt.paste("A newly typed unsent term");
        rt.frame(0.0);
        assert_eq!(rt.state.term.text, "A newly typed unsent term");
        assert!(matches!(rt.state.activity, Activity::Running { .. }));
        rt.update(|state, _| {
            assert!(state.complete_reading_result(state.revision, &pending, result));
        });
        assert_eq!(rt.state.term.text, "A newly typed unsent term");
        assert_eq!(
            rt.state.conversation.last().unwrap().answer,
            "A completed follow-up answer."
        );
        assert_eq!(rt.state.snapshot, snapshot);
        assert!(matches!(rt.state.activity, Activity::Idle));
        assert!(rt.state.services.is_none());
    }

    #[test]
    fn initial_term_answer_completes_renders_copies_and_builds_a_real_saved_entry() {
        let mut rt = reader();
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|state, _| {
            state.result = None;
            state.panel = Panel::Choosing;
        });
        let snapshot = rt.state.snapshot.clone();
        click(&mut rt, "explain-term");
        rt.paste("evidence");
        rt.key(KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        });
        rt.frame(0.0);
        let Activity::Consent(pending) = &rt.state.activity else {
            panic!("Initial word explanation bypassed consent or did not submit");
        };
        let pending = pending.clone();
        assert!(rt.state.result.is_none());
        let mut fixture = AppState::default();
        fixture.sample_result();
        let mut result = fixture.result.take().unwrap();
        result.action = ReadingAction::FollowUp;
        result.text = "**Evidence** means information supporting a claim.".into();
        rt.update(|state, _| {
            state.activity = Activity::Running {
                id: state.revision,
                cancel: CancellationToken::new(),
                progress: "Writing".into(),
            };
            assert!(!state.complete_reading_result(
                state.revision.wrapping_sub(1),
                &pending,
                result.clone()
            ));
            assert!(state.complete_reading_result(state.revision, &pending, result));
        });
        rt.frame(0.0);
        assert!(rt.state.result.is_none());
        assert_eq!(rt.state.snapshot, snapshot);
        assert_eq!(rt.state.conversation.len(), 1);
        assert!(rt.find("turn-0").is_some());
        assert!(rt.find("save-answer").is_some());
        assert!(rt.find("clear-follow-ups").is_some());
        let saved = rt.state.current_answer_entry().unwrap();
        assert_eq!(saved.action, "Follow-up");
        assert_eq!(saved.answer, rt.state.conversation[0].answer);
        assert_eq!(saved.passage, snapshot.as_ref().unwrap().text);
        assert_eq!(saved.question.as_deref(), Some("Explain “evidence”"));
        assert_ne!(saved.question.as_deref(), Some(pending.question.as_str()));
        click(&mut rt, "copy-answer");
        assert_eq!(
            clipboard.text().unwrap().as_str(),
            "Evidence means information supporting a claim."
        );
        click(&mut rt, "clear-follow-ups");
        assert!(rt.state.result.is_none() && rt.state.conversation.is_empty());
        assert_eq!(rt.state.snapshot, snapshot);
        assert!(rt.state.services.is_none());
    }

    #[test]
    fn term_membership_uses_only_the_visible_automatic_preview_until_commit() {
        for prior_reading in [false, true] {
            let mut rt = reader();
            rt.update(|state, _| {
                if prior_reading {
                    state.result.as_mut().unwrap().text = "alpha-old-answer".into();
                } else {
                    state.reset();
                }
                state.settings.automatic_menu = true;
                assert!(state.show_auto_capture(pickle_platform::CapturedSelection {
                    text: "The beta-preview is visible.".into(),
                    app_name: "Preview editor".into(),
                    app_id: "preview-editor".into(),
                    bounds: None,
                    source_url: None,
                    source_pid: 0,
                    source_window: None,
                }));
            });
            let old_snapshot = rt.state.snapshot.clone();
            let old_revision = rt.state.revision;
            rt.update(|state, tx| state.explain_term("alpha-old-answer".into(), tx));
            assert_eq!(rt.state.snapshot, old_snapshot);
            assert_eq!(rt.state.revision, old_revision);
            assert!(rt.state.auto_preview.is_some());
            assert!(
                rt.state
                    .notice
                    .as_deref()
                    .unwrap()
                    .contains("passage or answer")
            );
            rt.update(|state, tx| state.explain_term("beta-preview".into(), tx));
            assert!(rt.state.auto_preview.is_none());
            assert_eq!(
                rt.state.snapshot.as_ref().unwrap().text,
                "The beta-preview is visible."
            );
            assert_eq!(rt.state.snapshot.as_ref().unwrap().app_id, "preview-editor");
            assert!(rt.state.result.is_none() && rt.state.conversation.is_empty());
            assert!(
                matches!(&rt.state.activity, Activity::Consent(pending) if pending.question.contains("beta-preview"))
            );
            assert!(rt.state.services.is_none());
        }
    }

    #[test]
    fn choosing_discloses_short_and_multiline_passages_without_changing_the_source() {
        for passage in [
            format!("{} hiddenword", "A bounded sentence. ".repeat(15)),
            "First line\nSecond line\nA hidden third line".into(),
            "w".repeat(300),
        ] {
            assert!(passage.chars().count() <= 450);
            let mut rt = reader();
            rt.update(|state, _| {
                state.source = TextEdit::new(passage.clone());
                state.commit_source();
            });
            let snapshot = rt.state.snapshot.clone();
            assert!(rt.find("toggle-full-source").is_some());
            click(&mut rt, "toggle-full-source");
            assert!(rt.state.source_expanded);
            assert_eq!(rt.state.snapshot, snapshot);
            assert!(rt.state.result.is_none() && rt.state.conversation.is_empty());
            rt.set_viewport((480.0, 720.0), 1.0);
            rt.frame(0.0);
            let element = rt
                .elements()
                .into_iter()
                .find(|element| element.text.as_deref() == Some(&passage))
                .unwrap();
            assert!(
                element.rect[0] >= 20.0 && element.rect[0] + element.rect[2] <= 460.0,
                "Disclosed passage overflowed: {:?}",
                element.rect
            );
            assert!(
                element.rect[3] > 42.0,
                "Disclosed passage remained clipped: {:?}",
                element.rect
            );
            click(&mut rt, "toggle-full-source");
            assert!(!rt.state.source_expanded);
            assert_eq!(rt.state.snapshot, snapshot);
            assert!(rt.state.services.is_none());
        }
    }
}
