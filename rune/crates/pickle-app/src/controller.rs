use crate::library_search::{EntryStore, LibrarySearchState};
use crate::storage::{Entry, EntryReference, Repository, Settings};
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

#[path = "reading_worker.rs"]
mod reading_worker;
use reading_worker::{ReadingAttempt, ReadingUpdate, UpdateSink, run_prepared};

#[cfg(test)]
#[path = "reading_worker_tests.rs"]
mod reading_worker_tests;

#[cfg(test)]
#[path = "context_lifecycle_tests.rs"]
mod context_lifecycle_tests;

#[cfg(test)]
#[path = "answer_reference_tests.rs"]
mod answer_reference_tests;

#[cfg(test)]
#[path = "term_matching_tests.rs"]
mod term_matching_tests;

#[cfg(test)]
#[path = "recorded_intent_tests.rs"]
mod recorded_intent_tests;

#[cfg(test)]
#[path = "recorded_privacy_tests.rs"]
mod recorded_privacy_tests;

#[cfg(test)]
#[path = "recorded_quality_tests.rs"]
mod recorded_quality_tests;

#[path = "library_controller.rs"]
pub(crate) mod library_controller;

/// Recording metadata belongs to accepted answers, not to pending requests or context.
/// Explicit Save exposes only the base answer or the latest follow-up.
#[derive(Default)]
pub struct AnswerReferences {
    base: Option<EntryReference>,
    follow_up: Option<EntryReference>,
    follow_up_intent: Option<RecordedIntent>,
}

/// User-facing provenance only. Generation and conversation wire actions remain
/// FollowUp; labels and model instructions are not a recording classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedIntent {
    FollowUp(String),
    Explain(String),
    Adjustment(String),
}
impl RecordedIntent {
    fn fields(&self) -> (&'static str, &str) {
        match self {
            Self::FollowUp(question) => ("Follow-up", question),
            Self::Explain(term) => ("Explain", term),
            Self::Adjustment(label) => ("Adjustment", label),
        }
    }
}

pub const SAMPLE: &str = "The treatment may reduce symptoms in some patients, but the evidence remains limited. Larger studies are needed before the findings can be applied more broadly.";
pub const SAMPLE_ANSWER: &str = "The treatment **might help some people**, but there is not enough evidence yet.\n\nLarger studies are needed to find out how well it works and who could benefit.\n\nThis is a fixed demonstration. Your passage stays on this device.";
pub const READER_SCROLL: &str = "reader-scroll";
pub const READER_STATUS: &str = "reader-status";

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
    pub recorded_intent: Option<RecordedIntent>,
}
impl Pending {
    pub fn action(action: ReadingAction) -> Self {
        Self {
            action,
            question: String::new(),
            label: None,
            chart: None,
            limited: false,
            recorded_intent: None,
        }
    }
    fn intent(&self) -> Option<RecordedIntent> {
        (self.action == ReadingAction::FollowUp).then(|| {
            self.recorded_intent
                .clone()
                .unwrap_or_else(|| RecordedIntent::FollowUp(self.question.clone()))
        })
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
impl Drop for Activity {
    fn drop(&mut self) {
        if let Self::Running { cancel, .. } = self {
            cancel.cancel();
        }
    }
}
pub struct Services {
    pub runtime: crate::service_runtime::ServiceRuntime,
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
    pub fn new(
        repository: Option<Arc<Repository>>,
        settings: &Settings,
    ) -> std::io::Result<(Self, crate::service_runtime::RuntimeOwner)> {
        let (runtime, owner) = crate::service_runtime::ServiceRuntime::new()?;
        Ok((
            Self {
                runtime,
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
            },
            owner,
        ))
    }
}
pub struct AppState {
    pub route: Route,
    pub panel: Panel,
    pub activity: Activity,
    pub reader_following: bool,
    pub settings: Settings,
    pub source: TextEdit,
    pub question: TextEdit,
    pub term: TextEdit,
    pub context: TextEdit,
    pub local_address: TextEdit,
    pub local_model: TextEdit,
    pub local_connection: crate::local_connection::LocalConnectionState,
    pub account_id: TextEdit,
    pub local_vision_model: TextEdit,
    pub transcriber_executable: TextEdit,
    pub transcriber_model: TextEdit,
    pub transcriber_language: TextEdit,
    pub exclusions: TextEdit,
    pub cloud_model: TextEdit,
    pub credential: TextEdit,
    pub credential_recipient: Option<String>,
    pub credential_operation: Option<(uuid::Uuid, String)>,
    pub search: TextEdit,
    pub snapshot: Option<SelectionSnapshot>,
    pub result: Option<ReadingResult>,
    pub conversation: Vec<ConversationTurn>,
    pub answer_references: AnswerReferences,
    pub draft: String,
    pub demo: bool,
    pub paused: bool,
    pub notice: Option<String>,
    pub entries: EntryStore,
    pub bookmarks: EntryStore,
    pub library_search: LibrarySearchState,
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
    pub invocation_registration: crate::invocation_ui::InvocationRegistration,
    pub extension_id: TextEdit,
    pub update_state: crate::update_ui::UpdateState,
    pub update_attempt: uuid::Uuid,
    pub library_page: usize,
    pub library_selected: Option<String>,
    pub library_errors: [Option<String>; 2],
    pub library_delete_confirmation: Option<bool>,
    pub library_state: library_controller::LibraryState,
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
            reader_following: false,
            local_address: TextEdit::new(&settings.local_address),
            local_model: TextEdit::new(&settings.local_model),
            local_connection: crate::local_connection::LocalConnectionState::new(&settings),
            account_id: TextEdit::new(""),
            cloud_model: TextEdit::new(&settings.cloud_model),
            credential: TextEdit::new(""),
            credential_recipient: None,
            credential_operation: None,
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
            term: TextEdit::new(""),
            context: TextEdit::new(""),
            snapshot: None,
            result: None,
            conversation: vec![],
            answer_references: AnswerReferences::default(),
            draft: String::new(),
            demo: false,
            paused: false,
            notice: None,
            entries: EntryStore::default(),
            bookmarks: EntryStore::default(),
            library_search: LibrarySearchState::default(),
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
            invocation_registration: crate::invocation_ui::InvocationRegistration::Unavailable,
            extension_id: TextEdit::new(""),
            update_state: crate::update_ui::UpdateState::Idle,
            update_attempt: uuid::Uuid::new_v4(),
            library_page: 0,
            library_selected: None,
            library_errors: [None, None],
            library_delete_confirmation: None,
            library_state: library_controller::LibraryState::default(),
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
        self.pause_library_search();
        if !self.geometry_close_prepared {
            if self.auto_preview.is_none() {
                self.remember_window_geometry();
            }
            self.geometry_close_prepared = true;
        }
        self.dismiss_auto_preview();
    }

    pub fn load_settings(&mut self, settings: Settings) {
        if settings.credential_account() != self.settings.credential_account() {
            self.clear_draft_credential();
        }
        self.local_connection.record_saved(&settings);
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
        self.library_search.cancel_all();
        self.stop_reader_follow();
        self.cancel_local_connection_test();
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
        self.cancel_remove_all();
        self.cancel();
        self.snapshot = None;
        self.result = None;
        self.conversation.clear();
        self.answer_references = AnswerReferences::default();
        self.source.clear();
        self.context.clear();
        self.question.clear();
        self.term.clear();
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
        self.result = Some(Self::demonstration_result());
    }
    fn demonstration_result() -> ReadingResult {
        ReadingResult {
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
        }
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
        self.cancel_remove_all();
        self.capture_target = None;
        self.context.clear();
        self.demo = false;
        self.result = None;
        self.conversation.clear();
        self.answer_references = AnswerReferences::default();
        self.snapshot = Some(SelectionSnapshot::new(
            self.source.text.clone(),
            "Pasted passage",
            "manual",
        ));
        self.term.clear();
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
        if self.reading_busy() {
            return;
        }
        self.start_user_request(Pending::action(action), tx);
    }
    pub fn stop_reader_follow(&mut self) {
        self.reader_following = false;
    }
    /// Only an explicit reading action may rearm the viewport. Internal context
    /// completion continues through `start` without overriding a user's scroll.
    pub fn start_user_request(&mut self, pending: Pending, tx: &mut Tx) {
        if self.reading_busy() {
            return;
        }
        if self.settings_pending {
            self.start(pending, tx);
            return;
        }
        self.start(pending, tx);
        if matches!(
            self.activity,
            Activity::Running { .. } | Activity::WaitingContext
        ) || (self.demo && matches!(self.activity, Activity::Idle) && self.result.is_some())
        {
            self.reader_following = true;
            tx.scroll_to_bottom(READER_SCROLL);
        } else if self.route == Route::Reader && self.panel == Panel::Expanded {
            self.stop_reader_follow();
            tx.scroll_into_view(READER_STATUS);
        }
    }
    pub fn reading_busy(&self) -> bool {
        matches!(
            self.activity,
            Activity::Running { .. } | Activity::WaitingContext
        )
    }
    pub fn clear_follow_ups(&mut self) {
        if self.settings_pending || self.reading_busy() || self.conversation.is_empty() {
            return;
        }
        // Invalidate a consent/retry for a deleted turn without recapturing or
        // cancelling the source context retained by this reading.
        self.cancel_request();
        self.stop_reader_follow();
        self.conversation.clear();
        self.answer_references.follow_up = None;
        self.answer_references.follow_up_intent = None;
        self.last = self.result.as_ref().map(|result| Pending {
            chart: result.chart.as_ref().map(ValidatedChart::kind),
            ..Pending::action(result.action)
        });
    }
    pub fn explain_term(&mut self, term: String, tx: &mut Tx) {
        if self.reading_busy() || self.settings_pending {
            return;
        }
        let term = term.trim();
        if term.is_empty() || term.chars().count() > 150 {
            self.notice = Some("Choose a word or short phrase of at most 150 characters.".into());
            return;
        }
        let Some(snapshot) = self.display_snapshot() else {
            return;
        };
        let matches = |value: &str| contains_source_term(value, term);
        let answer_matches = |value: &str| matches(value) || matches(&answer_plain_text(value));
        if !matches(&snapshot.text)
            && !(self.auto_preview.is_none()
                && (self
                    .result
                    .as_ref()
                    .is_some_and(|result| answer_matches(&result.text))
                    || self
                        .conversation
                        .iter()
                        .any(|turn| answer_matches(&turn.answer))))
        {
            self.notice = Some("Choose a word or phrase from the passage or answer.".into());
            return;
        }
        self.notice = None;
        self.follow_up_recorded(
            format!("Explain the meaning of ‘{term}’ in this passage, using the surrounding context. Keep it brief; distinguish the contextual meaning from other meanings."),
            Some(format!("Explain “{term}”")), RecordedIntent::Explain(term.to_owned()), tx,
        );
    }
    pub fn follow_up(&mut self, question: String, label: Option<String>, tx: &mut Tx) {
        let intent = RecordedIntent::FollowUp(question.clone());
        self.follow_up_recorded(question, label, intent, tx);
    }
    pub fn adjust(&mut self, instruction: String, label: String, tx: &mut Tx) {
        self.follow_up_recorded(
            instruction,
            Some(label.clone()),
            RecordedIntent::Adjustment(label),
            tx,
        );
    }
    fn follow_up_recorded(
        &mut self,
        question: String,
        label: Option<String>,
        recorded_intent: RecordedIntent,
        tx: &mut Tx,
    ) {
        if question.trim().is_empty() {
            return;
        }
        self.start_user_request(
            Pending {
                question,
                label,
                recorded_intent: Some(recorded_intent),
                ..Pending::action(ReadingAction::FollowUp)
            },
            tx,
        );
    }
    pub fn start(&mut self, pending: Pending, tx: &mut Tx) {
        self.start_reading(pending, tx, true);
    }
    pub(crate) fn continue_context(&mut self, pending: Pending, tx: &mut Tx) {
        self.start_reading(pending, tx, false);
    }
    fn start_reading(&mut self, pending: Pending, tx: &mut Tx, activate: bool) {
        if self.settings_pending {
            self.notice =
                Some("Wait for your connection settings to finish saving, then try again.".into());
            return;
        }
        if activate && !self.commit_auto_preview() {
            return;
        }
        self.cancel_request();
        if activate {
            self.cancel_remove_all();
            self.route = Route::Reader;
            self.panel = Panel::Expanded;
            self.expand(tx, 680.0);
        }
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
                "This conversation has six follow-ups. Clear follow-ups to keep reading with the same passage."
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
                if activate {
                    self.sample_result();
                } else {
                    self.result = Some(Self::demonstration_result());
                    self.conversation.clear();
                    self.answer_references = AnswerReferences::default();
                    self.activity = Activity::Idle;
                }
            } else {
                self.activity =
                    Activity::Failed("Connection services are not ready. Try again.".into());
            }
            return;
        };
        let settings = self.settings.clone();
        let demo = self.demo;
        let record = settings.history_enabled && !demo;
        let (attempt, cancel) = self.enter_running(input, pending, record);
        let id = attempt.id;
        let sink: UpdateSink = Arc::new(move |update| {
            handle.update(move |state, _| {
                state.apply_reading_update(update);
            });
        });
        services.runtime.spawn(async move {
            let prepared = make_pipeline(&settings, demo).await;
            let pipeline = match prepared {
                Ok(pipeline) => pipeline,
                Err(message) => {
                    sink(ReadingUpdate::PreparationFailed { id, message });
                    return;
                }
            };
            run_prepared(pipeline, attempt, cancel, sink).await;
        });
    }
    /// Admission after validation/consent; start_reading already invalidated the
    /// prior attempt. This does not prepare providers or access native services.
    fn enter_running(
        &mut self,
        input: RequestInput,
        pending: Pending,
        record_at_start: bool,
    ) -> (ReadingAttempt, CancellationToken) {
        let id = self.revision;
        let cancel = CancellationToken::new();
        self.activity = Activity::Running {
            id,
            cancel: cancel.clone(),
            progress: "Preparing…".into(),
        };
        if let Some(services) = &self.services {
            services.busy.store(true, Ordering::Release);
        }
        (
            ReadingAttempt {
                id,
                input,
                pending,
                record_at_start,
            },
            cancel,
        )
    }
    fn apply_reading_update(&mut self, update: ReadingUpdate) -> bool {
        // Progress previously checked only Running.id. Requiring the revision
        // as well intentionally rejects inconsistent stale state; normal
        // cancel/start transitions already keep both identities in agreement.
        if !self.is_current(update.id()) {
            return false;
        }
        match update {
            ReadingUpdate::Progress { message, .. } => {
                if let Activity::Running { progress, .. } = &mut self.activity {
                    *progress = message;
                }
            }
            ReadingUpdate::Draft { text, .. } => self.draft = text,
            ReadingUpdate::PreparationFailed { message, .. } => {
                if let Some(services) = &self.services {
                    services.busy.store(false, Ordering::Release);
                }
                self.activity = Activity::Failed(message);
            }
            ReadingUpdate::Finished(finished) => {
                let reading_worker::FinishedReading { attempt, outcome } = *finished;
                if let Some(services) = &self.services {
                    services.busy.store(false, Ordering::Release);
                }
                self.draft.clear();
                match outcome {
                    Ok(PipelineOutcome::Result(result)) => {
                        if let Some(entry) = self.history_entry(
                            attempt.id,
                            &attempt.input,
                            &result,
                            &attempt.pending,
                            attempt.record_at_start,
                        ) {
                            self.record_history_entry(entry);
                        }
                        self.complete_reading_result_with_reference(
                            attempt.id,
                            &attempt.pending,
                            result,
                            reference_pointer(attempt.input.reference.as_ref()),
                        );
                    }
                    Ok(PipelineOutcome::NeedsContext(flags)) => {
                        self.activity = Activity::NeedsContext(attempt.pending, flags)
                    }
                    Ok(PipelineOutcome::ChooseChart) => {
                        self.activity = Activity::ChooseChart(attempt.pending)
                    }
                    Err(PickleError::Cancelled) => self.activity = Activity::Idle,
                    Err(error) => self.activity = Activity::Failed(error.to_string()),
                }
            }
        }
        true
    }
    fn is_current(&self, id: u64) -> bool {
        self.revision == id
            && matches!(self.activity, Activity::Running { id: current, .. } if current == id)
    }
    fn history_entry(
        &self,
        id: u64,
        input: &RequestInput,
        result: &ReadingResult,
        pending: &Pending,
        record_at_start: bool,
    ) -> Option<Entry> {
        if !self.is_current(id)
            || !should_record_result(result, record_at_start, self.settings.history_enabled)
        {
            return None;
        }
        let mut entry = make_entry(&input.snapshot, result, pending);
        entry.reference = reference_pointer(input.reference.as_ref());
        Some(entry)
    }
    #[cfg(test)]
    pub(crate) fn complete_reading_result(
        &mut self,
        id: u64,
        pending: &Pending,
        result: ReadingResult,
    ) -> bool {
        self.complete_reading_result_with_reference(id, pending, result, None)
    }
    fn complete_reading_result_with_reference(
        &mut self,
        id: u64,
        pending: &Pending,
        result: ReadingResult,
        reference: Option<EntryReference>,
    ) -> bool {
        if !self.is_current(id) {
            return false;
        }
        if pending.action == ReadingAction::FollowUp {
            self.conversation.push(ConversationTurn {
                question: pending.question.clone(),
                label: pending.label.clone(),
                answer: result.text,
                quality: result.quality,
            });
            self.question.clear();
            while self
                .conversation
                .iter()
                .map(|turn| turn.question.len() + turn.answer.len())
                .sum::<usize>()
                > limits::CONVERSATION
            {
                self.conversation.remove(0);
            }
            self.answer_references.follow_up = if self.conversation.is_empty() {
                None
            } else {
                reference
            };
            self.answer_references.follow_up_intent = if self.conversation.is_empty() {
                None
            } else {
                pending.intent()
            };
        } else {
            self.result = Some(result);
            self.conversation.clear();
            self.answer_references = AnswerReferences {
                base: reference,
                follow_up: None,
                follow_up_intent: None,
            };
        }
        self.draft.clear();
        self.activity = Activity::Idle;
        true
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
        self.start_user_request(pending, tx);
    }
    pub fn change_mode(&mut self, mode: &str) {
        if self.settings_pending {
            return;
        }
        self.cancel();
        if self.settings.mode != mode {
            self.clear_draft_credential();
        }
        self.settings.mode = mode.into();
        self.persist_settings();
    }
    pub fn toggle_pause(&mut self) {
        let next = !self.paused;
        if let Some(shell) = &self.native_shell {
            if let Err(error) = shell.set_paused(next) {
                if !next {
                    self.invocation_registration =
                        crate::invocation_ui::InvocationRegistration::NeedsRetry;
                }
                self.notice = Some(error.to_string());
                return;
            }
            if !next {
                self.invocation_registration = crate::invocation_ui::InvocationRegistration::Ready;
            }
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
        draft.reset_consent_for_recipient_change(&self.settings);
        // A rejected draft must not leave a masked key apparently associated
        // with a different visible endpoint/account.
        if draft.credential_account() != self.settings.credential_account() {
            self.clear_draft_credential();
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
        let invocation_changed =
            commit && settings.invocation_config() != self.settings.invocation_config();
        if invocation_changed && !self.apply_invocation(settings.invocation_config()) {
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
            let outcome = save_settings_transaction(&repo, &settings);
            let outcome_services = worker.clone();
            handle.update(move |s, tx| {
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
                        s.local_connection.record_saved(&settings);
                        if commit && s.settings == previous {
                            s.cancel();
                            s.apply_saved_settings(settings);
                        }
                        if let Ok(mut exclusions) = outcome_services.exclusions.lock() {
                            *exclusions = s.settings.exclusions.clone();
                        }
                        s.notice = Some("Settings saved on this device.".into());
                    }
                    Err(error) => {
                        if invocation_changed && !s.apply_invocation(previous.invocation_config()) {
                            let rollback = s.notice.take().unwrap_or_default();
                            s.notice = Some(format!(
                                "{error} The previous shortcut could not be restored: {rollback}"
                            ));
                        } else {
                            s.notice = Some(error);
                        }
                    }
                }
                // A context callback may have arrived while settings were
                // pending. Failure retains its action; success may deliberately
                // cancel it via the policy commit above. Neither navigates.
                s.context_ready(tx);
            });
        });
    }
    fn visible_credential_account(&self) -> String {
        Settings::credential_account_for(
            &self.settings.mode,
            self.local_address.text.trim(),
            self.account_id.text.trim(),
        )
    }
    pub fn edit_credential(&mut self) -> &mut TextEdit {
        let recipient = self.visible_credential_account();
        if self.credential_recipient.as_ref() != Some(&recipient) {
            self.credential.clear();
        }
        self.credential_recipient = Some(recipient);
        &mut self.credential
    }
    fn clear_draft_credential(&mut self) {
        self.credential.clear();
        self.credential_recipient = None;
    }
    fn apply_saved_settings(&mut self, mut settings: Settings) {
        settings.reset_consent_for_recipient_change(&self.settings);
        if settings.credential_account() != self.settings.credential_account() {
            self.clear_draft_credential();
        }
        self.settings = settings;
    }
    fn saved_credential_account(&self) -> std::result::Result<String, &'static str> {
        if self.credential_busy() {
            return Err("Wait for the current system credential operation to finish.");
        }
        if self.settings_pending {
            return Err(
                "Wait for the connection settings to finish saving before changing its credential.",
            );
        }
        let account = self.settings.credential_account();
        if account != self.visible_credential_account() {
            return Err("Save your connection settings before changing its credential.");
        }
        if self.settings.mode == "Online"
            && (self.settings.account_id.trim().len() != 32
                || !self
                    .settings
                    .account_id
                    .trim()
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("Save a valid Cloudflare account ID before using its credential.");
        }
        Ok(account)
    }
    pub fn credential_busy(&self) -> bool {
        self.credential_operation.is_some()
    }
    fn begin_credential_operation(
        &mut self,
        recipient: &str,
    ) -> std::result::Result<uuid::Uuid, &'static str> {
        if self.credential_busy() {
            return Err("Wait for the current system credential operation to finish.");
        }
        let id = uuid::Uuid::new_v4();
        self.credential_operation = Some((id, recipient.to_owned()));
        Ok(id)
    }
    fn finish_credential_operation(&mut self, id: uuid::Uuid, recipient: &str) -> bool {
        if !self
            .credential_operation
            .as_ref()
            .is_some_and(|(current, target)| *current == id && target == recipient)
        {
            return false;
        }
        self.credential_operation = None;
        let current = self.settings.credential_account() == recipient
            && self.visible_credential_account() == recipient;
        if current {
            self.cancel_local_connection_test();
        }
        current
    }
    pub fn save_credential(&mut self, remove: bool) {
        if self.settings_pending {
            self.notice = Some(
                "Wait for the connection settings to finish saving before changing its credential."
                    .into(),
            );
            return;
        }
        let account = match self.saved_credential_account() {
            Ok(account) => account,
            Err(message) => {
                self.notice = Some(message.into());
                return;
            }
        };
        if !remove && self.credential_recipient.as_ref() != Some(&account) {
            self.clear_draft_credential();
            self.notice =
                Some("Re-enter the access key for this saved connection before saving it.".into());
            return;
        }
        if !remove && self.credential.text.is_empty() {
            self.notice = Some("Enter an access key, or choose Remove saved credential.".into());
            return;
        }
        self.cancel_local_connection_test();
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let operation = match self.begin_credential_operation(&account) {
            Ok(operation) => operation,
            Err(message) => {
                self.notice = Some(message.into());
                return;
            }
        };
        let secret = self.credential.text.clone();
        self.clear_draft_credential();
        services.runtime.spawn_blocking(move || {
            let result = if remove {
                pickle_platform::credential_remove(&account)
            } else {
                pickle_platform::credential_set(&account, &secret)
            };
            handle.update(move |s, _| {
                if !s.finish_credential_operation(operation, &account) {
                    return;
                }
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
        let account = match self.saved_credential_account() {
            Ok(account) => account,
            Err(message) => {
                self.notice = Some(message.into());
                return;
            }
        };
        self.cancel_local_connection_test();
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let operation = match self.begin_credential_operation(&account) {
            Ok(operation) => operation,
            Err(message) => {
                self.notice = Some(message.into());
                return;
            }
        };
        services.runtime.spawn_blocking(move || {
            let result =
                pickle_platform::credential_get(&account, true).map(|token| token.is_some());
            handle.update(move |s, _| {
                if !s.finish_credential_operation(operation, &account) {
                    return;
                }
                s.notice = Some(match result {
                    Ok(true) => "Saved credential authorized by the system.".into(),
                    Ok(false) => "No credential is saved for this connection.".into(),
                    Err(error) => error.to_string(),
                })
            });
        });
    }
    pub fn save_answer(&mut self) {
        let Some(entry) = self.current_answer_entry() else {
            return;
        };
        self.append_saved_entry(entry);
    }
    pub(crate) fn current_answer_entry(&self) -> Option<Entry> {
        let snapshot = self.snapshot.as_ref()?;
        let entry = if let Some(turn) = self.conversation.last() {
            let fallback = RecordedIntent::FollowUp(turn.question.clone());
            let (action, question) = self
                .answer_references
                .follow_up_intent
                .as_ref()
                .unwrap_or(&fallback)
                .fields();
            Entry {
                id: uuid::Uuid::new_v4().to_string(),
                recorded_at: chrono::Utc::now().to_rfc3339(),
                app_name: snapshot.app_name.clone(),
                app_id: snapshot.app_id.clone(),
                action: action.into(),
                question: Some(question.into()),
                passage: snapshot.text.clone(),
                answer: turn.answer.clone(),
                quality: turn.quality.visible_label(),
                chart: None,
                reference: self.answer_references.follow_up.clone(),
            }
        } else {
            let result = self.result.as_ref()?;
            let mut entry = make_entry(snapshot, result, &Pending::action(result.action));
            entry.reference = self.answer_references.base.clone();
            entry
        };
        Some(entry)
    }
    pub fn remove_entry(&mut self, bookmark: bool, id: String) {
        self.remove_library_entry(bookmark, id);
    }
    pub fn answer(&self) -> &str {
        self.conversation
            .last()
            .map(|t| t.answer.as_str())
            .or_else(|| self.result.as_ref().map(|r| r.text.as_str()))
            .unwrap_or("")
    }
    pub fn import_legacy(&mut self) {
        self.import_library_legacy();
    }
    fn import_library_paths(&mut self) -> Option<crate::migration::LegacyPaths> {
        let Some(paths) = crate::migration::default_legacy_paths() else {
            self.notice = Some(
                "Legacy import is available on macOS, where the original app stored its data."
                    .into(),
            );
            return None;
        };
        Some(paths)
    }
}
fn save_settings_transaction(
    repo: &Repository,
    settings: &Settings,
) -> std::result::Result<(), String> {
    let prepared = pickle_platform::prepare_local_transcriber(settings.transcriber_config()?)
        .map_err(|error| error.to_string())?;
    commit_prepared_settings(repo, settings, prepared)
}

fn commit_prepared_settings(
    repo: &Repository,
    settings: &Settings,
    transcriber: pickle_platform::PreparedLocalTranscriber,
) -> std::result::Result<(), String> {
    // Preparing does not mutate the native cache. A failed atomic write leaves
    // both active destinations intact; after success installation is infallible
    // and does not revalidate files that could disappear during disk I/O.
    repo.save_settings(settings)?;
    pickle_platform::install_local_transcriber(transcriber);
    Ok(())
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
fn reference_pointer(reference: Option<&WebReference>) -> Option<EntryReference> {
    reference.map(|reference| EntryReference {
        url: reference.url.clone(),
        title: reference.title.clone(),
        kind: reference.kind.clone(),
    })
}
fn make_entry(snapshot: &SelectionSnapshot, result: &ReadingResult, pending: &Pending) -> Entry {
    let intent = pending.intent();
    let (action, question) = intent.as_ref().map_or_else(
        || {
            (
                pending.action.title(),
                (!pending.question.is_empty()).then_some(pending.question.as_str()),
            )
        },
        |intent| {
            let (action, question) = intent.fields();
            (action, Some(question))
        },
    );
    Entry {
        id: uuid::Uuid::new_v4().to_string(),
        recorded_at: chrono::Utc::now().to_rfc3339(),
        app_name: snapshot.app_name.clone(),
        app_id: snapshot.app_id.clone(),
        action: action.into(),
        question: question.map(str::to_owned),
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
mod invocation_settings_tests {
    use super::*;

    fn desktop_without_shell(
        directory: &std::path::Path,
    ) -> (
        AppState,
        Arc<Services>,
        crate::service_runtime::RuntimeOwner,
    ) {
        let repository = Arc::new(Repository::new(directory.to_owned()).unwrap());
        let (services, owner) = Services::new(Some(repository), &Settings::default()).unwrap();
        let services = Arc::new(services);
        // Construct only a handle queue; never start Rune's native event loop.
        let app = rune::App::new(rune::AppSpec::new(
            "Settings fixture",
            (560.0, 540.0),
            AppState::default,
            |_, _| rune::el(),
        ));
        (
            AppState {
                native_enabled: true,
                services: Some(services.clone()),
                handle: Some(app.handle()),
                ..AppState::default()
            },
            services,
            owner,
        )
    }

    #[test]
    fn unrelated_connection_save_can_queue_when_native_shell_is_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let (mut state, services, owner) = desktop_without_shell(directory.path());
        // Keep this admission regression free of native transcriber cache writes:
        // invalidate the queued revision before allowing the worker to proceed.
        let writer = services.settings_writer.lock().unwrap();
        state.local_model = TextEdit::new("downloaded-fixture-model");
        state.save_settings();
        assert!(state.settings_pending);
        assert!(services.policy_pending.load(Ordering::Acquire));
        assert_eq!(services.settings_revision.load(Ordering::Acquire), 1);
        assert!(state.notice.is_none());
        services.settings_revision.fetch_add(1, Ordering::AcqRel);
        drop(writer);
        assert!(owner.shutdown(std::time::Duration::from_secs(2)).finished);
    }

    #[test]
    fn changed_shortcut_save_rejects_before_storage_when_native_shell_is_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let (mut state, services, owner) = desktop_without_shell(directory.path());
        let original = state.settings.invocation_config();
        state.shortcut = TextEdit::new("ctrl+alt+q");
        state.save_settings();
        assert!(!state.settings_pending);
        assert!(!services.policy_pending.load(Ordering::Acquire));
        assert_eq!(services.settings_revision.load(Ordering::Acquire), 0);
        assert_eq!(state.settings.invocation_config(), original);
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("native invocation service")
        );
        assert!(!directory.path().join("settings.json").exists());
        assert!(owner.shutdown(std::time::Duration::from_secs(2)).finished);
    }
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

#[cfg(test)]
mod credential_tests {
    use super::*;
    const ACCOUNT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ACCOUNT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn online() -> AppState {
        AppState {
            settings: Settings {
                mode: "Online".into(),
                account_id: ACCOUNT_A.into(),
                cloud_consent: true,
                jev_consent: true,
                ..Settings::default()
            },
            account_id: TextEdit::new(ACCOUNT_A),
            ..AppState::default()
        }
    }
    fn type_key(state: &mut AppState) {
        *state.edit_credential() = TextEdit::new("private-fixture-access-key");
    }

    #[test]
    fn cloud_token_draft_cannot_be_reused_after_switching_to_a_local_recipient() {
        let mut state = online();
        type_key(&mut state);
        assert_eq!(
            state.credential_recipient,
            Some(format!("cloudflare:{ACCOUNT_A}"))
        );
        state.change_mode("Local");
        assert!(state.credential.text.is_empty());
        assert!(state.credential_recipient.is_none());
        state.save_credential(false);
        assert!(state.notice.as_deref().unwrap().contains("Re-enter"));
        assert!(state.services.is_none());
    }

    #[test]
    fn endpoint_drafts_bind_new_input_and_clear_old_input_on_rejected_save() {
        let mut state = AppState::default();
        type_key(&mut state);
        state.local_address = TextEdit::new("http://untrusted.example");
        state.save_credential(false);
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("Save your connection")
        );
        let previous = state.settings.clone();
        state.save_settings();
        assert_eq!(state.settings, previous);
        assert!(!state.settings_pending);
        assert!(state.credential.text.is_empty());
        assert!(state.credential_recipient.is_none());
        *state.edit_credential() = TextEdit::new("new-recipient-fixture-key");
        assert_eq!(
            state.credential_recipient,
            Some("local-server:http://untrusted.example".into())
        );
        assert!(!state.credential.text.contains("private-fixture"));
    }

    #[test]
    fn a_successful_staged_recipient_change_clears_draft_and_prior_cloud_consents() {
        let mut state = online();
        type_key(&mut state);
        state.settings.context_consent_destination = state.settings.context_destination();
        let mut committed = state.settings.clone();
        committed.account_id = ACCOUNT_B.into();
        state.apply_saved_settings(committed);
        assert_eq!(
            state.settings.credential_account(),
            format!("cloudflare:{ACCOUNT_B}")
        );
        assert!(!state.settings.cloud_consent);
        assert!(!state.settings.jev_consent);
        assert!(state.settings.context_consent_destination.is_empty());
        assert!(state.credential.text.is_empty());
        assert!(state.credential_recipient.is_none());
        state.account_id = TextEdit::new(ACCOUNT_B);
        state.snapshot = Some(SelectionSnapshot::new(
            "A private passage.",
            "Fixture",
            "fixture",
        ));
        let mut runtime = rune::Runtime::new(
            state,
            crate::views::view,
            (560.0, 720.0),
            rune::Env::new(true, 1.0),
        );
        runtime.update(|state, tx| state.run_action(ReadingAction::Simplify, tx));
        assert!(matches!(runtime.state.activity, Activity::Consent(_)));
        assert!(runtime.state.services.is_none());
    }

    #[test]
    fn pending_or_unsaved_recipients_cannot_authorize_or_remove_another_key() {
        let mut state = online();
        state.account_id = TextEdit::new(ACCOUNT_B);
        state.authorize_credential();
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("Save your connection")
        );
        state.save_credential(true);
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("Save your connection")
        );
        state.account_id = TextEdit::new(ACCOUNT_A);
        state.settings_pending = true;
        state.authorize_credential();
        assert!(state.notice.as_deref().unwrap().contains("finish saving"));
        assert!(state.services.is_none());
    }

    #[test]
    fn programmatically_unbound_keys_fail_before_services_or_vault_access() {
        let mut state = online();
        state.credential = TextEdit::new("unbound-fixture-key");
        state.save_credential(false);
        assert!(state.credential.text.is_empty());
        assert!(state.notice.as_deref().unwrap().contains("Re-enter"));
        assert!(state.services.is_none());
        state.account_id.clear();
        state.settings.account_id.clear();
        state.authorize_credential();
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("valid Cloudflare account")
        );
    }

    #[test]
    fn unchanged_recipient_preferences_retain_bound_input_and_granted_consent() {
        let mut state = online();
        type_key(&mut state);
        let mut committed = state.settings.clone();
        committed.text_size = 20.0;
        state.apply_saved_settings(committed);
        assert!(state.settings.cloud_consent && state.settings.jev_consent);
        assert!(!state.credential.text.is_empty());
        assert_eq!(
            state.saved_credential_account().unwrap(),
            format!("cloudflare:{ACCOUNT_A}")
        );
    }

    #[test]
    fn settings_transaction_does_not_revalidate_after_commit_or_mutate_on_failure() {
        use fs2::FileExt;
        let directory = tempfile::tempdir().unwrap();
        let repo = Arc::new(Repository::new(directory.path().join("store")).unwrap());
        let executable = directory.path().join(if cfg!(windows) {
            "whisper-cli.exe"
        } else {
            "whisper-cli"
        });
        let model = directory.path().join("fixture-model.bin");
        std::fs::write(&executable, b"fixture executable").unwrap();
        std::fs::write(&model, b"fixture model").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let next = Settings {
            local_address: "https://new-recipient.example".into(),
            transcriber_executable: executable.to_string_lossy().into_owned(),
            transcriber_model: model.to_string_lossy().into_owned(),
            ..Settings::default()
        };
        let prepared =
            pickle_platform::prepare_local_transcriber(next.transcriber_config().unwrap()).unwrap();
        let reset = pickle_platform::prepare_local_transcriber(None).unwrap();
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(repo.directory().join("store.lock"))
            .unwrap();
        FileExt::lock_exclusive(&lock).unwrap();
        let worker_repo = repo.clone();
        let worker_settings = next.clone();
        let worker = std::thread::spawn(move || {
            commit_prepared_settings(&worker_repo, &worker_settings, prepared)
        });
        // Files disappear after preparation while another writer holds the
        // same transaction lock. Installation must not fail after disk commit.
        std::fs::remove_file(&executable).unwrap();
        std::fs::remove_file(&model).unwrap();
        drop(lock);
        worker.join().unwrap().unwrap();
        assert_eq!(repo.settings().unwrap(), next);
        assert_eq!(
            pickle_platform::local_transcriber_config().unwrap().model,
            model
        );
        pickle_platform::install_local_transcriber(reset);

        // An archive rejection leaves the native cache and active endpoint
        // untouched; no rollback can itself fail by revalidating old files.
        std::fs::write(repo.directory().join("settings.json"), b"damaged archive").unwrap();
        std::fs::write(&executable, b"fixture executable").unwrap();
        std::fs::write(&model, b"fixture model").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let candidate = Settings {
            local_address: "https://rejected-recipient.example".into(),
            ..next
        };
        assert!(save_settings_transaction(&repo, &candidate).is_err());
        assert_eq!(
            std::fs::read(repo.directory().join("settings.json")).unwrap(),
            b"damaged archive"
        );
        assert!(pickle_platform::local_transcriber_config().is_none());
    }

    #[test]
    fn vault_mutations_and_authorization_are_serialized_before_any_native_call() {
        let mut state = online();
        type_key(&mut state);
        let recipient = state.saved_credential_account().unwrap();
        let first = state.begin_credential_operation(&recipient).unwrap();
        for remove in [false, true] {
            state.save_credential(remove);
            assert!(
                state
                    .notice
                    .as_deref()
                    .unwrap()
                    .contains("current system credential operation")
            );
            assert_eq!(state.credential_operation, Some((first, recipient.clone())));
        }
        state.authorize_credential();
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("current system credential operation")
        );
        assert!(state.begin_credential_operation(&recipient).is_err());
        assert!(!state.finish_credential_operation(uuid::Uuid::new_v4(), &recipient));
        assert!(state.credential_busy());
        assert!(state.finish_credential_operation(first, &recipient));
        let remove = state.begin_credential_operation(&recipient).unwrap();
        assert_ne!(remove, first);
        assert!(!state.finish_credential_operation(first, &recipient));
        assert_eq!(
            state.credential_operation,
            Some((remove, recipient.clone()))
        );
        assert!(state.finish_credential_operation(remove, &recipient));
        assert!(!state.credential_busy());
        assert!(state.services.is_none());
    }

    #[test]
    fn old_recipient_completion_unblocks_operations_without_claiming_new_target_success() {
        let mut state = online();
        let recipient = state.saved_credential_account().unwrap();
        let first = state.begin_credential_operation(&recipient).unwrap();
        state.change_mode("Local");
        assert!(state.credential_busy());
        assert!(!state.finish_credential_operation(first, &recipient));
        assert!(!state.credential_busy());
        assert!(
            state
                .begin_credential_operation(&state.settings.credential_account())
                .is_ok()
        );
    }

    #[test]
    fn credential_buttons_expose_busy_disabled_state_to_accessibility() {
        let mut state = online();
        state.route = Route::Settings;
        let recipient = state.saved_credential_account().unwrap();
        state.begin_credential_operation(&recipient).unwrap();
        let mut runtime = rune::Runtime::new(
            state,
            crate::views::view,
            (560.0, 720.0),
            rune::Env::new(true, 1.0),
        );
        runtime.frame(0.0);
        fn find<'a>(node: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
            if node["key"] == key {
                return Some(node);
            }
            node["children"]
                .as_array()?
                .iter()
                .find_map(|child| find(child, key))
        }
        let tree = runtime.accessibility_json();
        for key in [
            "save-credential",
            "remove-credential",
            "authorize-credential",
        ] {
            let node = find(&tree, key).unwrap_or_else(|| panic!("Missing {key}"));
            assert!(
                node["states"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|state| state == "disabled")
            );
        }
    }
}
