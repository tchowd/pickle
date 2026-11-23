//! Automatic selection previews never replace a reading session until an action is chosen.
use crate::controller::{Activity, AppState, Panel, Route};
use pickle_core::{SelectionBounds, SelectionSnapshot};
use pickle_platform::{CapturedSelection, InvocationConfig};
use std::sync::atomic::Ordering;

pub struct AutoPreview {
    pub capture: CapturedSelection,
    pub snapshot: SelectionSnapshot,
    previous_panel: Panel,
    previous_route: Route,
    previous_source_expanded: bool,
}

impl AppState {
    pub fn display_snapshot(&self) -> Option<&SelectionSnapshot> {
        self.auto_preview
            .as_ref()
            .map(|p| &p.snapshot)
            .or(self.snapshot.as_ref())
    }

    pub fn show_auto_capture(&mut self, capture: CapturedSelection) -> bool {
        if self.paused
            || self.settings_pending
            || self.pinned
            || !self.settings.automatic_menu
            || self.context_state.pending > 0
            || matches!(
                self.activity,
                Activity::Running { .. } | Activity::WaitingContext
            )
            || capture.text.trim().is_empty()
            || pickle_platform::source_is_excluded(&capture.app_id, &self.settings.exclusions)
        {
            return false;
        }
        let identity = (capture.app_id.clone(), capture.text.clone());
        if self.last_auto_selection.as_ref() == Some(&identity) {
            return false;
        }
        let (previous_panel, previous_route, previous_source_expanded) = self
            .auto_preview
            .as_ref()
            .map(|p| {
                (
                    p.previous_panel,
                    p.previous_route,
                    p.previous_source_expanded,
                )
            })
            .unwrap_or((self.panel, self.route, self.source_expanded));
        if self.auto_preview.is_none() {
            self.remember_window_geometry();
        }
        let mut snapshot =
            SelectionSnapshot::new(&capture.text, &capture.app_name, &capture.app_id);
        snapshot.method = "Accessibility".into();
        snapshot.source_url = capture.source_url.clone();
        snapshot.bounds = capture.bounds.as_ref().map(|b| SelectionBounds {
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
        });
        self.auto_preview = Some(AutoPreview {
            capture,
            snapshot,
            previous_panel,
            previous_route,
            previous_source_expanded,
        });
        self.last_auto_selection = Some(identity);
        self.panel = Panel::Choosing;
        self.source_expanded = false;
        self.route = Route::Reader;
        true
    }

    pub fn dismiss_auto_preview(&mut self) {
        if let Some(preview) = self.auto_preview.take() {
            self.panel = preview.previous_panel;
            self.route = preview.previous_route;
            self.source_expanded = preview.previous_source_expanded;
            if preview.previous_panel != Panel::Choosing {
                self.restore_window_geometry();
            }
        }
    }

    pub fn commit_auto_preview(&mut self) -> bool {
        let Some(preview) = self.auto_preview.take() else {
            return true;
        };
        self.panel = preview.previous_panel;
        self.route = preview.previous_route;
        self.source_expanded = preview.previous_source_expanded;
        if self.paused
            || self.settings_pending
            || pickle_platform::source_is_excluded(
                &preview.capture.app_id,
                &self.settings.exclusions,
            )
        {
            if preview.previous_panel != Panel::Choosing {
                self.restore_window_geometry();
            }
            self.notice = Some(
                "This selection is no longer available under the current capture policy.".into(),
            );
            return false;
        }
        self.accept_capture(preview.capture);
        true
    }

    pub fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
        if let Some(window) = &self.window {
            window.set_always_on_top(self.pinned);
        }
        if self.pinned {
            self.dismiss_auto_preview();
        }
        if let Some(services) = &self.services {
            services.invocation_revision.fetch_add(1, Ordering::AcqRel);
            services.pinned.store(self.pinned, Ordering::Release);
        }
    }

    pub fn apply_invocation(&mut self, config: InvocationConfig) -> bool {
        if let Some(services) = &self.services {
            services.invocation_revision.fetch_add(1, Ordering::AcqRel);
        }
        let result = config.validate().and_then(|()| match &self.native_shell {
            Some(shell) => shell.configure_invocation(config),
            None if self.native_enabled => Err(pickle_platform::PlatformError(
                "The native invocation service is unavailable. Restart Pickle before changing its invocation settings.".into(),
            )),
            None => Ok(()),
        });
        if let Err(error) = result {
            self.notice = Some(error.to_string());
            return false;
        }
        true
    }

    pub fn set_automatic_menu(&mut self, enabled: bool) {
        if self.settings_pending {
            return;
        }
        let mut config = self.settings.invocation_config();
        config.automatic_menu = enabled;
        if self.apply_invocation(config) {
            self.settings.automatic_menu = enabled;
            if !enabled {
                self.dismiss_auto_preview();
            }
            self.persist_settings();
        }
    }

    pub fn set_modifier_gesture(&mut self, enabled: bool) {
        if self.settings_pending {
            return;
        }
        let mut config = self.settings.invocation_config();
        config.modifier_gesture = enabled;
        if self.apply_invocation(config) {
            self.settings.modifier_gesture = enabled;
            self.persist_settings();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_without_native_shell_rejects_invocation_changes() {
        let mut state = AppState {
            native_enabled: true,
            ..AppState::default()
        };
        let original = state.settings.invocation_config();
        state.set_modifier_gesture(!original.modifier_gesture);
        assert_eq!(state.settings.invocation_config(), original);
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("native invocation service")
        );
        state.set_automatic_menu(!original.automatic_menu);
        assert_eq!(state.settings.invocation_config(), original);
        assert!(!state.settings_pending);
    }
    #[test]
    fn isolated_fixtures_can_validate_invocation_without_native_shell() {
        let mut state = AppState::default();
        assert!(state.apply_invocation(InvocationConfig::default()));
        assert!(state.notice.is_none());
        assert!(!state.apply_invocation(InvocationConfig {
            shortcut: "invalid".into(),
            ..InvocationConfig::default()
        }));
    }
    fn capture(text: &str) -> CapturedSelection {
        CapturedSelection {
            text: text.into(),
            app_name: "Editor".into(),
            app_id: "editor".into(),
            bounds: None,
            source_url: None,
            source_pid: 0,
            source_window: None,
        }
    }
    #[test]
    fn automatic_preview_does_not_replace_or_cancel_working_session() {
        let mut state = AppState::default();
        state.sample_result();
        let id = state.snapshot.as_ref().unwrap().id;
        state.settings.automatic_menu = true;
        assert!(state.show_auto_capture(capture("A different passage.")));
        assert_eq!(state.snapshot.as_ref().unwrap().id, id);
        assert_eq!(
            state.display_snapshot().unwrap().text,
            "A different passage."
        );
        assert!(state.result.is_some());
        assert_eq!(state.context_state.pending, 0);
        state.dismiss_auto_preview();
        assert_eq!(state.panel, Panel::Expanded);
        assert_eq!(state.display_snapshot().unwrap().id, id);
        assert!(!state.show_auto_capture(capture("A different passage.")));
    }
    #[test]
    fn chooser_commit_rechecks_policy_and_never_runs_against_old_source() {
        let mut state = AppState::default();
        state.sample_result();
        let id = state.snapshot.as_ref().unwrap().id;
        state.settings.automatic_menu = true;
        assert!(state.show_auto_capture(capture("A different passage.")));
        state.settings.exclusions.push("editor".into());
        assert!(!state.commit_auto_preview());
        assert_eq!(state.snapshot.as_ref().unwrap().id, id);
        state.settings.exclusions.clear();
        assert!(state.show_auto_capture(capture("Another passage.")));
        assert!(state.commit_auto_preview());
        assert_eq!(state.snapshot.as_ref().unwrap().text, "Another passage.");
        assert!(state.result.is_none());
        assert!(!state.demo);
    }
    #[test]
    fn automatic_invocation_respects_pin_busy_pending_and_current_policy() {
        let mut state = AppState::default();
        state.settings.automatic_menu = true;
        state.toggle_pin();
        assert!(!state.show_auto_capture(capture("First.")));
        state.toggle_pin();
        state.settings_pending = true;
        assert!(!state.show_auto_capture(capture("First.")));
        state.settings_pending = false;
        let token = tokio_util::sync::CancellationToken::new();
        state.activity = Activity::Running {
            id: 1,
            cancel: token.clone(),
            progress: "Writing".into(),
        };
        assert!(!state.show_auto_capture(capture("First.")));
        assert!(!token.is_cancelled());
        state.cancel();
        assert!(state.show_auto_capture(capture("First.")));
        state.toggle_pin();
        assert!(state.auto_preview.is_none());
    }
}
