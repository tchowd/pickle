//! Original completed-answer archive predicates, using accepted app recorders.
//! All data is synthetic; no provider, vault, native host, or user archive runs.
use super::*;

fn source() -> SelectionSnapshot {
    SelectionSnapshot::new("Cells release energy.", "Safari", "com.apple.Safari")
}

fn begin(action: ReadingAction, question: &str) -> (AppState, Pending, RequestInput) {
    let snapshot = source();
    let pending = Pending {
        question: question.into(),
        ..Pending::action(action)
    };
    let state = AppState {
        snapshot: Some(snapshot.clone()),
        revision: 1,
        activity: Activity::Running {
            id: 1,
            cancel: CancellationToken::new(),
            progress: "Controlled completed-answer recorder".into(),
        },
        settings: Settings {
            history_enabled: true,
            ..Settings::default()
        },
        ..AppState::default()
    };
    let mut input = RequestInput::new(snapshot, action);
    input.question = question.into();
    (state, pending, input)
}

fn persist_both(history: Entry, saved: Entry, check: impl Fn(&Entry, &str)) {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::new(directory.path().into()).unwrap();
    repo.append(false, history).unwrap();
    repo.append(true, saved).unwrap();
    drop(repo);
    let reopened = Repository::new(directory.path().into()).unwrap();
    for (bookmark, archive) in [(false, "history.json"), (true, "bookmarks.json")] {
        let entries = if bookmark {
            reopened.bookmarks().unwrap()
        } else {
            reopened.history().unwrap()
        };
        assert_eq!(entries.len(), 1);
        let path = directory.path().join(archive);
        let raw = std::fs::read_to_string(&path).unwrap();
        check(&entries[0], &raw);
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let fields = json["data"][0].as_object().unwrap();
        assert_eq!(fields.len(), 11);
        assert!(fields.keys().all(|key| {
            [
                "id",
                "recorded_at",
                "app_name",
                "app_id",
                "action",
                "question",
                "passage",
                "answer",
                "quality",
                "chart",
                "reference",
            ]
            .contains(&key.as_str())
        }));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn completed_follow_up_reopens_pointer_without_context_or_model_in_both_archives() {
    // Exact original testCompletedAnswerRecordsPointerNotContext data and
    // omissions, extended to both automatic History and accepted explicit Save.
    let (mut state, pending, mut input) = begin(ReadingAction::FollowUp, "Why does it matter?");
    input.page_context = "OCR PAGE TEXT".into();
    input.visual_context = "VISUAL SUMMARY TEXT".into();
    input.reference = Some(WebReference {
        url: "https://example.com/cells".into(),
        title: "Cell energy".into(),
        text: "REFERENCE BODY TEXT".into(),
        kind: "article".into(),
        timestamp: None,
    });
    input.validate().unwrap();
    let mut answer = AppState::demonstration_result();
    answer.action = ReadingAction::FollowUp;
    answer.text = "Energy powers the cell.".into();
    answer.quality = QualityStatus::Checked;
    answer.model = "secret-model-name".into();
    let history = state
        .history_entry(1, &input, &answer, &pending, true)
        .unwrap();
    assert!(state.complete_reading_result_with_reference(
        1,
        &pending,
        answer,
        reference_pointer(input.reference.as_ref())
    ));
    // A later attached reference must not replace the accepted request pointer.
    state.context_state.reference = Some(WebReference {
        url: "https://example.com/later".into(),
        title: "Unaccepted later reference".into(),
        text: "LATER REFERENCE BODY".into(),
        kind: "article".into(),
        timestamp: None,
    });
    let saved = state.current_answer_entry().unwrap();
    persist_both(history, saved, |entry, raw| {
        assert_eq!(entry.action, "Follow-up");
        assert_eq!(entry.question.as_deref(), Some("Why does it matter?"));
        assert_eq!(entry.passage, "Cells release energy.");
        assert_eq!(entry.answer, "Energy powers the cell.");
        assert_eq!(entry.app_name, "Safari");
        assert_eq!(entry.app_id, "com.apple.Safari");
        assert_eq!(
            entry.quality.as_deref(),
            Some("Checked against your selection")
        );
        assert_eq!(
            entry.reference,
            Some(EntryReference {
                url: "https://example.com/cells".into(),
                title: "Cell energy".into(),
                kind: "article".into(),
            })
        );
        assert!(entry.chart.is_none());
        for absent in [
            "OCR PAGE TEXT",
            "VISUAL SUMMARY TEXT",
            "REFERENCE BODY TEXT",
            "secret-model-name",
            "LATER REFERENCE BODY",
            "https://example.com/later",
        ] {
            assert!(
                !raw.contains(absent),
                "Private supplemental field entered an archive"
            );
        }
        let json: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(json["data"][0]["reference"].as_object().unwrap().len(), 3);
    });
}

#[test]
fn completed_flow_reopens_text_alternative_and_kind_without_reference_in_both_archives() {
    // This checks storing an already validated type, as the original archive
    // check does; it does not assert these nodes came from a live chart request.
    let chart = ValidatedChart::Flow {
        nodes: vec![
            pickle_core::FlowNode {
                id: "a".into(),
                label: "Received".into(),
            },
            pickle_core::FlowNode {
                id: "b".into(),
                label: "Saved".into(),
            },
        ],
        edges: vec![pickle_core::FlowEdge {
            from: "a".into(),
            to: "b".into(),
            evidence: "received and saved".into(),
            condition: String::new(),
        }],
    };
    let alternative = chart.alternative();
    let (mut state, pending, input) = begin(ReadingAction::Chart, "");
    let mut answer = AppState::demonstration_result();
    answer.action = ReadingAction::Chart;
    answer.text = alternative.clone();
    answer.chart = Some(chart);
    answer.quality = QualityStatus::ChartValidated;
    answer.model = "CHART_MODEL_NOT_ARCHIVED".into();
    let history = state
        .history_entry(1, &input, &answer, &pending, true)
        .unwrap();
    assert!(state.complete_reading_result_with_reference(1, &pending, answer, None));
    let saved = state.current_answer_entry().unwrap();
    persist_both(history, saved, |entry, raw| {
        assert_eq!(entry.action, "Visualize");
        assert!(entry.question.is_none());
        assert_eq!(entry.passage, "Cells release energy.");
        assert_eq!(entry.answer, alternative);
        assert_eq!(entry.chart.as_deref(), Some("flow_diagram"));
        assert!(entry.reference.is_none());
        assert_eq!(
            entry.quality.as_deref(),
            Some("Chart structure and source evidence checked")
        );
        assert!(!raw.contains("CHART_MODEL_NOT_ARCHIVED"));
    });
}

#[test]
fn original_three_record_dedup_order_search_and_removals_reopen_in_both_archives() {
    // Original testEachAnswerGetsOneRecordAndSearchFindsIt: accepted recorders
    // feed the real stores; the separate intent tests cover actual UI entrypoints.
    let record = |action, question: &str, intent, text: &str, with_reference: bool| {
        let (mut state, mut pending, mut input) = begin(action, question);
        pending.recorded_intent = intent;
        input.reference = with_reference.then(|| WebReference {
            url: "https://example.com/cells".into(),
            title: "Cell energy".into(),
            kind: "article".into(),
            text: "REFERENCE BODY TEXT".into(),
            timestamp: None,
        });
        input.validate().unwrap();
        let mut answer = AppState::demonstration_result();
        answer.action = action;
        answer.text = text.into();
        answer.quality = QualityStatus::Checked;
        let history = state
            .history_entry(1, &input, &answer, &pending, true)
            .unwrap();
        assert!(state.complete_reading_result_with_reference(
            1,
            &pending,
            answer,
            reference_pointer(input.reference.as_ref())
        ));
        (history, state.current_answer_entry().unwrap())
    };
    let records = [
        record(ReadingAction::Simplify, "", None, "Simpler.", true),
        record(ReadingAction::Simplify, "", None, "Simpler.", true),
        record(
            ReadingAction::FollowUp,
            "Explain…",
            Some(RecordedIntent::Explain("energy".into())),
            "Meaning.",
            false,
        ),
        record(
            ReadingAction::FollowUp,
            "Rewrite…",
            Some(RecordedIntent::Adjustment("Shorter".into())),
            "Short.",
            false,
        ),
    ];
    let read = |repo: &Repository, bookmark| {
        if bookmark {
            repo.bookmarks().unwrap()
        } else {
            repo.history().unwrap()
        }
    };
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().into()).unwrap();
        for ((history, saved), expected_count) in records.iter().zip([1, 1, 2, 3]) {
            repo.append(bookmark, if bookmark { saved } else { history }.clone())
                .unwrap();
            assert_eq!(read(&repo, bookmark).len(), expected_count);
        }
        drop(repo);
        let reopened = Repository::new(directory.path().into()).unwrap();
        let entries = read(&reopened, bookmark);
        assert_eq!(
            entries
                .iter()
                .map(|e| e.action.as_str())
                .collect::<Vec<_>>(),
            ["Adjustment", "Explain", "Simplify"]
        );
        assert_eq!(entries.iter().filter(|e| e.action == "Simplify").count(), 1);
        for query in [
            "cell energy",
            "EXAMPLE.COM/cells",
            "shorter",
            "Meaning",
            "release",
        ] {
            assert!(
                entries.iter().any(|entry| entry.matches(query)),
                "Original query must find an accepted record: {query}"
            );
        }
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.matches("Shorter"))
                .map(|e| e.action.as_str())
                .collect::<Vec<_>>(),
            ["Adjustment"]
        );
        assert!(!entries.iter().any(|entry| entry.matches("not recorded")));
        reopened.remove(bookmark, &entries[0].id).unwrap();
        drop(reopened);
        let reopened = Repository::new(directory.path().into()).unwrap();
        assert_eq!(read(&reopened, bookmark).len(), 2);
        reopened.remove_all(bookmark).unwrap();
        drop(reopened);
        let reopened = Repository::new(directory.path().into()).unwrap();
        assert!(read(&reopened, bookmark).is_empty());
        assert!(read(&reopened, !bookmark).is_empty());
    }
}
