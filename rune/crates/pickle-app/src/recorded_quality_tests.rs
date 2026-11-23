//! Original check-label predicates through Pipeline, accepted record builders,
//! and reopened archives. All writers/checkers are in-memory owned fixtures.
use super::*;
use async_trait::async_trait;
use std::sync::atomic::AtomicUsize;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

const SOURCE: &str = "The drug may help some patients.";
const PROSE: &str = "It may help some patients.";
const CHART_SOURCE: &str = "Revenue was $10 million and $15 million.";
const CHART: &str = r#"{"type":"bar_chart","bars":[{"evidenceID":"n12"},{"evidenceID":"n28"}]}"#;

#[derive(Clone, Copy)]
enum Check {
    Clear,
    Concern,
    Uncertain,
    Unavailable,
    ReviewFails,
}
struct Writer {
    calls: AtomicUsize,
    chart: bool,
    fail_repair: bool,
}
#[async_trait]
impl GenerativeProvider for Writer {
    fn is_remote(&self) -> bool {
        false
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: UnboundedSender<PipelineEvent>,
        _: CancellationToken,
    ) -> pickle_core::Result<Generation> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(call < 2, "The fixture permits at most one repair");
        assert_eq!(prompt.structured, self.chart);
        assert_eq!(
            prompt.local_input.as_ref().unwrap().snapshot.text,
            if self.chart { CHART_SOURCE } else { SOURCE }
        );
        if call == 1 {
            assert!(prompt.system.contains("Revise your prior answer."));
        }
        if call == 1 && self.fail_repair {
            return Err(PickleError::message("Controlled repair unavailable"));
        }
        let text = if self.chart { CHART } else { PROSE };
        if !self.chart {
            let _ = events.send(PipelineEvent::Draft(text.into()));
        }
        Ok(Generation::new(text, "PRIVATE_QUALITY_WRITER_MODEL"))
    }
}
struct Checker {
    calls: AtomicUsize,
    mode: Check,
}
#[async_trait]
impl DecisionClient for Checker {
    async fn evaluate(
        &self,
        state: DecisionState,
        questions: Questions,
        _: CancellationToken,
    ) -> pickle_core::Result<DecisionResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(
            call < 3,
            "Only preflight, review and one recheck are allowed"
        );
        assert_eq!(state.get("source").map(String::as_str), Some(SOURCE));
        let review = state.contains_key("candidate");
        if matches!(self.mode, Check::Unavailable)
            || (review && matches!(self.mode, Check::ReviewFails))
        {
            return Err(PickleError::message("Controlled evaluator unavailable"));
        }
        let answers = questions
            .iter()
            .map(|(key, question)| {
                let answer = if question.kind == "choice" {
                    let criteria = question.criteria.as_ref().unwrap();
                    assert!(criteria.contains_key("everyday"));
                    DecisionAnswer {
                        kind: "choice".into(),
                        noul: None,
                        choice: Some("everyday".into()),
                        confidence: Some(1.0),
                        probabilities: Some(
                            criteria
                                .keys()
                                .map(|key| (key.clone(), if key == "everyday" { 1.0 } else { 0.0 }))
                                .collect(),
                        ),
                    }
                } else {
                    DecisionAnswer::noul(if review {
                        match self.mode {
                            Check::Concern if key == "lost_uncertainty" => 0.9,
                            Check::Uncertain => 0.5,
                            _ => 0.1,
                        }
                    } else {
                        0.1
                    })
                };
                (key.clone(), answer)
            })
            .collect();
        Ok(DecisionResponse {
            model: "PRIVATE_QUALITY_EVALUATOR_MODEL".into(),
            answers,
            usage: None,
        })
    }
}

struct Case {
    name: &'static str,
    check: Option<Check>,
    offline: bool,
    fail_repair: bool,
    chart: bool,
    calls: (usize, usize),
    quality: QualityStatus,
    label: Option<&'static str>,
}

fn persist_accepted(input: &RequestInput, result: ReadingResult, expected: Option<&str>) {
    let accepted_text = result.text.clone();
    let mut state = AppState {
        snapshot: Some(input.snapshot.clone()),
        revision: 1,
        activity: Activity::Running {
            id: 1,
            cancel: CancellationToken::new(),
            progress: "Owned quality fixture".into(),
        },
        settings: Settings {
            history_enabled: true,
            ..Settings::default()
        },
        ..AppState::default()
    };
    let pending = Pending::action(input.action);
    let history = state
        .history_entry(1, input, &result, &pending, true)
        .unwrap();
    assert!(state.complete_reading_result_with_reference(1, &pending, result, None));
    let saved = state.current_answer_entry().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::new(directory.path().into()).unwrap();
    repo.append(false, history).unwrap();
    repo.append(true, saved).unwrap();
    drop(repo);
    let reopened = Repository::new(directory.path().into()).unwrap();
    for (bookmark, filename) in [(false, "history.json"), (true, "bookmarks.json")] {
        let entries = if bookmark {
            reopened.bookmarks()
        } else {
            reopened.history()
        }
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].quality.as_deref(), expected);
        assert_eq!(entries[0].passage, input.snapshot.text);
        assert_eq!(entries[0].answer, accepted_text);
        let raw = std::fs::read_to_string(directory.path().join(filename)).unwrap();
        let archive: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(archive["data"][0]["quality"], serde_json::json!(expected));
        for omitted in [
            "PRIVATE_QUALITY_WRITER_MODEL",
            "PRIVATE_QUALITY_EVALUATOR_MODEL",
        ] {
            assert!(!raw.contains(omitted));
        }
        assert!(state.services.is_none());
    }
}

#[tokio::test]
async fn original_quality_labels_follow_actual_pipeline_outcomes_into_both_archives() {
    let cases = [
        Case {
            name: "original disabled",
            check: None,
            offline: false,
            fail_repair: false,
            chart: false,
            calls: (1, 0),
            quality: QualityStatus::NotChecked("Jev disabled".into()),
            label: None,
        },
        Case {
            name: "original offline",
            check: Some(Check::Unavailable),
            offline: true,
            fail_repair: false,
            chart: false,
            calls: (1, 0),
            quality: QualityStatus::NotChecked("local-only mode".into()),
            label: None,
        },
        Case {
            name: "original unavailable",
            check: Some(Check::Unavailable),
            offline: false,
            fail_repair: false,
            chart: false,
            calls: (1, 1),
            quality: QualityStatus::NotChecked("Jev unavailable".into()),
            label: Some("Semantic checks not performed · Jev unavailable"),
        },
        Case {
            name: "checked",
            check: Some(Check::Clear),
            offline: false,
            fail_repair: false,
            chart: false,
            calls: (1, 2),
            quality: QualityStatus::Checked,
            label: Some("Checked against your selection"),
        },
        Case {
            name: "remaining concerns after one repair",
            check: Some(Check::Concern),
            offline: false,
            fail_repair: false,
            chart: false,
            calls: (2, 3),
            quality: QualityStatus::Concerns(vec!["lost_uncertainty".into()]),
            label: Some("Check found concerns — review the source"),
        },
        Case {
            name: "uncertain",
            check: Some(Check::Uncertain),
            offline: false,
            fail_repair: false,
            chart: false,
            calls: (1, 2),
            quality: QualityStatus::Uncertain,
            label: Some("Check inconclusive — review the source"),
        },
        Case {
            name: "validated chart",
            check: None,
            offline: false,
            fail_repair: false,
            chart: true,
            calls: (1, 0),
            quality: QualityStatus::ChartValidated,
            label: Some("Chart structure and source evidence checked"),
        },
        Case {
            name: "review failure",
            check: Some(Check::ReviewFails),
            offline: false,
            fail_repair: false,
            chart: false,
            calls: (1, 2),
            quality: QualityStatus::NotChecked("Jev check or repair failed".into()),
            label: Some("Semantic checks not performed · Jev check or repair failed"),
        },
        Case {
            name: "repair failure",
            check: Some(Check::Concern),
            offline: false,
            fail_repair: true,
            chart: false,
            calls: (2, 2),
            quality: QualityStatus::NotChecked("Jev check or repair failed".into()),
            label: Some("Semantic checks not performed · Jev check or repair failed"),
        },
    ];
    for case in cases {
        let writer = Arc::new(Writer {
            calls: AtomicUsize::new(0),
            chart: case.chart,
            fail_repair: case.fail_repair,
        });
        let checker = case.check.map(|mode| {
            Arc::new(Checker {
                calls: AtomicUsize::new(0),
                mode,
            })
        });
        let evaluator = checker
            .clone()
            .map(|checker| checker as Arc<dyn DecisionClient>);
        let action = if case.chart {
            ReadingAction::Chart
        } else {
            ReadingAction::Simplify
        };
        let mut input = RequestInput::new(
            SelectionSnapshot::new(
                if case.chart { CHART_SOURCE } else { SOURCE },
                "Test",
                "test",
            ),
            action,
        );
        if case.chart {
            input.chart_choice = Some(ChartKind::Bar);
        }
        let (events, _) = unbounded_channel();
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            Pipeline::new(writer.clone(), evaluator, case.offline).run(
                input.clone(),
                events,
                CancellationToken::new(),
            ),
        )
        .await
        .expect("Owned pipeline fixture must finish boundedly")
        .unwrap();
        let PipelineOutcome::Result(result) = outcome else {
            panic!("{} must complete", case.name);
        };
        assert_eq!(result.quality, case.quality, "{}", case.name);
        assert_eq!(
            (
                writer.calls.load(Ordering::SeqCst),
                checker
                    .as_ref()
                    .map_or(0, |checker| checker.calls.load(Ordering::SeqCst))
            ),
            case.calls,
            "{}",
            case.name
        );
        assert_eq!(result.chart.is_some(), case.chart, "{}", case.name);
        assert_eq!(
            result.repairs,
            usize::from(case.calls.0 == 2),
            "{}",
            case.name
        );
        if !case.chart {
            assert_eq!(result.text, PROSE, "{}", case.name);
        }
        persist_accepted(&input, result, case.label);
    }
}
