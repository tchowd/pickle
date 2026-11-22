use crate::storage::{Entry, Repository, Settings};
use pickle_core::*;
use pickle_providers::{
    CloudflareProvider, JevClient, LocalAIConfiguration, OllamaProvider, SampleProvider,
};
use rune::{AppHandle, TextEdit, Tx, WindowHandle};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio_util::sync::CancellationToken;

pub const SAMPLE: &str = "The treatment may reduce symptoms in some patients, but the evidence remains limited. Larger studies are needed before the findings can be applied more broadly.";
pub const SAMPLE_ANSWER: &str = "The treatment **might help some people**, but there is not enough evidence yet.\n\nLarger studies are needed to find out how well it works and who could benefit.\n\nThis is a fixed demonstration. Your passage stays on this device.";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Route {
    Reader,
    Settings,
    Library(bool),
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Panel {
    Compose,
    Choosing,
    Expanded,
}
#[derive(Clone)]
pub struct Pending {
    pub action: ReadingAction,
    pub question: String,
    pub label: Option<String>,
    pub chart: Option<ChartKind>,
    pub limited: bool,
}
impl Pending {
    pub fn action(action: ReadingAction) -> Self {
        Self {
            action,
            question: String::new(),
            label: None,
            chart: None,
            limited: false,
        }
    }
}
pub enum Activity {
    Idle,
    WaitingContext,
    Running {
        id: u64,
        cancel: CancellationToken,
        progress: String,
    },
    Consent(Pending),
    NeedsContext(Pending, Vec<String>),
    ChooseChart(Pending),
    Failed(String),
}
pub struct Services {
    pub runtime: tokio::runtime::Runtime,
    pub repository: Option<Arc<Repository>>,
    pub paused: AtomicBool,
    pub exclusions: std::sync::Mutex<Vec<String>>,
    pub shutdown: CancellationToken,
    pub settings_revision: AtomicU64,
    pub settings_writer: std::sync::Mutex<()>,
    pub policy_pending: AtomicBool,
    pub pinned: AtomicBool,
    pub busy: AtomicBool,
    pub invocation_revision: AtomicU64,
}
impl Services {
    pub fn new(repository: Option<Arc<Repository>>, settings: &Settings) -> Self {
        Self {
            runtime: tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Could not start the background runtime"),
            repository,
            paused: AtomicBool::new(false),
            exclusions: std::sync::Mutex::new(settings.exclusions.clone()),
            shutdown: CancellationToken::new(),
            settings_revision: AtomicU64::new(0),
            settings_writer: std::sync::Mutex::new(()),
            policy_pending: AtomicBool::new(false),
            pinned: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            invocation_revision: AtomicU64::new(0),
        }
    }
}
pub struct AppState {
    pub route: Route,
    pub panel: Panel,
    pub activity: Activity,
    pub settings: Settings,
    pub source: TextEdit,
    pub question: TextEdit,
    pub context: TextEdit,
    pub local_address: TextEdit,
    pub local_model: TextEdit,
    pub account_id: TextEdit,
    pub local_vision_model: TextEdit,
    pub transcriber_executable: TextEdit,
    pub transcriber_model: TextEdit,
    pub transcriber_language: TextEdit,
    pub exclusions: TextEdit,
    pub cloud_model: TextEdit,
    pub credential: TextEdit,
    pub search: TextEdit,
    pub snapshot: Option<SelectionSnapshot>,
    pub result: Option<ReadingResult>,
    pub conversation: Vec<ConversationTurn>,
    pub draft: String,
    pub demo: bool,
    pub paused: bool,
    pub notice: Option<String>,
    pub entries: Vec<Entry>,
    pub bookmarks: Vec<Entry>,
    pub handle: Option<AppHandle<Self>>,
    pub window: Option<WindowHandle>,
    pub window_geometry: Option<crate::geometry_store::WindowFrame>,
    pub window_geometry_worker: Option<Arc<crate::geometry_store::Worker>>,
    pub geometry_close_prepared: bool,
    pub diagnostics: crate::diagnostics::DiagnosticsState,
    pub services: Option<Arc<Services>>,
    pub revision: u64,
    pub last: Option<Pending>,
    pub capture_target: Option<crate::context::CaptureTarget>,
    pub context_state: crate::context::ContextState,
    pub native_enabled: bool,
    pub native_shell: Option<pickle_platform::NativeShell>,
    pub extension_id: TextEdit,
    pub update_state: crate::update_ui::UpdateState,
    pub update_attempt: uuid::Uuid,
    pub library_page: usize,
    pub library_selected: Option<String>,
    pub source_expanded: bool,
    pub image_cache: Option<rune::ImageCache>,
    pub pinned: bool,
    pub shortcut: TextEdit,
    pub settings_pending: bool,
    pub auto_preview: Option<crate::invocation_ui::AutoPreview>,
    pub last_auto_selection: Option<(String, String)>,
}
impl Default for AppState {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            route: Route::Reader,
            panel: Panel::Compose,
            activity: Activity::Idle,
            local_address: TextEdit::new(&settings.local_address),
            local_model: TextEdit::new(&settings.local_model),
            account_id: TextEdit::new(""),
            cloud_model: TextEdit::new(&settings.cloud_model),
            credential: TextEdit::new(""),
            search: TextEdit::new(""),
            local_vision_model: TextEdit::new(""),
            transcriber_executable: TextEdit::new(""),
            transcriber_model: TextEdit::new(""),
            transcriber_language: TextEdit::new("auto"),
            exclusions: TextEdit::new(settings.exclusions.join("\n")),
            shortcut: TextEdit::new(&settings.shortcut),
            settings,
            source: TextEdit::new(""),
            question: TextEdit::new(""),
            context: TextEdit::new(""),
            snapshot: None,
            result: None,
            conversation: vec![],
            draft: String::new(),
            demo: false,
            paused: false,
            notice: None,
            entries: vec![],
            bookmarks: vec![],
            handle: None,
            window: None,
            window_geometry: None,
            window_geometry_worker: None,
            geometry_close_prepared: false,
            diagnostics: crate::diagnostics::DiagnosticsState::default(),
            services: None,
            revision: 0,
            last: None,
            capture_target: None,
            context_state: crate::context::ContextState::default(),
            native_enabled: false,
            native_shell: None,
            extension_id: TextEdit::new(""),
            update_state: crate::update_ui::UpdateState::Idle,
            update_attempt: uuid::Uuid::new_v4(),
            library_page: 0,
            library_selected: None,
            source_expanded: false,
            image_cache: None,
            pinned: false,
            settings_pending: false,
            auto_preview: None,
            last_auto_selection: None,
        }
    }
}
impl AppState {
    /// Capture native placement on the UI thread; the dedicated worker performs
    /// all filesystem access. Transient choosers never replace reader geometry.
    pub fn remember_window_geometry(&mut self) {
        if !self.native_enabled || self.auto_preview.is_some() || self.panel == Panel::Choosing {
            return;
        }
        if let Some(problem) = self
            .window_geometry_worker
            .as_ref()
            .and_then(|worker| worker.take_error())
        {
            self.notice = Some(problem);
        }
        let Some(placement) = self.window.as_ref().and_then(WindowHandle::placement) else {
            return;
        };
        if placement.minimized || placement.maximized || placement.full_screen {
            return;
        }
        let frame = crate::geometry_store::WindowFrame {
            position: placement.position,
            logical_size: (
                (f64::from(placement.content_size.0) / placement.scale) as f32,
                (f64::from(placement.content_size.1) / placement.scale) as f32,
            ),
            scale: placement.scale,
        };
        self.record_window_geometry(frame);
    }

    fn record_window_geometry(&mut self, frame: crate::geometry_store::WindowFrame) {
        if !self.native_enabled
            || self.auto_preview.is_some()
            || self.panel == Panel::Choosing
            || frame.validate().is_err()
        {
            return;
        }
        self.window_geometry = Some(frame);
        if let Some(worker) = &self.window_geometry_worker
            && let Err(problem) = worker.enqueue(frame)
        {
            self.notice = Some(problem);
        }
    }

    /// Restore within a current work area. A removed monitor or changed scale
    /// cannot leave the saved reader off-screen. Call only on the UI thread.
    pub fn restore_window_geometry(&mut self) {
        if !self.native_enabled {
            return;
        }
        let (Some(frame), Some(window)) = (self.window_geometry, self.window.as_ref()) else {
            return;
        };
        let Some(placement) = window.placement() else {
            return;
        };
        let target =
            pickle_platform::restore_work_area(frame.position.0, frame.position.1, frame.scale)
                .or_else(|| {
                    pickle_platform::window_work_area(
                        window.window_number(),
                        placement.position.0,
                        placement.position.1,
                    )
                    .map(|area| (area, placement.scale))
                })
                .or_else(|| {
                    placement.display.map(|(position, size)| {
                        (
                            pickle_platform::WorkArea {
                                x: position.0,
                                y: position.1,
                                width: size.0,
                                height: size.1,
                            },
                            placement.scale,
                        )
                    })
                });
        let Some((area, target_scale)) = target else {
            return;
        };
        // winit's macOS global physical origin uses the window backing scale;
        // Windows desktop origins remain physical across monitor DPI changes.
        #[cfg(target_os = "macos")]
        let frame = crate::geometry_store::WindowFrame {
            position: (
                (f64::from(frame.position.0) / frame.scale * target_scale).round() as i32,
                (f64::from(frame.position.1) / frame.scale * target_scale).round() as i32,
            ),
            ..frame
        };
        let Some(restored) = crate::geometry_store::clamp_to_work_area(
            frame,
            target_scale,
            (area.x, area.y, area.width, area.height),
        ) else {
            return;
        };
        window.set_position(restored.position);
        window.resize_instant(restored.logical_size);
    }

    /// The same idempotent preparation is used by Escape, native dismiss, close
    /// and quit hooks. Preserve the pre-preview frame before restoring the panel.
    pub fn prepare_window_close(&mut self) {
        if !self.geometry_close_prepared {
            if self.auto_preview.is_none() {
                self.remember_window_geometry();
            }
            self.geometry_close_prepared = true;
        }
        self.dismiss_auto_preview();
    }

    pub fn load_settings(&mut self, settings: Settings) {
        self.shortcut = TextEdit::new(&settings.shortcut);
        self.local_address = TextEdit::new(&settings.local_address);
        self.local_model = TextEdit::new(&settings.local_model);
        self.account_id = TextEdit::new(&settings.account_id);
        self.cloud_model = TextEdit::new(&settings.cloud_model);
        self.local_vision_model = TextEdit::new(&settings.local_vision_model);
        self.transcriber_executable = TextEdit::new(&settings.transcriber_executable);
        self.transcriber_model = TextEdit::new(&settings.transcriber_model);
        self.transcriber_language = TextEdit::new(&settings.transcriber_language);
        self.exclusions = TextEdit::new(settings.exclusions.join("\n"));
        self.settings = settings;
    }
    pub fn cancel(&mut self) {
        if let Some(services) = &self.services {
            services.invocation_revision.fetch_add(1, Ordering::AcqRel);
        }
        self.context_state.cancel_work();
        self.cancel_request();
    }
    fn cancel_request(&mut self) {
        if let Some(services) = &self.services {
            services.busy.store(false, Ordering::Release);
        }
        if let Activity::Running { cancel, .. } = &self.activity {
            cancel.cancel();
        }
        self.revision = self.revision.wrapping_add(1);
        self.activity = Activity::Idle;
        self.draft.clear();
    }
    pub fn reset(&mut self) {
        self.cancel();
        self.snapshot = None;
        self.result = None;
        self.conversation.clear();
        self.source.clear();
        self.context.clear();
        self.question.clear();
        self.demo = false;
        self.notice = None;
        self.last = None;
        self.auto_preview = None;
        self.last_auto_selection = None;
        self.source_expanded = false;
        self.panel = Panel::Compose;
        self.route = Route::Reader;
        self.capture_target = None;
        self.context_state.clear();
    }
    pub fn sample(&mut self) {
        self.reset();
        self.demo = true;
        self.source = TextEdit::new(SAMPLE);
        self.snapshot = Some(SelectionSnapshot::new(
            SAMPLE,
            "Pickle demonstration",
            "sample",
        ));
        self.panel = Panel::Choosing;
    }
    pub fn sample_result(&mut self) {
        self.sample();
        self.panel = Panel::Expanded;
        self.result = Some(ReadingResult {
            id: uuid::Uuid::new_v4(),
            action: ReadingAction::Simplify,
            text: SAMPLE_ANSWER.into(),
            chart: None,
            quality: QualityStatus::NotChecked(QualityStatus::OFFLINE.into()),
            model: "Fixed demonstration".into(),
            evaluator_model: None,
            elapsed: 0.0,
            repairs: 0,
            notes: vec![],
            timing: None,
        });
    }
    pub fn accept_capture(&mut self, capture: pickle_platform::CapturedSelection) {
        if self.paused || self.settings_pending {
            return;
        }
        if pickle_platform::source_is_excluded(&capture.app_id, &self.settings.exclusions) {
            self.notice = Some("This source application is excluded.".into());
            return;
        }
        self.reset();
        self.last_auto_selection = Some((capture.app_id.clone(), capture.text.clone()));
        self.capture_target = Some(capture.clone());
        let mut snapshot = SelectionSnapshot::new(&capture.text, capture.app_name, capture.app_id);
        snapshot.method = "Accessibility".into();
        snapshot.source_url = capture.source_url;
        snapshot.bounds = capture.bounds.map(|b| SelectionBounds {
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
        });
        self.source = TextEdit::new(&capture.text);
        self.snapshot = Some(snapshot);
        self.panel = Panel::Choosing;
        self.collect_context();
    }
    pub fn commit_source(&mut self) {
        if self.source.text.trim().is_empty() {
            self.activity = Activity::Failed("Paste a passage to start reading.".into());
            return;
        }
        if self.source.text.len() > limits::SELECTION {
            self.activity = Activity::Failed(
                "This passage is too long. Use at most 12,000 UTF-8 bytes.".into(),
            );
            return;
        }
        self.cancel();
        self.context_state.clear();
        self.capture_target = None;
        self.context.clear();
        self.demo = false;
        self.result = None;
        self.conversation.clear();
        self.snapshot = Some(SelectionSnapshot::new(
            self.source.text.clone(),
            "Pasted passage",
            "manual",
        ));
        self.panel = Panel::Choosing;
    }
    pub fn expand(&self, tx: &mut Tx, height: f32) {
        // Preserve the bottom anchor, then clamp to the actual screen work area (including Dock/taskbar).
        if let Some(window) = &self.window
            && let Some(p) = window.placement()
        {
            let scale = if p.scale.is_finite() && p.scale > 0.0 {
                p.scale
            } else {
                1.0
            };
            let area = pickle_platform::window_work_area(
                window.window_number(),
                p.position.0,
                p.position.1,
            )
            .or_else(|| {
                p.display.map(|(position, size)| pickle_platform::WorkArea {
                    x: position.0,
                    y: position.1,
                    width: size.0,
                    height: size.1,
                })
            });
            if let Some(area) = area {
                let w = p.content_size.0.min(area.width).max(1);
                let h = ((height as f64 * scale).round() as u32)
                    .min(area.height)
                    .max(1);
                let max_x = area.x as i64 + area.width as i64 - w as i64;
                let max_y = area.y as i64 + area.height as i64 - h as i64;
                let x = (p.position.0 as i64).clamp(area.x as i64, max_x.max(area.x as i64));
                let y = (p.position.1 as i64 + p.content_size.1 as i64 - h as i64)
                    .clamp(area.y as i64, max_y.max(area.y as i64));
                tx.resize_window_instant((w as f32 / scale as f32, h as f32 / scale as f32));
                window.set_position((x as i32, y as i32));
            } else {
                tx.resize_window_instant((p.content_size.0 as f32 / scale as f32, height));
            }
            return;
        }
        tx.resize_window_instant((560.0, height));
    }
    pub fn run_action(&mut self, action: ReadingAction, tx: &mut Tx) {
        self.start(Pending::action(action), tx);
    }
    pub fn follow_up(&mut self, question: String, label: Option<String>, tx: &mut Tx) {
        if question.trim().is_empty() {
            return;
        }
        self.start(
            Pending {
                question,
                label,
                ..Pending::action(ReadingAction::FollowUp)
            },
            tx,
        );
    }
    pub fn start(&mut self, pending: Pending, tx: &mut Tx) {
        if self.settings_pending {
            self.notice =
                Some("Wait for your connection settings to finish saving, then try again.".into());
            return;
        }
        if !self.commit_auto_preview() {
            return;
        }
        self.cancel_request();
        self.route = Route::Reader;
        self.panel = Panel::Expanded;
        self.expand(tx, 680.0);
        self.last = Some(pending.clone());
        if self.context_state.pending > 0 {
            self.context_state.action = Some(pending);
            self.activity = Activity::WaitingContext;
            if let Some(services) = &self.services {
                services.busy.store(true, Ordering::Release);
            }
            return;
        }
        if self.paused {
            self.activity = Activity::Failed("Pickle is paused. Resume to continue.".into());
            return;
        }
        let Some(snapshot) = self.snapshot.clone() else {
            self.activity = Activity::Failed("Select or paste a passage first.".into());
            return;
        };
        if pickle_platform::source_is_excluded(&snapshot.app_id, &self.settings.exclusions) {
            self.activity = Activity::Failed("This application is excluded from Pickle.".into());
            return;
        }
        if pending.action == ReadingAction::FollowUp && self.conversation.len() >= limits::TURNS {
            self.activity = Activity::Failed(
                "This conversation has six follow-ups. Start a new reading action to continue."
                    .into(),
            );
            return;
        }
        if !self.demo {
            if self.settings.mode == "Online" && self.settings.strict_offline {
                self.activity = Activity::Failed(
                    "Offline mode blocks Online AI. Choose a model running on this device.".into(),
                );
                return;
            }
            if (self.settings.mode == "Online"
                && (!self.settings.cloud_consent
                    || self.settings.jev_enabled && !self.settings.jev_consent))
                || (self.settings.mode == "Local"
                    && self.settings.local_consent != self.settings.destination())
                || (self.context_state.has_content()
                    && self.settings.context_consent_destination
                        != self.settings.context_destination())
            {
                self.activity = Activity::Consent(pending);
                return;
            }
        }
        let mut input = RequestInput::new(snapshot, pending.action);
        input.level = self.settings.level;
        input.context = self.context.text.clone();
        input.question = pending.question.clone();
        input.chart_choice = pending.chart;
        input.limited = pending.limited;
        input.page_context = self.context_state.reading_text();
        input.visual_context = self.context_state.visual_summary.clone();
        input.reference = self.context_state.reference.clone();
        if pending.action == ReadingAction::FollowUp {
            input.conversation = self.conversation.clone();
            input.previous_result = self
                .conversation
                .last()
                .map(|t| t.answer.clone())
                .or_else(|| self.result.as_ref().map(|r| r.text.clone()))
                .unwrap_or_default();
        }
        if let Err(error) = input.validate() {
            self.activity = Activity::Failed(error.to_string());
            return;
        }
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            if self.demo {
                self.sample_result();
            } else {
                self.activity =
                    Activity::Failed("Connection services are not ready. Try again.".into());
            }
            return;
        };
        let id = self.revision;
        let cancel = CancellationToken::new();
        self.activity = Activity::Running {
            id,
            cancel: cancel.clone(),
            progress: "Preparing…".into(),
        };
        services.busy.store(true, Ordering::Release);
        let settings = self.settings.clone();
        let demo = self.demo;
        let record = settings.history_enabled && !demo;
        let input_for_record = input.clone();
        let worker_services = services.clone();
        services.runtime.spawn(async move {
            let prepared = make_pipeline(&settings, demo).await;
            let pipeline = match prepared { Ok(p) => p, Err(error) => { handle.update(move |s, _| if s.is_current(id) { if let Some(services) = &s.services { services.busy.store(false, Ordering::Release); } s.activity = Activity::Failed(error); }); return; } };
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut latest_draft = None; let mut tick = tokio::time::interval(std::time::Duration::from_millis(80));
            let work = pipeline.run(input, sender, cancel.clone()); tokio::pin!(work);
            let outcome = loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    event = receiver.recv() => match event {
                        Some(PipelineEvent::Draft(draft)) => latest_draft = Some(draft),
                        Some(PipelineEvent::Progress(progress)) => { let h = handle.clone(); h.update(move |s, _| if let Activity::Running { id: current, progress: label, .. } = &mut s.activity && *current == id { *label = progress; }); },
                        None => {},
                    },
                    _ = tick.tick() => if let Some(draft) = latest_draft.take() { handle.update(move |s, _| if s.is_current(id) { s.draft = draft; }); },
                    outcome = &mut work => break outcome,
                }
            };
            handle.update(move |s, _| {
                if !s.is_current(id) { return; }
                if let Some(services) = &s.services { services.busy.store(false, Ordering::Release); }
                s.draft.clear();
                match outcome {
                    Ok(PipelineOutcome::Result(result)) => {
                        if should_record_result(&result, record, s.settings.history_enabled) {
                            let mut entry = make_entry(&input_for_record.snapshot, &result, &pending);
                            entry.reference = input_for_record.reference.as_ref().map(|r| crate::storage::EntryReference { url: r.url.clone(), title: r.title.clone(), kind: r.kind.clone() });
                            if let Some(repo) = worker_services.repository.clone() {
                                let h = s.handle.clone();
                                worker_services.runtime.spawn_blocking(move || {
                                    let saved = repo.append(false, entry).and_then(|_| repo.history());
                                    if let Some(h) = h { h.update(move |s, _| match saved { Ok(entries) => s.entries = entries, Err(error) => s.notice = Some(error) }); }
                                });
                            }
                        }
                        if pending.action == ReadingAction::FollowUp {
                            s.conversation.push(ConversationTurn { question: pending.question, label: pending.label, answer: result.text.clone(), quality: result.quality.clone() }); s.question.clear();
                            while s.conversation.iter().map(|t| t.question.len() + t.answer.len()).sum::<usize>() > limits::CONVERSATION { s.conversation.remove(0); }
                        } else { s.result = Some(result); s.conversation.clear(); }
                        s.activity = Activity::Idle;
                    }
                    Ok(PipelineOutcome::NeedsContext(flags)) => s.activity = Activity::NeedsContext(pending, flags),
                    Ok(PipelineOutcome::ChooseChart) => s.activity = Activity::ChooseChart(pending),
                    Err(PickleError::Cancelled) => s.activity = Activity::Idle,
                    Err(error) => s.activity = Activity::Failed(error.to_string()),
                }
            });
        });
    }
    fn is_current(&self, id: u64) -> bool {
        self.revision == id
            && matches!(self.activity, Activity::Running { id: current, .. } if current == id)
    }
    pub fn consent(&mut self, tx: &mut Tx) {
        let Activity::Consent(pending) = &self.activity else {
            return;
        };
        let pending = pending.clone();
        if self.settings.mode == "Online" {
            self.settings.cloud_consent = true;
            if self.settings.jev_enabled {
                self.settings.jev_consent = true;
            }
        } else {
            self.settings.local_consent = self.settings.destination();
        }
        if self.context_state.has_content() {
            self.settings.context_consent_destination = self.settings.context_destination();
        }
        self.persist_settings();
        self.start(pending, tx);
    }
    pub fn change_mode(&mut self, mode: &str) {
        if self.settings_pending {
            return;
        }
        self.cancel();
        self.settings.mode = mode.into();
        self.persist_settings();
    }
    pub fn toggle_pause(&mut self) {
        let next = !self.paused;
        if let Some(shell) = &self.native_shell
            && let Err(error) = shell.set_paused(next)
        {
            self.notice = Some(error.to_string());
            return;
        }
        self.cancel();
        self.paused = next;
        self.dismiss_auto_preview();
        if let Some(services) = &self.services {
            services.paused.store(self.paused, Ordering::Release);
        }
    }
    pub fn save_settings(&mut self) {
        if self.settings_pending {
            return;
        }
        let mut draft = self.settings.clone();
        draft.local_address = self.local_address.text.trim().into();
        draft.local_model = self.local_model.text.trim().into();
        draft.account_id = self.account_id.text.trim().into();
        draft.cloud_model = self.cloud_model.text.trim().into();
        draft.local_vision_model = self.local_vision_model.text.trim().into();
        draft.transcriber_executable = self.transcriber_executable.text.trim().into();
        draft.transcriber_model = self.transcriber_model.text.trim().into();
        draft.transcriber_language = self.transcriber_language.text.trim().into();
        draft.shortcut = self.shortcut.text.trim().into();
        draft.exclusions = self
            .exclusions
            .text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if draft.destination() != self.settings.destination() {
            draft.local_consent.clear();
        }
        if let Err(error) = draft.validate() {
            self.notice = Some(error);
            return;
        }
        if draft.mode == "Local" {
            let config = LocalAIConfiguration {
                address: draft.local_address.clone(),
                model: draft.local_model.clone(),
                token: String::new(),
                same_device: draft.same_device,
            };
            if let Err(error) = config.validate(false) {
                self.notice = Some(error.to_string());
                return;
            }
        }
        if !draft.account_id.is_empty()
            && (draft.account_id.len() != 32
                || !draft.account_id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            self.notice =
                Some("Cloudflare account IDs must contain 32 hexadecimal characters.".into());
            return;
        }
        self.cancel();
        self.queue_settings(draft, true);
    }
    pub(crate) fn persist_settings(&mut self) {
        if self.settings_pending {
            return;
        }
        self.queue_settings(self.settings.clone(), false);
    }
    pub fn save_appearance(&mut self) {
        if let Err(error) = self.settings.validate() {
            self.notice = Some(error);
            return;
        }
        self.persist_settings();
    }
    fn queue_settings(&mut self, settings: Settings, commit: bool) {
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let Some(repo) = services.repository.clone() else {
            self.notice = Some("Local storage is unavailable.".into());
            return;
        };
        if commit && !self.apply_invocation(settings.invocation_config()) {
            return;
        }
        if commit {
            self.settings_pending = true;
            services.policy_pending.store(true, Ordering::Release);
        }
        let version = services.settings_revision.fetch_add(1, Ordering::AcqRel) + 1;
        let worker = services.clone();
        let previous = self.settings.clone();
        services.runtime.spawn_blocking(move || {
            // The mutex protects write ordering only; a prior panic cannot invalidate a settings value.
            let _guard = worker
                .settings_writer
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if worker.settings_revision.load(Ordering::Acquire) != version {
                return;
            }
            let outcome = settings.transcriber_config().and_then(|config| {
                if let Some(config) = &config {
                    config.validate().map_err(|e| e.to_string())?;
                }
                repo.save_settings(&settings)?;
                pickle_platform::configure_local_transcriber(config).map_err(|e| e.to_string())
            });
            let outcome_services = worker.clone();
            handle.update(move |s, _| {
                if outcome_services.settings_revision.load(Ordering::Acquire) != version {
                    return;
                }
                if commit {
                    s.settings_pending = false;
                    outcome_services
                        .policy_pending
                        .store(false, Ordering::Release);
                }
                match outcome {
                    Ok(()) => {
                        if commit && s.settings == previous {
                            s.cancel();
                            s.settings = settings;
                        }
                        if let Ok(mut exclusions) = outcome_services.exclusions.lock() {
                            *exclusions = s.settings.exclusions.clone();
                        }
                        s.notice = Some("Settings saved on this device.".into());
                    }
                    Err(error) => {
                        if commit && !s.apply_invocation(previous.invocation_config()) {
                            let rollback = s.notice.take().unwrap_or_default();
                            s.notice = Some(format!(
                                "{error} The previous shortcut could not be restored: {rollback}"
                            ));
                        } else {
                            s.notice = Some(error);
                        }
                    }
                }
            });
        });
    }
    pub fn save_credential(&mut self, remove: bool) {
        if self.settings_pending {
            self.notice = Some(
                "Wait for the connection settings to finish saving before changing its credential."
                    .into(),
            );
            return;
        }
        if (self.settings.mode == "Local"
            && self.settings.local_address != self.local_address.text.trim())
            || (self.settings.mode == "Online"
                && self.settings.account_id != self.account_id.text.trim())
        {
            self.notice =
                Some("Save your connection settings before changing its credential.".into());
            return;
        }
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let account = self.settings.credential_account();
        let secret = self.credential.text.clone();
        self.credential.clear();
        services.runtime.spawn_blocking(move || {
            let result = if remove {
                pickle_platform::credential_remove(&account)
            } else {
                pickle_platform::credential_set(&account, &secret)
            };
            handle.update(move |s, _| {
                s.notice = Some(
                    result
                        .map(|_| {
                            if remove {
                                "Saved credential removed."
                            } else {
                                "Credential saved in your system vault."
                            }
                            .to_owned()
                        })
                        .unwrap_or_else(|e| e.to_string()),
                )
            });
        });
    }
    pub fn authorize_credential(&mut self) {
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let account = self.settings.credential_account();
        services.runtime.spawn_blocking(move || {
            let result =
                pickle_platform::credential_get(&account, true).map(|token| token.is_some());
            handle.update(move |s, _| {
                s.notice = Some(match result {
                    Ok(true) => "Saved credential authorized by the system.".into(),
                    Ok(false) => "No credential is saved for this connection.".into(),
                    Err(error) => error.to_string(),
                })
            });
        });
    }
    pub fn save_answer(&mut self) {
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        let Some(result) = self.result.clone() else {
            return;
        };
        let pending = Pending::action(result.action);
        let mut entry = make_entry(&snapshot, &result, &pending);
        if let Some(turn) = self.conversation.last() {
            entry.answer = turn.answer.clone();
            entry.action = "Follow-up".into();
            entry.question = Some(turn.question.clone());
            entry.quality = turn.quality.visible_label();
            entry.chart = None;
        }
        entry.reference =
            self.context_state
                .reference
                .as_ref()
                .map(|r| crate::storage::EntryReference {
                    url: r.url.clone(),
                    title: r.title.clone(),
                    kind: r.kind.clone(),
                });
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let Some(repo) = services.repository.clone() else {
            return;
        };
        services.runtime.spawn_blocking(move || {
            let result = repo.append(true, entry).and_then(|_| repo.bookmarks());
            handle.update(move |s, _| match result {
                Ok(entries) => {
                    s.bookmarks = entries;
                    s.notice = Some("Answer saved on this device.".into());
                }
                Err(error) => s.notice = Some(error),
            });
        });
    }
    pub fn remove_entry(&mut self, bookmark: bool, id: String) {
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let Some(repo) = services.repository.clone() else {
            return;
        };
        services.runtime.spawn_blocking(move || {
            let result = repo.remove(bookmark, &id).and_then(|_| {
                if bookmark {
                    repo.bookmarks()
                } else {
                    repo.history()
                }
            });
            handle.update(move |s, _| match result {
                Ok(entries) => {
                    if bookmark {
                        s.bookmarks = entries;
                    } else {
                        s.entries = entries;
                    }
                }
                Err(error) => s.notice = Some(error),
            });
        });
    }
    pub fn answer(&self) -> &str {
        self.conversation
            .last()
            .map(|t| t.answer.as_str())
            .or_else(|| self.result.as_ref().map(|r| r.text.as_str()))
            .unwrap_or("")
    }
    pub fn import_legacy(&mut self) {
        let Some(paths) = crate::migration::default_legacy_paths() else {
            self.notice = Some(
                "Legacy import is available on macOS, where the original app stored its data."
                    .into(),
            );
            return;
        };
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let Some(repo) = services.repository.clone() else {
            return;
        };
        services.runtime.spawn_blocking(move || {
            let result =
                crate::migration::import_legacy(&repo, Some(&paths.preferences), &paths.history);
            let entries = repo.history();
            let bookmarks = repo.bookmarks();
            handle.update(move |s, _| {
                s.notice = Some(
                    result
                        .map(|report| report.to_string())
                        .unwrap_or_else(|e| e),
                );
                if let Ok(entries) = entries {
                    s.entries = entries;
                }
                if let Ok(bookmarks) = bookmarks {
                    s.bookmarks = bookmarks;
                }
            });
        });
    }
}
pub(crate) fn should_record_result(
    result: &ReadingResult,
    record_at_start: bool,
    enabled_now: bool,
) -> bool {
    record_at_start
        && enabled_now
        && (result.action != ReadingAction::Chart || result.chart.is_some())
}
fn make_entry(snapshot: &SelectionSnapshot, result: &ReadingResult, pending: &Pending) -> Entry {
    Entry {
        id: uuid::Uuid::new_v4().to_string(),
        recorded_at: chrono::Utc::now().to_rfc3339(),
        app_name: snapshot.app_name.clone(),
        app_id: snapshot.app_id.clone(),
        action: pending.action.title().into(),
        question: if pending.question.is_empty() {
            None
        } else {
            Some(
                pending
                    .label
                    .clone()
                    .unwrap_or_else(|| pending.question.clone()),
            )
        },
        passage: snapshot.text.clone(),
        answer: result.text.clone(),
        quality: result.quality.visible_label(),
        chart: result.chart.as_ref().map(|c| c.kind().as_str().into()),
        reference: None,
    }
}
async fn make_pipeline(settings: &Settings, demo: bool) -> std::result::Result<Pipeline, String> {
    if demo {
        return Ok(Pipeline::new(Arc::new(SampleProvider), None, true));
    }
    let account = settings.credential_account();
    let token =
        tokio::task::spawn_blocking(move || pickle_platform::credential_get(&account, false))
            .await
            .map_err(|_| "The credential service could not finish.".to_owned())?
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
    let provider: Arc<dyn GenerativeProvider>;
    let mut evaluator: Option<Arc<dyn DecisionClient>> = None;
    if settings.mode == "Local" {
        let configuration = LocalAIConfiguration {
            address: settings.local_address.clone(),
            model: settings.local_model.clone(),
            token,
            same_device: settings.same_device,
        };
        configuration
            .validate(settings.strict_offline)
            .map_err(|e| e.to_string())?;
        provider = Arc::new(OllamaProvider::new(configuration).with_streaming(settings.streaming));
    } else {
        if settings.strict_offline {
            return Err("Offline mode blocks Online AI.".into());
        }
        if token.is_empty() {
            return Err("Save your Cloudflare API token in Settings.".into());
        }
        if settings.jev_enabled {
            evaluator = Some(Arc::new(JevClient::new(
                settings.account_id.clone(),
                token.clone(),
            )));
        }
        provider = Arc::new(
            CloudflareProvider::new(
                settings.account_id.clone(),
                token,
                settings.cloud_model.clone(),
            )
            .with_streaming(settings.streaming),
        );
    }
    Ok(Pipeline::new(provider, evaluator, settings.strict_offline))
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    fn frame() -> crate::geometry_store::WindowFrame {
        crate::geometry_store::WindowFrame {
            position: (-600, 120),
            logical_size: (560.0, 680.0),
            scale: 2.0,
        }
    }

    #[test]
    fn geometry_cannot_be_recorded_by_headless_scenes_or_transient_choosers() {
        let mut state = AppState::default();
        state.record_window_geometry(frame());
        assert!(state.window_geometry.is_none());
        state.native_enabled = true;
        state.panel = Panel::Choosing;
        state.record_window_geometry(frame());
        assert!(state.window_geometry.is_none());
        state.panel = Panel::Expanded;
        state.record_window_geometry(frame());
        assert_eq!(state.window_geometry, Some(frame()));
        state.record_window_geometry(crate::geometry_store::WindowFrame {
            scale: f64::NAN,
            ..frame()
        });
        assert_eq!(state.window_geometry, Some(frame()));
    }

    #[test]
    fn closing_an_automatic_preview_preserves_the_previous_reader_frame() {
        let mut state = AppState {
            native_enabled: true,
            ..AppState::default()
        };
        state.sample_result();
        state.record_window_geometry(frame());
        state.settings.automatic_menu = true;
        assert!(state.show_auto_capture(pickle_platform::CapturedSelection {
            text: "A preview passage".into(),
            app_name: "Fixture".into(),
            app_id: "fixture".into(),
            bounds: None,
            source_url: None,
            source_pid: 0,
            source_window: None,
        }));
        state.record_window_geometry(crate::geometry_store::WindowFrame {
            logical_size: (560.0, 300.0),
            ..frame()
        });
        state.prepare_window_close();
        state.prepare_window_close();
        assert!(state.auto_preview.is_none());
        assert_eq!(state.panel, Panel::Expanded);
        assert_eq!(state.window_geometry, Some(frame()));
        assert!(state.geometry_close_prepared);
    }
}
