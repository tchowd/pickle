use crate::controller::{Activity, AppState, Pending};
use pickle_core::WebReference;
use pickle_platform::{CapturedPage, CapturedSelection};
use tokio_util::sync::CancellationToken;

pub struct ContextState {
    pub id: uuid::Uuid,
    pub cancel: CancellationToken,
    pub pending: usize,
    pub action: Option<Pending>,
    pub page: Option<CapturedPage>,
    pub page_image: Option<rune::Image>,
    pub preview_full: bool,
    pub reference: Option<WebReference>,
    pub visual_summary: String,
    pub audio_text: String,
    pub notice: Option<String>,
}
impl Default for ContextState {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            cancel: CancellationToken::new(),
            pending: 0,
            action: None,
            page: None,
            page_image: None,
            preview_full: false,
            reference: None,
            visual_summary: String::new(),
            audio_text: String::new(),
            notice: None,
        }
    }
}
impl ContextState {
    pub fn cancel_work(&mut self) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.id = uuid::Uuid::new_v4();
        self.pending = 0;
        self.action = None;
    }
    pub fn clear(&mut self) {
        self.cancel_work();
        self.page = None;
        self.page_image = None;
        self.preview_full = false;
        self.reference = None;
        self.visual_summary.clear();
        self.audio_text.clear();
        self.notice = None;
    }
    pub fn has_content(&self) -> bool {
        self.page.is_some()
            || self.reference.is_some()
            || !self.visual_summary.is_empty()
            || !self.audio_text.is_empty()
    }
    pub fn reading_text(&self) -> String {
        let mut text = self
            .page
            .as_ref()
            .map(|p| p.text.clone())
            .unwrap_or_default();
        if !self.audio_text.is_empty() {
            text.push_str("\nLocally transcribed source-app audio (may contain errors):\n");
            text.push_str(&self.audio_text);
        }
        pickle_core::bounded_utf8(&text, pickle_core::limits::PAGE_CONTEXT).to_owned()
    }
}
impl AppState {
    fn context_policy(&mut self) -> bool {
        if self.settings_pending {
            self.notice = Some(
                "Wait for your connection settings to finish saving before using context tools."
                    .into(),
            );
            return false;
        }
        if self.paused {
            self.notice = Some("Pickle is paused. Resume before using context tools.".into());
            return false;
        }
        if self.snapshot.as_ref().is_some_and(|source| {
            pickle_platform::source_is_excluded(&source.app_id, &self.settings.exclusions)
        }) {
            self.notice = Some("This source application is excluded.".into());
            return false;
        }
        true
    }
    pub fn collect_context(&mut self) {
        if !self.context_policy() {
            return;
        }
        let (Some(services), Some(handle), Some(target)) = (
            self.services.clone(),
            self.handle.clone(),
            self.capture_target.clone(),
        ) else {
            return;
        };
        self.context_state.clear();
        let id = self.context_state.id;
        if self.settings.screen_context_enabled
            && pickle_platform::capabilities().window_capture_ocr
        {
            self.context_state.pending += 1;
            let handle = handle.clone();
            let cancel = self.context_state.cancel.child_token();
            let target = target.clone();
            services.runtime.spawn(async move {
                let worker_cancel = cancel.clone();
                let work = tokio::task::spawn_blocking(move || pickle_platform::capture_page_with_cancel(&target, worker_cancel));
                let outcome = tokio::select! {
                    _ = cancel.cancelled() => return,
                    result = tokio::time::timeout(std::time::Duration::from_secs(5), work) => result.ok().and_then(|r| r.ok()),
                };
                cancel.cancel();
                handle.update(move |s, tx| {
                    if s.context_state.id != id { return; }
                    match outcome {
                        Some(Ok(page)) => {
                            s.context_state.page_image = s.image_cache.as_ref().map(|cache| cache.load(page.jpeg.clone()));
                            s.context_state.page = Some(page);
                        },
                        Some(Err(error)) => s.context_state.notice = Some(error.to_string()),
                        None => s.context_state.notice = Some("Page capture timed out. Reading will use your passage.".into()),
                    }
                    s.context_state.pending = s.context_state.pending.saturating_sub(1); s.context_ready(tx);
                });
            });
        }
        if self.settings.web_context_enabled
            && !self.settings.strict_offline
            && let Some(url) = target.source_url
        {
            self.context_state.pending += 1;
            let cancel = self.context_state.cancel.child_token();
            let selection = target.text;
            services.runtime.spawn(async move {
                let outcome =
                    pickle_providers::fetch_public_reference(&url, &selection, false, cancel).await;
                handle.update(move |s, tx| {
                    if s.context_state.id != id {
                        return;
                    }
                    match outcome {
                        Ok(reference) => s.context_state.reference = Some(reference),
                        Err(error) => s.context_state.notice = Some(error.to_string()),
                    }
                    s.context_state.pending = s.context_state.pending.saturating_sub(1);
                    s.context_ready(tx);
                });
            });
        }
        services.busy.store(
            self.context_state.pending > 0,
            std::sync::atomic::Ordering::Release,
        );
    }
    pub fn context_ready(&mut self, tx: &mut rune::Tx) {
        if self.context_state.pending == 0
            && !matches!(self.activity, Activity::Running { .. })
            && let Some(services) = &self.services
        {
            services
                .busy
                .store(false, std::sync::atomic::Ordering::Release);
        }
        if self.context_state.pending == 0
            && matches!(self.activity, Activity::WaitingContext)
            && let Some(action) = self.context_state.action.take()
        {
            self.start(action, tx);
        }
    }
    pub fn clear_context(&mut self) {
        self.cancel();
        self.context_state.clear();
    }
    pub fn begin_audio(&mut self) {
        if self.context_state.pending > 0 {
            return;
        }
        if !self.context_policy() {
            return;
        }
        let (Some(services), Some(handle), Some(target)) = (
            self.services.clone(),
            self.handle.clone(),
            self.capture_target.clone(),
        ) else {
            self.notice = Some("Invoke Pickle from the source app before listening.".into());
            return;
        };
        if !pickle_platform::capabilities().source_audio {
            self.notice = Some("Source-app transcription is unavailable on this system.".into());
            return;
        }
        self.cancel();
        let id = self.context_state.id;
        let cancel = self.context_state.cancel.child_token();
        self.context_state.pending = 1;
        services
            .busy
            .store(true, std::sync::atomic::Ordering::Release);
        self.notice = Some("Listening for up to 30 seconds. Other playing audio in the same app may be included. No microphone or audio file is used.".into());
        services.runtime.spawn(async move {
            let outcome =
                tokio::task::spawn_blocking(move || pickle_platform::record_audio(&target, cancel))
                    .await;
            handle.update(move |s, tx| {
                if s.context_state.id != id {
                    return;
                }
                match outcome {
                    Ok(Ok(text)) => s.apply_audio_text(text),
                    Ok(Err(error)) => s.context_state.notice = Some(error.to_string()),
                    Err(_) => {
                        s.context_state.notice =
                            Some("Source audio could not be transcribed.".into())
                    }
                }
                s.context_state.pending = 0;
                s.notice = None;
                s.context_ready(tx);
            });
        });
    }
    pub fn begin_visual_analysis(&mut self) {
        if self.context_state.pending > 0 {
            return;
        }
        if !self.context_policy() {
            return;
        }
        let Some(page) = &self.context_state.page else {
            self.notice = Some("Capture page context before analyzing visuals.".into());
            return;
        };
        let local = self.settings.mode == "Local";
        if local && self.settings.local_vision_model.is_empty() {
            self.notice = Some(
                "Configure a downloaded local vision model in Settings before analyzing images."
                    .into(),
            );
            return;
        }
        if !local && self.settings.strict_offline {
            self.notice = Some("Offline mode blocks cloud visual analysis.".into());
            return;
        }
        let (Some(services), Some(handle)) = (self.services.clone(), self.handle.clone()) else {
            return;
        };
        let jpeg = page.jpeg.clone();
        let selection = self
            .snapshot
            .as_ref()
            .map(|s| s.text.clone())
            .unwrap_or_default();
        self.cancel();
        let id = self.context_state.id;
        let cancel = self.context_state.cancel.child_token();
        let settings = self.settings.clone();
        self.context_state.pending = 1;
        services
            .busy
            .store(true, std::sync::atomic::Ordering::Release);
        self.notice = Some(if local {
            "Analyzing visuals with your Local server…".into()
        } else {
            "Analyzing visuals with Cloudflare…".into()
        });
        services.runtime.spawn(async move {
            let result = async {
                let credential_account = settings.credential_account();
                let token = tokio::task::spawn_blocking(move || {
                    pickle_platform::credential_get(&credential_account, false)
                })
                .await
                .map_err(|_| "Could not read the credential.".to_owned())?
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
                if local {
                    let config = pickle_providers::LocalAIConfiguration {
                        address: settings.local_address,
                        model: settings.local_vision_model,
                        token,
                        same_device: settings.same_device,
                    };
                    config
                        .validate(settings.strict_offline)
                        .map_err(|e| e.to_string())?;
                    pickle_providers::OllamaProvider::new(config)
                        .summarize(&jpeg, &selection, cancel)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    pickle_providers::VisualContextClient::new(settings.account_id, token)
                        .summarize(&jpeg, &selection, cancel)
                        .await
                        .map_err(|e| e.to_string())
                }
            }
            .await;
            handle.update(move |s, tx| {
                if s.context_state.id != id {
                    return;
                }
                match result {
                    Ok(summary) => s.context_state.visual_summary = summary,
                    Err(error) => s.context_state.notice = Some(error),
                }
                s.context_state.pending = 0;
                s.notice = None;
                s.context_ready(tx);
            });
        });
    }
    pub fn reference_from_browser(
        &mut self,
        selection: String,
        app_id: String,
        reference: WebReference,
    ) {
        self.reset();
        let mut snapshot =
            pickle_core::SelectionSnapshot::new(selection, "Browser reference", app_id);
        snapshot.source_url = Some(reference.url.clone());
        snapshot.source_title = Some(reference.title.clone());
        self.source = rune::TextEdit::new(&snapshot.text);
        self.snapshot = Some(snapshot);
        self.context_state.reference = Some(reference);
        self.panel = crate::controller::Panel::Choosing;
    }
    pub fn apply_audio_text(&mut self, text: String) {
        if let Some(url) = self.snapshot.as_ref().and_then(|s| s.source_url.clone()) {
            self.context_state.reference = Some(WebReference {
                url,
                title: "Locally transcribed source-app audio".into(),
                kind: "recorded audio".into(),
                text,
                timestamp: None,
            });
        } else {
            self.context_state.audio_text = text;
        }
    }
}

// Captured target metadata is session-only and never reaches history or provider payloads.
pub type CaptureTarget = CapturedSelection;

#[cfg(test)]
mod pending_operation_tests {
    use super::*;
    use crate::controller::Panel;
    use pickle_core::{ReadingAction, SelectionSnapshot};
    use rune::{Env, Runtime};

    fn pending() -> AppState {
        let mut state = AppState {
            snapshot: Some(SelectionSnapshot::new(
                "A controlled source passage.",
                "Fixture",
                "fixture.editor",
            )),
            panel: Panel::Expanded,
            activity: Activity::WaitingContext,
            // Without the pending guard, policy validation would replace the
            // notice before any service check. No live service is constructed.
            paused: true,
            notice: Some("Existing operation remains active.".into()),
            ..AppState::default()
        };
        state.context_state.pending = 2;
        state.context_state.action = Some(Pending::action(ReadingAction::Simplify));
        state.context_state.page = Some(CapturedPage {
            jpeg: vec![1, 2, 3],
            text: "Controlled OCR fixture.".into(),
            truncated: false,
        });
        state
    }

    fn unchanged(state: &AppState, id: uuid::Uuid, cancel: &CancellationToken) {
        assert_eq!(state.context_state.id, id);
        assert_eq!(state.context_state.pending, 2);
        assert!(!cancel.is_cancelled());
        assert_eq!(
            state.context_state.action.as_ref().unwrap().action,
            ReadingAction::Simplify
        );
        assert!(matches!(state.activity, Activity::WaitingContext));
        assert_eq!(
            state.notice.as_deref(),
            Some("Existing operation remains active.")
        );
        assert!(state.services.is_none() && state.handle.is_none());
    }

    #[test]
    fn pending_audio_and_visual_tools_return_before_policy_or_service_work() {
        let mut state = pending();
        let id = state.context_state.id;
        let cancel = state.context_state.cancel.clone();
        state.begin_visual_analysis();
        unchanged(&state, id, &cancel);
        state.begin_audio();
        unchanged(&state, id, &cancel);
    }

    #[test]
    fn disabled_context_buttons_preserve_the_existing_operation_when_hit() {
        let state = pending();
        let id = state.context_state.id;
        let cancel = state.context_state.cancel.clone();
        let mut runtime = Runtime::new(
            state,
            crate::views::view,
            (560.0, 1800.0),
            Env::new(true, 1.0),
        );
        fn disabled(node: &serde_json::Value, key: &str) -> bool {
            if node["key"] == key {
                return node["states"]
                    .as_array()
                    .is_some_and(|states| states.iter().any(|state| state == "disabled"));
            }
            node["children"]
                .as_array()
                .is_some_and(|children| children.iter().any(|child| disabled(child, key)))
        }
        for key in ["analyze-visuals", "listen"] {
            for _ in 0..60 {
                runtime.frame(1.0 / 60.0);
            }
            assert!(disabled(&runtime.accessibility_json(), key));
            let (x, y) = runtime.find(key).unwrap();
            assert!(
                y > 0.0 && y < 1800.0,
                "The disabled control must be in the fixture viewport."
            );
            runtime.pointer_down(x, y);
            runtime.pointer_up(x, y);
            runtime.frame(1.0 / 60.0);
            unchanged(&runtime.state, id, &cancel);
        }
    }
}
