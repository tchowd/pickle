use crate::*;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
// One owned outcome exists per bounded request. Inline ownership avoids an allocation
// on the overwhelmingly common result path without increasing a queue or collection.
#[allow(clippy::large_enum_variant)]
pub enum PipelineOutcome {
    Result(ReadingResult),
    NeedsContext(Vec<String>),
    ChooseChart,
}
pub struct Pipeline {
    provider: Arc<dyn GenerativeProvider>,
    evaluator: Option<Arc<dyn DecisionClient>>,
    local_only: bool,
    pub policy: DecisionPolicy,
}
impl Pipeline {
    pub fn new(
        provider: Arc<dyn GenerativeProvider>,
        evaluator: Option<Arc<dyn DecisionClient>>,
        local_only: bool,
    ) -> Self {
        Self {
            provider,
            evaluator,
            local_only,
            policy: DecisionPolicy::default(),
        }
    }
    pub async fn run(
        &self,
        input: RequestInput,
        events: UnboundedSender<PipelineEvent>,
        cancel: CancellationToken,
    ) -> Result<PipelineOutcome> {
        input.validate()?;
        check_cancel(&cancel)?;
        if self.local_only && self.provider.is_remote() {
            return Err(PickleError::message(
                "Offline mode blocks inference on another computer or in the cloud.",
            ));
        }
        let started = Instant::now();
        let mut checker = if self.local_only || !self.provider.permits_cloud_checks() {
            None
        } else {
            self.evaluator.as_ref()
        };
        let mut unavailable = if self.local_only {
            QualityStatus::OFFLINE
        } else {
            QualityStatus::CHECKS_OFF
        };
        let mut evaluator_model = None;
        let mut notes = vec![];
        let mut difficulty = None;
        if input.limited {
            notes.push("Limited explanation: missing context may affect this answer.".into());
        }
        let mut state = DecisionState::from([(
            "source".into(),
            if input.action == ReadingAction::Chart {
                input.source()
            } else {
                input.reading_source()
            },
        )]);
        if input.action == ReadingAction::FollowUp {
            state.insert("follow_up_question".into(), input.question.clone());
        }
        if input.action != ReadingAction::Chart
            && let Some(client) = checker
        {
            progress(&events, "Checking the passage…");
            let mut questions = DecisionEvaluators::context();
            if input.action != ReadingAction::FollowUp {
                questions.extend(DecisionEvaluators::difficulty());
            }
            match evaluate(client, state.clone(), questions, &cancel).await {
                Ok(response) => {
                    evaluator_model = Some(response.model.clone());
                    let missing = self
                        .policy
                        .concerns(&response, &DecisionEvaluators::context());
                    if !missing.is_empty() && !input.limited {
                        return Ok(PipelineOutcome::NeedsContext(missing));
                    }
                    if self
                        .policy
                        .quality(&response, &DecisionEvaluators::context())
                        == QualityStatus::Uncertain
                    {
                        notes.push("Context assessment was inconclusive; supply surrounding text if needed.".into());
                    }
                    difficulty = self.policy.difficulty(&response);
                }
                Err(PickleError::Cancelled) => return Err(PickleError::Cancelled),
                Err(_) => {
                    checker = None;
                    unavailable = "Jev unavailable";
                    notes.push("Jev could not assess context or reading difficulty.".into());
                }
            }
        }
        let mut kind = None;
        if input.action == ReadingAction::Chart {
            progress(&events, "Choosing a supported visual…");
            if let Some(client) = checker {
                match evaluate(client, state.clone(), DecisionEvaluators::chart(), &cancel).await {
                    Ok(response) => {
                        evaluator_model = Some(response.model.clone());
                        kind = self.policy.chart_kind(&response);
                        if kind.is_none() {
                            let no_visual = response
                                .answers
                                .get("visual_justified")
                                .and_then(|a| a.noul)
                                .unwrap_or(0.5)
                                <= self.policy.low;
                            if no_visual {
                                return Ok(PipelineOutcome::Result(ReadingResult { id: Uuid::new_v4(), action: input.action, text: "This selection does not clearly support a bar chart or flow diagram. Try Explain more or add a passage with explicit comparable quantities or relationships.".into(), chart: None, quality: QualityStatus::Uncertain, model: "No generation requested".into(), evaluator_model, elapsed: started.elapsed().as_secs_f64(), repairs: 0, notes, timing: None }));
                            }
                        }
                    }
                    Err(PickleError::Cancelled) => return Err(PickleError::Cancelled),
                    Err(_) => {
                        checker = None;
                        unavailable = "Jev unavailable";
                    }
                }
            }
            kind = kind.or(input.chart_choice);
            let Some(chosen) = kind else {
                return Ok(PipelineOutcome::ChooseChart);
            };
            if chosen == ChartKind::Bar {
                let values = numeric_evidence(&input.source());
                if !values
                    .iter()
                    .any(|v| values.iter().filter(|other| other.unit == v.unit).count() >= 2)
                {
                    return Err(PickleError::message(
                        "A bar chart needs at least two explicit quantities with matching units. Bare numbers and dates are not enough.",
                    ));
                }
            }
        }
        progress(
            &events,
            match input.action {
                ReadingAction::Chart => "Preparing the chart…",
                ReadingAction::Simplify => "Simplifying…",
                ReadingAction::Expand => "Explaining…",
                ReadingAction::FollowUp => "Answering…",
            },
        );
        let mut prompt = make_prompt(&input, difficulty.as_deref(), kind)?;
        let mut generated = self
            .generate(prompt.clone(), events.clone(), &cancel)
            .await?;
        if kind.is_none() {
            generated.text = without_preamble(&generated.text);
        }
        validate_prose(&generated.text)?;
        if let Some(kind) = kind {
            let chart = validate_chart(&generated.text, kind, &input.source())?;
            notes.push(
                if checker.is_some() {
                    "Jev suggested this format. A model judgment is not proof of correctness."
                } else {
                    "Jev routing was not performed; format chosen by you."
                }
                .into(),
            );
            notes.push("Values come from explicit source quantities. Flow evidence is matched literally; relationship meaning still needs your review.".into());
            return Ok(PipelineOutcome::Result(ReadingResult {
                id: Uuid::new_v4(),
                action: input.action,
                text: chart.alternative(),
                chart: Some(chart),
                quality: QualityStatus::ChartValidated,
                model: generated.model,
                evaluator_model,
                elapsed: started.elapsed().as_secs_f64(),
                repairs: 0,
                notes,
                timing: generated.timing,
            }));
        }
        let mut quality = QualityStatus::NotChecked(unavailable.into());
        let mut repairs = 0;
        if let Some(client) = checker {
            let questions = if input.action == ReadingAction::Simplify {
                DecisionEvaluators::fidelity()
            } else {
                DecisionEvaluators::support()
            };
            progress(&events, "Reviewing…");
            state.insert("candidate".into(), generated.text.clone());
            let check_result: Result<()> = async {
                let response = evaluate(client, state.clone(), questions.clone(), &cancel).await?;
                evaluator_model = Some(response.model.clone());
                quality = self.policy.quality(&response, &questions);
                if let QualityStatus::Concerns(flags) = &quality {
                    repairs = 1;
                    progress(&events, "Refining…");
                    prompt.system.push_str("\nRevise your prior answer. ");
                    for flag in flags {
                        prompt.system.push_str(DecisionEvaluators::repair(flag));
                        prompt.system.push(' ');
                    }
                    prompt.user.push_str("\nPrior answer (untrusted draft):\n");
                    prompt.user.push_str(&generated.text);
                    // A repair must not stream an unchecked replacement over the original draft.
                    let (repair_events, _) = tokio::sync::mpsc::unbounded_channel();
                    let mut revised = self.generate(prompt, repair_events, &cancel).await?;
                    revised.text = without_preamble(&revised.text);
                    validate_prose(&revised.text)?;
                    generated = revised;
                    progress(&events, "Reviewing…");
                    state.insert("candidate".into(), generated.text.clone());
                    let response = evaluate(client, state.clone(), questions, &cancel).await?;
                    evaluator_model = Some(response.model.clone());
                    quality = self.policy.quality(
                        &response,
                        &if input.action == ReadingAction::Simplify {
                            DecisionEvaluators::fidelity()
                        } else {
                            DecisionEvaluators::support()
                        },
                    );
                }
                Ok(())
            }
            .await;
            match check_result {
                Err(PickleError::Cancelled) => return Err(PickleError::Cancelled),
                Err(_) => {
                    quality = QualityStatus::NotChecked("Jev check or repair failed".into());
                    notes.push(
                        "The full check could not finish. Review the result against your source."
                            .into(),
                    );
                }
                Ok(()) => {}
            }
        }
        check_cancel(&cancel)?;
        Ok(PipelineOutcome::Result(ReadingResult {
            id: Uuid::new_v4(),
            action: input.action,
            text: generated.text,
            chart: None,
            quality,
            model: generated.model,
            evaluator_model,
            elapsed: started.elapsed().as_secs_f64(),
            repairs,
            notes,
            timing: generated.timing,
        }))
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: UnboundedSender<PipelineEvent>,
        cancel: &CancellationToken,
    ) -> Result<Generation> {
        tokio::select! { biased; _ = cancel.cancelled() => Err(PickleError::Cancelled), result = self.provider.generate(prompt, events, cancel.clone()) => { check_cancel(cancel)?; result } }
    }
}
fn progress(events: &UnboundedSender<PipelineEvent>, message: &str) {
    let _ = events.send(PipelineEvent::Progress(message.into()));
}
async fn evaluate(
    client: &Arc<dyn DecisionClient>,
    state: DecisionState,
    questions: Questions,
    cancel: &CancellationToken,
) -> Result<DecisionResponse> {
    tokio::select! { biased; _ = cancel.cancelled() => Err(PickleError::Cancelled), result = client.evaluate(state, questions.clone(), cancel.clone()) => { check_cancel(cancel)?; let response = result?; response.validate(&questions)?; Ok(response) } }
}
