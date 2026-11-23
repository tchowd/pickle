//! Causal counterpart of StreamingTests.testDraftRemainsVisibleThroughoutRepair.
//! Gates keep the real pipeline in repair/review so retention is checked before
//! completion, rather than inferred from the final result or a drained channel.
use async_trait::async_trait;
use pickle_core::*;
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::{
    Notify, Semaphore,
    mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};
use tokio_util::sync::CancellationToken;

const INITIAL: &str = "Initial answer";
const REVISED: &str = "Revised answer";

struct StreamingRepairWriter {
    fail_repair: bool,
    prompts: Mutex<Vec<GenerationPrompt>>,
    repair_draft_attempts: AtomicUsize,
    repair_started: Notify,
    finish_repair: Semaphore,
}
#[async_trait]
impl GenerativeProvider for StreamingRepairWriter {
    fn is_remote(&self) -> bool {
        false
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: UnboundedSender<PipelineEvent>,
        _: CancellationToken,
    ) -> Result<Generation> {
        let call = {
            let mut prompts = self.prompts.lock().unwrap();
            prompts.push(prompt);
            prompts.len()
        };
        match call {
            1 => {
                events.send(PipelineEvent::Draft(INITIAL.into())).unwrap();
                Ok(Generation::new(INITIAL, "streaming-fixture"))
            }
            2 => {
                // Deliberately try to stream unchecked repair content even on
                // the failure path. A pipeline regression forwarding this sender
                // would overwrite the original before the gate is released.
                for draft in ["Unchecked repair prefix", REVISED] {
                    self.repair_draft_attempts.fetch_add(1, Ordering::SeqCst);
                    let _ = events.send(PipelineEvent::Draft(draft.into()));
                }
                self.repair_started.notify_one();
                self.finish_repair.acquire().await.unwrap().forget();
                if self.fail_repair {
                    Err(PickleError::message("Repair unavailable"))
                } else {
                    Ok(Generation::new(REVISED, "streaming-fixture"))
                }
            }
            _ => panic!("A repair must never cause a third generation"),
        }
    }
}

struct RepairChecks {
    calls: Mutex<Vec<(DecisionState, Questions)>>,
    recheck_started: Notify,
    finish_recheck: Semaphore,
}
impl Default for RepairChecks {
    fn default() -> Self {
        Self {
            calls: Mutex::new(vec![]),
            recheck_started: Notify::new(),
            finish_recheck: Semaphore::new(0),
        }
    }
}
#[async_trait]
impl DecisionClient for RepairChecks {
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
        if state.get("candidate").map(String::as_str) == Some(REVISED) {
            self.recheck_started.notify_one();
            self.finish_recheck.acquire().await.unwrap().forget();
        }
        let answers = questions
            .iter()
            .map(|(key, question)| {
                let answer = if question.kind == "choice" {
                    DecisionAnswer {
                        kind: "choice".into(),
                        noul: None,
                        choice: Some("specialist".into()),
                        confidence: Some(0.95),
                        probabilities: Some(BTreeMap::from([
                            ("everyday".into(), 0.0),
                            ("professional".into(), 0.0),
                            ("specialist".into(), 1.0),
                            ("dense".into(), 0.0),
                        ])),
                    }
                } else {
                    DecisionAnswer::noul(
                        if key == "lost_uncertainty"
                            && state.get("candidate").map(String::as_str) == Some(INITIAL)
                        {
                            0.99
                        } else {
                            0.01
                        },
                    )
                };
                (key.clone(), answer)
            })
            .collect();
        Ok(DecisionResponse {
            model: "repair-check-fixture".into(),
            answers,
            usage: None,
        })
    }
}

#[derive(Default)]
struct Preview {
    events: Vec<String>,
    draft: String,
}
impl Preview {
    fn receive_available(&mut self, receiver: &mut UnboundedReceiver<PipelineEvent>) {
        while let Ok(event) = receiver.try_recv() {
            match event {
                PipelineEvent::Draft(text) => {
                    self.events.push(format!("draft:{text}"));
                    self.draft = text;
                }
                PipelineEvent::Progress(text) => self.events.push(format!("status:{text}")),
            }
        }
    }
}

async fn assert_streamed_draft_retention(fail_repair: bool) {
    let writer = Arc::new(StreamingRepairWriter {
        fail_repair,
        prompts: Mutex::new(vec![]),
        repair_draft_attempts: AtomicUsize::new(0),
        repair_started: Notify::new(),
        finish_repair: Semaphore::new(0),
    });
    let checker = Arc::new(RepairChecks::default());
    let pipeline = Pipeline::new(writer.clone(), Some(checker.clone()), false);
    let input = RequestInput::new(
        SelectionSnapshot::new("Treatment may help.", "Test", "test"),
        ReadingAction::Simplify,
    );
    let (sender, mut receiver) = unbounded_channel();
    let task =
        tokio::spawn(async move { pipeline.run(input, sender, CancellationToken::new()).await });
    writer.repair_started.notified().await;
    assert!(!task.is_finished(), "Repair is held before completion");
    assert_eq!(writer.repair_draft_attempts.load(Ordering::SeqCst), 2);
    let mut preview = Preview::default();
    preview.receive_available(&mut receiver);
    let mut expected = vec![
        "status:Checking the passage…",
        "status:Simplifying…",
        "draft:Initial answer",
        "status:Reviewing…",
        "status:Refining…",
    ];
    assert_eq!(preview.events, expected);
    assert_eq!(preview.draft, INITIAL, "Refining retains the initial draft");

    writer.finish_repair.add_permits(1);
    if !fail_repair {
        checker.recheck_started.notified().await;
        assert!(!task.is_finished(), "Revised answer is still under review");
        preview.receive_available(&mut receiver);
        expected.push("status:Reviewing…");
        assert_eq!(preview.events, expected);
        assert_eq!(
            preview.draft, INITIAL,
            "Rechecking retains the initial draft"
        );
        checker.finish_recheck.add_permits(1);
    }
    let PipelineOutcome::Result(result) = task.await.unwrap().unwrap() else {
        panic!("Expected a completed explanation");
    };
    preview.receive_available(&mut receiver);
    assert_eq!(
        preview.events, expected,
        "No repair draft reaches the caller"
    );
    assert_eq!(preview.draft, INITIAL);
    assert_eq!(result.text, if fail_repair { INITIAL } else { REVISED });
    assert_eq!(result.repairs, 1);
    assert_eq!(
        result.quality,
        if fail_repair {
            QualityStatus::NotChecked("Jev check or repair failed".into())
        } else {
            QualityStatus::Checked
        }
    );
    let prompts = writer.prompts.lock().unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].system.contains("Restore all uncertainty"));
    assert!(prompts[1].user.ends_with(INITIAL));
    let calls = checker.calls.lock().unwrap();
    assert_eq!(calls.len(), if fail_repair { 2 } else { 3 });
    assert_eq!(calls[1].0["candidate"], INITIAL);
    assert!(calls[1].1.keys().eq(DecisionEvaluators::fidelity().keys()));
    if !fail_repair {
        assert_eq!(calls[2].0["candidate"], REVISED);
        assert!(calls[2].1.keys().eq(calls[1].1.keys()));
    }
}

#[tokio::test]
async fn streamed_draft_survives_successful_repair_and_recheck_in_order() {
    tokio::time::timeout(
        Duration::from_secs(5),
        assert_streamed_draft_retention(false),
    )
    .await
    .expect("Deterministic repair/recheck gates must complete");
}

#[tokio::test]
async fn streamed_draft_survives_failed_repair_in_order() {
    tokio::time::timeout(
        Duration::from_secs(5),
        assert_streamed_draft_retention(true),
    )
    .await
    .expect("Deterministic failed-repair gate must complete");
}
