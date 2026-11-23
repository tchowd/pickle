use async_trait::async_trait;
use pickle_core::*;
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

struct Writer {
    calls: AtomicUsize,
    remote: bool,
    cloud_checks: bool,
    prompts: Mutex<Vec<GenerationPrompt>>,
    outputs: Vec<String>,
    delay: bool,
}
impl Writer {
    fn new(outputs: &[&str]) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            remote: false,
            cloud_checks: true,
            prompts: Mutex::new(vec![]),
            outputs: outputs.iter().map(|s| s.to_string()).collect(),
            delay: false,
        }
    }
}
#[async_trait]
impl GenerativeProvider for Writer {
    fn is_remote(&self) -> bool {
        self.remote
    }
    fn permits_cloud_checks(&self) -> bool {
        self.cloud_checks
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: UnboundedSender<PipelineEvent>,
        _: CancellationToken,
    ) -> Result<Generation> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        self.prompts.lock().unwrap().push(prompt);
        let _ = events.send(PipelineEvent::Draft("draft".into()));
        if self.delay {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        Ok(Generation::new(
            self.outputs.get(n).unwrap_or(&self.outputs[0]),
            "fixture",
        ))
    }
}
struct Checker {
    calls: Mutex<Vec<(DecisionState, Questions)>>,
    concern: bool,
    missing: bool,
    outage: bool,
    post_outage: bool,
    uncertain: bool,
    repair_succeeds: bool,
    chart_yes: bool,
}
impl Checker {
    fn new() -> Self {
        Self {
            calls: Mutex::new(vec![]),
            concern: false,
            missing: false,
            outage: false,
            post_outage: false,
            uncertain: false,
            repair_succeeds: false,
            chart_yes: false,
        }
    }
}
#[async_trait]
impl DecisionClient for Checker {
    async fn evaluate(
        &self,
        state: DecisionState,
        questions: Questions,
        _: CancellationToken,
    ) -> Result<DecisionResponse> {
        self.calls
            .lock()
            .unwrap()
            .push((state.clone(), questions.clone()));
        if self.outage || (self.post_outage && state.contains_key("candidate")) {
            return Err(PickleError::message("offline"));
        }
        let candidate = state.get("candidate");
        let answers = questions
            .iter()
            .map(|(k, q)| {
                let answer = if q.kind == "choice" {
                    DecisionAnswer {
                        kind: "choice".into(),
                        noul: None,
                        choice: Some("everyday".into()),
                        confidence: Some(0.9),
                        probabilities: Some(BTreeMap::from([
                            ("everyday".into(), 0.9),
                            ("professional".into(), 0.1),
                            ("specialist".into(), 0.0),
                            ("dense".into(), 0.0),
                        ])),
                    }
                } else {
                    DecisionAnswer::noul(
                        if self.chart_yes
                            || (self.concern
                                && candidate.is_some()
                                && k == "lost_uncertainty"
                                && !(self.repair_succeeds
                                    && candidate.map(String::as_str) == Some("second")))
                            || (self.missing && candidate.is_none() && k == "missing_referent")
                        {
                            0.9
                        } else if self.uncertain && candidate.is_some() {
                            0.5
                        } else {
                            0.1
                        },
                    )
                };
                (k.clone(), answer)
            })
            .collect();
        Ok(DecisionResponse {
            model: "jev-fixture".into(),
            answers,
            usage: None,
        })
    }
}
fn input(action: ReadingAction) -> RequestInput {
    RequestInput::new(
        SelectionSnapshot::new(
            "It may help some people, but evidence is limited.",
            "Fixture",
            "fixture",
        ),
        action,
    )
}
async fn run(
    writer: Arc<Writer>,
    checker: Option<Arc<Checker>>,
    input: RequestInput,
) -> PipelineOutcome {
    let (events, _) = unbounded_channel();
    Pipeline::new(writer, checker.map(|c| c as Arc<dyn DecisionClient>), false)
        .run(input, events, CancellationToken::new())
        .await
        .unwrap()
}
#[tokio::test]
async fn simplify_checks_context_difficulty_and_fidelity_only() {
    let writer = Arc::new(Writer::new(&[
        "It might help some people; evidence is limited.",
    ]));
    let checker = Arc::new(Checker::new());
    let PipelineOutcome::Result(result) = run(
        writer,
        Some(checker.clone()),
        input(ReadingAction::Simplify),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(result.quality, QualityStatus::Checked);
    let calls = checker.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].1.contains_key("difficulty"));
    assert!(calls[1].1.contains_key("lost_uncertainty"));
    assert!(!calls[1].1.contains_key("invented_specifics"));
}
#[tokio::test]
async fn expansion_and_followup_check_support_against_the_fixed_source() {
    for action in [ReadingAction::Expand, ReadingAction::FollowUp] {
        let writer = Arc::new(Writer::new(&["Explanation"]));
        let checker = Arc::new(Checker::new());
        let mut request = input(action);
        if action == ReadingAction::FollowUp {
            request.question = "Why?".into();
        }
        request.previous_result = "An older answer, not evidence".into();
        let fixed_source = request.reading_source();
        let PipelineOutcome::Result(result) =
            run(writer.clone(), Some(checker.clone()), request).await
        else {
            panic!("Expected a checked explanation");
        };
        assert_eq!(result.quality, QualityStatus::Checked);
        let calls = checker.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].1.contains_key("difficulty"),
            action == ReadingAction::Expand
        );
        assert!(calls[1].1.contains_key("invented_specifics"));
        assert!(!calls[1].1.contains_key("lost_uncertainty"));
        for (state, _) in calls.iter() {
            assert_eq!(state["source"], fixed_source);
        }
        let prompts = writer.prompts.lock().unwrap();
        let body: serde_json::Value = serde_json::from_str(&prompts[0].user).unwrap();
        assert_eq!(body["source"], fixed_source);
        assert_eq!(body["prior_explanation"], "An older answer, not evidence");
    }
}
#[tokio::test]
async fn missing_context_stops_before_generation_unless_explicitly_limited() {
    let writer = Arc::new(Writer::new(&["Limited explanation"]));
    let mut c = Checker::new();
    c.missing = true;
    let checker = Arc::new(c);
    assert!(matches!(
        run(
            writer.clone(),
            Some(checker.clone()),
            input(ReadingAction::Simplify)
        )
        .await,
        PipelineOutcome::NeedsContext(_)
    ));
    assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
    let mut request = input(ReadingAction::Simplify);
    request.limited = true;
    assert!(matches!(
        run(writer.clone(), Some(checker), request).await,
        PipelineOutcome::Result(_)
    ));
    assert!(
        writer.prompts.lock().unwrap()[0]
            .system
            .contains("Context may be missing")
    );
}
#[tokio::test]
async fn concern_repairs_once_rechecks_and_keeps_remaining_concerns() {
    let writer = Arc::new(Writer::new(&["first", "second"]));
    let mut c = Checker::new();
    c.concern = true;
    let checker = Arc::new(c);
    let PipelineOutcome::Result(result) = run(
        writer.clone(),
        Some(checker.clone()),
        input(ReadingAction::Simplify),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(writer.calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.repairs, 1);
    assert!(matches!(result.quality, QualityStatus::Concerns(_)));
    let calls = checker.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert!(calls[1].1.keys().eq(calls[2].1.keys()));
}
#[tokio::test]
async fn invalid_repair_keeps_valid_original_with_unchecked_status() {
    let writer = Arc::new(Writer::new(&["Original valid answer", ""]));
    let mut c = Checker::new();
    c.concern = true;
    let PipelineOutcome::Result(result) =
        run(writer, Some(Arc::new(c)), input(ReadingAction::Simplify)).await
    else {
        panic!()
    };
    assert_eq!(result.text, "Original valid answer");
    assert!(matches!(result.quality, QualityStatus::NotChecked(_)));
}
#[tokio::test]
async fn evaluator_outage_still_generates_with_honest_status() {
    let writer = Arc::new(Writer::new(&["Answer"]));
    let mut c = Checker::new();
    c.outage = true;
    let PipelineOutcome::Result(result) =
        run(writer, Some(Arc::new(c)), input(ReadingAction::Simplify)).await
    else {
        panic!()
    };
    assert_eq!(
        result.quality,
        QualityStatus::NotChecked("Jev unavailable".into())
    );
    assert!(result.quality.visible_label().is_some());
}
#[tokio::test]
async fn local_provider_never_calls_cloud_checks() {
    let mut w = Writer::new(&["Local answer"]);
    w.cloud_checks = false;
    let checker = Arc::new(Checker::new());
    let PipelineOutcome::Result(result) = run(
        Arc::new(w),
        Some(checker.clone()),
        input(ReadingAction::Simplify),
    )
    .await
    else {
        panic!()
    };
    assert!(checker.calls.lock().unwrap().is_empty());
    assert!(result.quality.visible_label().is_none());
}
#[tokio::test]
async fn offline_blocks_remote_before_any_generation() {
    let mut w = Writer::new(&["Remote answer"]);
    w.remote = true;
    let writer = Arc::new(w);
    let (events, _) = unbounded_channel();
    assert!(
        Pipeline::new(writer.clone(), None, true)
            .run(
                input(ReadingAction::Simplify),
                events,
                CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn cancellation_drops_noncooperative_provider_future() {
    let mut w = Writer::new(&["Late answer"]);
    w.delay = true;
    let writer = Arc::new(w);
    let (events, _) = unbounded_channel();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        signal.cancel();
    });
    let result = Pipeline::new(writer, None, false)
        .run(input(ReadingAction::Simplify), events, cancel)
        .await;
    assert!(matches!(result, Err(PickleError::Cancelled)));
}
#[tokio::test]
async fn oversize_input_and_unsupported_bars_never_reach_provider() {
    let writer = Arc::new(Writer::new(&["Answer"]));
    let (events, _) = unbounded_channel();
    let mut request = input(ReadingAction::Simplify);
    request.snapshot.text = "a".repeat(12001);
    assert!(
        Pipeline::new(writer.clone(), None, false)
            .run(request, events, CancellationToken::new())
            .await
            .is_err()
    );
    let mut chart = input(ReadingAction::Chart);
    chart.chart_choice = Some(ChartKind::Bar);
    let (events, _) = unbounded_channel();
    assert!(
        Pipeline::new(writer.clone(), None, false)
            .run(chart, events, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn chart_without_checker_requires_explicit_format_choice() {
    let writer = Arc::new(Writer::new(&["Answer"]));
    assert!(matches!(
        run(writer.clone(), None, input(ReadingAction::Chart)).await,
        PipelineOutcome::ChooseChart
    ));
    assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn successful_repair_rechecks_entire_bank_and_restores_checked_status() {
    let writer = Arc::new(Writer::new(&["first", "second"]));
    let mut checker = Checker::new();
    checker.concern = true;
    checker.repair_succeeds = true;
    let checker = Arc::new(checker);
    let PipelineOutcome::Result(result) = run(
        writer,
        Some(checker.clone()),
        input(ReadingAction::Simplify),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(result.quality, QualityStatus::Checked);
    assert_eq!(result.text, "second");
    assert_eq!(result.repairs, 1);
    assert_eq!(
        checker.calls.lock().unwrap()[2].1.len(),
        DecisionEvaluators::fidelity().len()
    );
}
#[tokio::test]
async fn uncertain_checks_do_not_trigger_repairs() {
    let writer = Arc::new(Writer::new(&["Answer"]));
    let mut checker = Checker::new();
    checker.uncertain = true;
    let PipelineOutcome::Result(result) = run(
        writer.clone(),
        Some(Arc::new(checker)),
        input(ReadingAction::Simplify),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(result.quality, QualityStatus::Uncertain);
    assert_eq!(result.repairs, 0);
    assert_eq!(writer.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn failed_postcheck_never_becomes_a_pass() {
    let writer = Arc::new(Writer::new(&["Answer"]));
    let mut checker = Checker::new();
    checker.post_outage = true;
    let PipelineOutcome::Result(result) = run(
        writer,
        Some(Arc::new(checker)),
        input(ReadingAction::Expand),
    )
    .await
    else {
        panic!()
    };
    assert!(matches!(result.quality, QualityStatus::NotChecked(_)));
    assert!(result.quality.visible_label().is_some());
}
#[tokio::test]
async fn evaluator_can_refuse_an_unsupported_chart_before_generation() {
    let writer = Arc::new(Writer::new(&["unused"]));
    let mut request = input(ReadingAction::Chart);
    request.chart_choice = Some(ChartKind::Flow);
    let PipelineOutcome::Result(result) =
        run(writer.clone(), Some(Arc::new(Checker::new())), request).await
    else {
        panic!()
    };
    assert!(result.chart.is_none());
    assert_eq!(result.model, "No generation requested");
    assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn evaluator_chart_routing_still_requires_deterministic_validation() {
    let source = "Revenue was $10 and $20.";
    let values = numeric_evidence(source);
    let answer=serde_json::json!({"type":"bar_chart","bars":[{"evidenceID":values[0].id},{"evidenceID":values[1].id}]}).to_string();
    let writer = Arc::new(Writer::new(&[&answer]));
    let mut checker = Checker::new();
    checker.chart_yes = true;
    let request = RequestInput::new(
        SelectionSnapshot::new(source, "Fixture", "fixture"),
        ReadingAction::Chart,
    );
    let PipelineOutcome::Result(result) =
        run(writer.clone(), Some(Arc::new(checker)), request).await
    else {
        panic!()
    };
    assert_eq!(result.quality, QualityStatus::ChartValidated);
    assert!(result.chart.is_some());
    assert!(writer.prompts.lock().unwrap()[0].structured);
}
#[tokio::test]
async fn context_and_reading_preferences_are_preserved_in_prompts() {
    let writer = Arc::new(Writer::new(&["Answer"]));
    let mut request = input(ReadingAction::Expand);
    request.level = ReadingLevel::Technical;
    request.context = "Additional source".into();
    request.page_context = "Visible heading".into();
    request.visual_context = "Possible picture relationship".into();
    run(writer.clone(), Some(Arc::new(Checker::new())), request).await;
    let prompts = writer.prompts.lock().unwrap();
    let prompt = &prompts[0];
    assert!(prompt.system.contains("Keep technical detail"));
    assert!(!prompt.system.contains("Source language category"));
    let body: serde_json::Value = serde_json::from_str(&prompt.user).unwrap();
    assert!(
        body["source"]
            .as_str()
            .unwrap()
            .contains("Additional source")
    );
    assert!(body["source"].as_str().unwrap().contains("automatic OCR"));
    assert!(
        !body["source"]
            .as_str()
            .unwrap()
            .contains("Possible picture")
    );
    assert_eq!(
        body["visual_context_summary"],
        "Possible picture relationship"
    );
}
#[tokio::test]
async fn invalid_chart_payload_fails_instead_of_becoming_completed() {
    let writer = Arc::new(Writer::new(&[
        r#"{"type":"bar_chart","bars":[{"evidenceID":"invented1"},{"evidenceID":"invented2"}]}"#,
    ]));
    let mut request = RequestInput::new(
        SelectionSnapshot::new("$10 and $20", "Fixture", "fixture"),
        ReadingAction::Chart,
    );
    request.chart_choice = Some(ChartKind::Bar);
    let (events, _) = unbounded_channel();
    assert!(
        Pipeline::new(writer, None, false)
            .run(request, events, CancellationToken::new())
            .await
            .is_err()
    );
}
