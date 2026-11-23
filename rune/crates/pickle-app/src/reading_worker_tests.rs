//! Real prepared worker + typed queue + Rune Runtime update boundary.
//! Provider preparation/vault and Rune's native queue are deliberately outside
//! this boundary. Both production and these tests apply the same typed updates.
use super::*;
use async_trait::async_trait;
use rune::{Env, Runtime};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};
use tokio::{
    sync::{Semaphore, oneshot},
    task::JoinHandle,
};

const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum Answer {
    Success(&'static str),
    Failure,
}
impl Answer {
    fn result(self) -> Result<Generation> {
        match self {
            Self::Success(text) => Ok(Generation::new(text, "owned-fixture")),
            Self::Failure => Err(PickleError::message("Controlled old failure")),
        }
    }
}
struct Producer {
    initial: &'static str,
    late: &'static str,
    answer: Answer,
    release: Semaphore,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    draft_attempts: AtomicU64,
    result_attempts: AtomicU64,
    draft_sent: AtomicBool,
    result_sent: AtomicBool,
    done: AtomicBool,
}
impl Producer {
    fn new(initial: &'static str, late: &'static str, answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            initial,
            late,
            answer,
            release: Semaphore::new(0),
            tasks: Mutex::new(vec![]),
            draft_attempts: AtomicU64::new(0),
            result_attempts: AtomicU64::new(0),
            draft_sent: AtomicBool::new(false),
            result_sent: AtomicBool::new(false),
            done: AtomicBool::new(false),
        })
    }
    fn finish(&self) {
        self.release.add_permits(1);
    }
}
struct UncooperativeWriter(Arc<Producer>);
#[async_trait]
impl GenerativeProvider for UncooperativeWriter {
    fn is_remote(&self) -> bool {
        false
    }
    async fn generate(
        &self,
        _: GenerationPrompt,
        events: tokio::sync::mpsc::UnboundedSender<PipelineEvent>,
        _: CancellationToken,
    ) -> Result<Generation> {
        let (result, received) = oneshot::channel();
        let producer = self.0.clone();
        // Intentionally escape the provider future. This owned task ignores
        // cancellation and retains BOTH sinks after that future is dropped.
        let task = tokio::spawn(async move {
            let _ = events.send(PipelineEvent::Draft(producer.initial.into()));
            producer.release.acquire().await.unwrap().forget();
            producer.draft_attempts.fetch_add(1, Ordering::SeqCst);
            producer.draft_sent.store(
                events
                    .send(PipelineEvent::Draft(producer.late.into()))
                    .is_ok(),
                Ordering::SeqCst,
            );
            producer.result_attempts.fetch_add(1, Ordering::SeqCst);
            producer.result_sent.store(
                result.send(producer.answer.result()).is_ok(),
                Ordering::SeqCst,
            );
            producer.done.store(true, Ordering::SeqCst);
        });
        self.0.tasks.lock().unwrap().push(task);
        received
            .await
            .map_err(|_| PickleError::message("Owned producer stopped"))?
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Progress,
    Draft,
    Finished,
    PreparationFailed,
}
fn kind(update: &ReadingUpdate) -> Kind {
    match update {
        ReadingUpdate::Progress { .. } => Kind::Progress,
        ReadingUpdate::Draft { .. } => Kind::Draft,
        ReadingUpdate::Finished(_) => Kind::Finished,
        ReadingUpdate::PreparationFailed { .. } => Kind::PreparationFailed,
    }
}
struct Worker {
    id: u64,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}
struct Cleanup {
    joined: bool,
    unfinished_blocking: usize,
    history: std::result::Result<Vec<Entry>, String>,
}
struct Harness {
    directory: tempfile::TempDir,
    repository: Arc<Repository>,
    services: Arc<Services>,
    owner: Option<crate::service_runtime::RuntimeOwner>,
    _app: rune::App<AppState>,
    runtime: Runtime<AppState>,
    sink: UpdateSink,
    receiver: mpsc::Receiver<ReadingUpdate>,
    queued: VecDeque<ReadingUpdate>,
    applied: Vec<(u64, Kind, bool)>,
    workers: Vec<Worker>,
    producers: Vec<Arc<Producer>>,
}
impl Harness {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = Arc::new(Repository::new(directory.path().into()).unwrap());
        let settings = Settings {
            history_enabled: true,
            ..Settings::default()
        };
        let (services, owner) = Services::new(Some(repository.clone()), &settings).unwrap();
        let services = Arc::new(services);
        // Only a queue handle for archive completion; no native event loop.
        // Reading publications use the same reducer via the typed sink below.
        let app = rune::App::new(rune::AppSpec::new(
            "Owned reading fixture",
            (560.0, 680.0),
            AppState::default,
            |_, _| rune::el(),
        ));
        let state = AppState {
            services: Some(services.clone()),
            handle: Some(app.handle()),
            settings,
            source: TextEdit::new("Sample"),
            ..AppState::default()
        };
        let mut runtime = Runtime::new(
            state,
            |_, _| rune::el(),
            (560.0, 680.0),
            Env::new(true, 1.0),
        );
        runtime.update(|state, _| state.commit_source());
        let (sender, receiver) = mpsc::channel();
        let sink: UpdateSink = Arc::new(move |update| {
            let _ = sender.send(update);
        });
        Self {
            directory,
            repository,
            services,
            owner: Some(owner),
            _app: app,
            runtime,
            sink,
            receiver,
            queued: VecDeque::new(),
            applied: vec![],
            workers: vec![],
            producers: vec![],
        }
    }
    fn start(
        &mut self,
        producer: Arc<Producer>,
        pending: Pending,
        reference: Option<WebReference>,
    ) -> u64 {
        let mut admitted = None;
        self.runtime.update(|state, _| {
            // Same invalidation and admission methods as start_reading, after
            // its preparation-independent validation/consent boundary.
            state.cancel_request();
            let mut input = RequestInput::new(state.snapshot.clone().unwrap(), pending.action);
            input.question = pending.question.clone();
            input.reference = reference;
            admitted = Some(state.enter_running(input, pending, state.settings.history_enabled));
        });
        let (attempt, cancel) = admitted.unwrap();
        let id = attempt.id;
        let pipeline = Pipeline::new(Arc::new(UncooperativeWriter(producer.clone())), None, false);
        let task = self.services.runtime.spawn(run_prepared(
            pipeline,
            attempt,
            cancel.clone(),
            self.sink.clone(),
        ));
        self.workers.push(Worker { id, cancel, task });
        self.producers.push(producer);
        id
    }
    fn wait_for(&mut self, id: u64, expected: Kind) {
        let deadline = Instant::now() + DEADLINE;
        while !self
            .queued
            .iter()
            .any(|update| update.id() == id && kind(update) == expected)
        {
            let update = self
                .receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("Actual reading worker must publish the gated update");
            self.queued.push_back(update);
            assert!(self.queued.len() <= 64, "Fixture update count is bounded");
        }
    }
    fn drain(&mut self) {
        self.queued.extend(self.receiver.try_iter());
        while let Some(update) = self.queued.pop_front() {
            let (id, tag) = (update.id(), kind(&update));
            let mut applied = false;
            self.runtime
                .update(|state, _| applied = state.apply_reading_update(update));
            self.applied.push((id, tag, applied));
        }
    }
    fn accepted(&self, id: u64, tag: Kind) -> usize {
        self.applied
            .iter()
            .filter(|&&(current, kind, applied)| current == id && kind == tag && applied)
            .count()
    }
    fn wait_stopped(&self, id: u64) {
        let task = &self
            .workers
            .iter()
            .find(|worker| worker.id == id)
            .unwrap()
            .task;
        wait(|| task.is_finished());
    }
    fn cleanup(&mut self) -> Cleanup {
        self.runtime.update(|state, _| state.cancel());
        for worker in &self.workers {
            worker.cancel.cancel();
        }
        for producer in &self.producers {
            producer.finish();
        }
        let workers = std::mem::take(&mut self.workers);
        let producers = self.producers.clone();
        let (done, joined) = mpsc::sync_channel(1);
        self.services.runtime.spawn(async move {
            let mut complete = true;
            for worker in workers {
                complete &= worker.task.await.is_ok();
            }
            // Worker completion precedes taking producer handles: no late
            // provider invocation can add an unowned task after this point.
            for producer in producers {
                let tasks = std::mem::take(&mut *producer.tasks.lock().unwrap());
                for task in tasks {
                    complete &= task.await.is_ok();
                }
            }
            let _ = done.send(complete);
        });
        let joined = joined.recv_timeout(DEADLINE).ok() == Some(true);
        self.services.shutdown.cancel();
        let stopped = self.owner.take().unwrap().shutdown(DEADLINE);
        if !stopped.finished {
            // Never unwind/drop the temporary archive while a writer survives.
            eprintln!("Owned reading fixture runtime did not shut down.");
            std::process::exit(1);
        }
        Cleanup {
            joined,
            unfinished_blocking: stopped.unfinished_blocking,
            history: self.repository.history(),
        }
    }
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while !predicate() {
        assert!(Instant::now() < deadline, "Owned gated task must finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn check(
    test: impl FnOnce(&mut Harness) -> Vec<(&'static str, &'static str, Option<&'static str>)>,
) {
    // RuntimeOwner's production panic policy is fail-closed. Keep it OUTSIDE
    // the caught test body, clean up first, and only then resume any assertion.
    let mut harness = Harness::new();
    let result = catch_unwind(AssertUnwindSafe(|| test(&mut harness)));
    let cleanup = harness.cleanup();
    let expected = match result {
        Ok(expected) => expected,
        Err(panic) => {
            if !cleanup.joined || cleanup.unfinished_blocking != 0 || cleanup.history.is_err() {
                eprintln!("Owned reading fixture cleanup also failed.");
            }
            resume_unwind(panic)
        }
    };
    assert!(
        cleanup.joined,
        "Every owned reading/producer task must join successfully"
    );
    assert_eq!(cleanup.unfinished_blocking, 0);
    let history = cleanup.history.unwrap();
    assert_eq!(history.len(), expected.len());
    for (entry, (answer, action, question)) in history.iter().zip(expected) {
        assert_eq!(entry.answer, answer);
        assert_eq!(entry.action, action);
        assert_eq!(entry.question.as_deref(), question);
    }
    // Retained handles, queued archive UI callbacks and temporary files are all
    // released only after executor/writer shutdown above. No native queue claim.
    assert!(harness.directory.path().is_dir());
}
fn pointer(path: &str) -> WebReference {
    WebReference {
        url: format!("https://example.com/{path}"),
        title: path.into(),
        kind: "article".into(),
        text: "Synthetic reference body.".into(),
        timestamp: None,
    }
}

#[test]
fn queued_old_result_and_draft_cannot_replace_a_newer_reading_or_clear_its_busy_flag() {
    check(|h| {
        let old = Producer::new(
            "Old queued draft",
            "Old final draft",
            Answer::Success("Old response"),
        );
        let old_id = h.start(
            old.clone(),
            Pending::action(ReadingAction::Simplify),
            Some(pointer("old")),
        );
        h.wait_for(old_id, Kind::Draft); // actual 80ms coalescing, not a manufactured update
        old.finish();
        h.wait_for(old_id, Kind::Finished);
        h.wait_stopped(old_id);
        let new = Producer::new(
            "New draft",
            "New final draft",
            Answer::Success("New response"),
        );
        let new_id = h.start(
            new.clone(),
            Pending::action(ReadingAction::Simplify),
            Some(pointer("new")),
        );
        h.drain(); // old completion was queued BEFORE supersession, applied AFTER
        assert_eq!(h.accepted(old_id, Kind::Finished), 0);
        assert_eq!(h.accepted(old_id, Kind::Draft), 0);
        assert!(h.services.busy.load(Ordering::Acquire));
        assert!(matches!(h.runtime.state.activity, Activity::Running { id, .. } if id == new_id));
        assert!(h.runtime.state.result.is_none());
        new.finish();
        h.wait_for(new_id, Kind::Finished);
        h.drain();
        assert_eq!(h.runtime.state.answer(), "New response");
        assert_eq!(h.accepted(new_id, Kind::Finished), 1);
        assert!(!h.services.busy.load(Ordering::Acquire));
        assert_eq!(
            h.runtime
                .state
                .current_answer_entry()
                .unwrap()
                .reference
                .unwrap()
                .url,
            "https://example.com/new"
        );
        vec![("New response", "Simplify", None)]
    });
}

#[test]
fn cancellation_delivers_neither_queued_nor_uncooperative_success_or_error() {
    for answer in [Answer::Success("Old response"), Answer::Failure] {
        for queued in [true, false] {
            check(|h| {
                let old = Producer::new("Initial", "Stale draft", answer);
                let id = h.start(old.clone(), Pending::action(ReadingAction::Simplify), None);
                h.wait_for(id, Kind::Draft);
                if queued {
                    old.finish();
                    h.wait_for(id, Kind::Finished);
                    h.wait_stopped(id);
                } else {
                    h.drain();
                    assert_eq!(h.runtime.state.draft, "Initial");
                }
                h.runtime.update(|state, _| state.cancel());
                h.wait_stopped(id);
                if !queued {
                    old.finish();
                    wait(|| old.done.load(Ordering::SeqCst));
                }
                h.drain();
                assert_eq!(old.result_attempts.load(Ordering::SeqCst), 1);
                if !queued {
                    assert!(!old.result_sent.load(Ordering::SeqCst));
                    assert!(!old.draft_sent.load(Ordering::SeqCst));
                }
                assert_eq!(h.accepted(id, Kind::Finished), 0);
                assert!(matches!(h.runtime.state.activity, Activity::Idle));
                assert!(h.runtime.state.draft.is_empty());
                assert!(h.runtime.state.result.is_none());
                assert!(h.runtime.state.conversation.is_empty());
                assert!(h.runtime.state.current_answer_entry().is_none());
                assert!(!h.services.busy.load(Ordering::Acquire));
                vec![]
            });
        }
    }
}

#[test]
fn ignored_cancellation_late_draft_attempt_cannot_enter_a_new_source_session() {
    check(|h| {
        let old = Producer::new("initial", "stale draft", Answer::Success("stale answer"));
        let old_id = h.start(
            old.clone(),
            Pending::action(ReadingAction::Simplify),
            Some(pointer("old")),
        );
        h.wait_for(old_id, Kind::Draft);
        h.drain();
        assert_eq!(h.runtime.state.draft, "initial");
        h.runtime.update(|state, _| {
            state.source = TextEdit::new("New Sample passage.");
            state.commit_source();
        });
        h.wait_stopped(old_id);
        let new = Producer::new(
            "New draft",
            "New final draft",
            Answer::Success("New answer"),
        );
        let pending = Pending {
            question: "Explain Sample in the fixed source.".into(),
            label: Some("Explain “Sample”".into()),
            recorded_intent: Some(RecordedIntent::Explain("Sample".into())),
            ..Pending::action(ReadingAction::FollowUp)
        };
        let id = h.start(new.clone(), pending, Some(pointer("new")));
        h.wait_for(id, Kind::Draft);
        h.drain();
        assert_eq!(h.runtime.state.draft, "New draft");
        old.finish();
        wait(|| old.done.load(Ordering::SeqCst));
        h.drain();
        assert_eq!(old.draft_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(old.result_attempts.load(Ordering::SeqCst), 1);
        assert!(!old.draft_sent.load(Ordering::SeqCst));
        assert!(!old.result_sent.load(Ordering::SeqCst));
        assert_eq!(h.runtime.state.draft, "New draft");
        assert_eq!(h.accepted(old_id, Kind::Draft), 1); // initial only, before supersession
        assert_eq!(h.accepted(old_id, Kind::Finished), 0);
        new.finish();
        h.wait_for(id, Kind::Finished);
        h.drain();
        assert_eq!(h.runtime.state.answer(), "New answer");
        let saved = h.runtime.state.current_answer_entry().unwrap();
        assert_eq!(saved.action, "Explain");
        assert_eq!(saved.question.as_deref(), Some("Sample"));
        assert_eq!(saved.passage, "New Sample passage.");
        assert_eq!(saved.reference.unwrap().url, "https://example.com/new");
        vec![("New answer", "Explain", Some("Sample"))]
    });
}
