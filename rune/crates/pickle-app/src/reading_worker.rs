//! Prepared reading publication only; provider preparation/vault access stays in
//! controller::start_reading. The sink schedules owned typed UI updates.
use super::Pending;
use pickle_core::{Pipeline, PipelineEvent, PipelineOutcome, RequestInput, Result};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) struct ReadingAttempt {
    pub id: u64,
    pub input: RequestInput,
    pub pending: Pending,
    pub record_at_start: bool,
}
pub(super) struct FinishedReading {
    pub attempt: ReadingAttempt,
    pub outcome: Result<PipelineOutcome>,
}
pub(super) enum ReadingUpdate {
    Progress { id: u64, message: String },
    Draft { id: u64, text: String },
    PreparationFailed { id: u64, message: String },
    Finished(Box<FinishedReading>),
}
impl ReadingUpdate {
    pub fn id(&self) -> u64 {
        match self {
            Self::Progress { id, .. }
            | Self::Draft { id, .. }
            | Self::PreparationFailed { id, .. } => *id,
            Self::Finished(finished) => finished.attempt.id,
        }
    }
}
pub(super) type UpdateSink = Arc<dyn Fn(ReadingUpdate) + Send + Sync>;

pub(super) async fn run_prepared(
    pipeline: Pipeline,
    attempt: ReadingAttempt,
    cancel: CancellationToken,
    sink: UpdateSink,
) {
    let id = attempt.id;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut latest_draft = None;
    let mut tick = tokio::time::interval(Duration::from_millis(80));
    let work = pipeline.run(attempt.input.clone(), sender, cancel.clone());
    tokio::pin!(work);
    let outcome = loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            event = receiver.recv() => match event {
                Some(PipelineEvent::Draft(text)) => latest_draft = Some(text),
                Some(PipelineEvent::Progress(message)) => sink(ReadingUpdate::Progress { id, message }),
                None => {},
            },
            _ = tick.tick() => if let Some(text) = latest_draft.take() { sink(ReadingUpdate::Draft { id, text }); },
            outcome = &mut work => break outcome,
        }
    };
    sink(ReadingUpdate::Finished(Box::new(FinishedReading {
        attempt,
        outcome,
    })));
}
