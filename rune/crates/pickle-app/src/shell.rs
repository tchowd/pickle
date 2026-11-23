use crate::controller::{AppState, Route};
use pickle_platform::{NativeShell, PlatformEvent};
use std::sync::atomic::Ordering;

pub fn setup(s: &mut AppState, waker: rune::Waker) {
    s.image_cache = Some(rune::ImageCache::new(waker));
    if !s.native_enabled {
        return;
    }
    let (Some(handle), Some(services)) = (s.handle.clone(), s.services.clone()) else {
        return;
    };
    start_browser(&services, &handle);
    match NativeShell::install(move |event| {
        let h = handle.clone();
        if matches!(event, PlatformEvent::HideChooser | PlatformEvent::Invoke) {
            services.invocation_revision.fetch_add(1, Ordering::AcqRel);
        }
        if matches!(
            event,
            PlatformEvent::Invoke | PlatformEvent::AutomaticInvoke
        ) {
            let automatic = event == PlatformEvent::AutomaticInvoke;
            if services.paused.load(Ordering::Acquire)
                || services.policy_pending.load(Ordering::Acquire)
            {
                return;
            }
            if automatic
                && (services.pinned.load(Ordering::Acquire)
                    || services.busy.load(Ordering::Acquire))
            {
                return;
            }
            let exclusions = match services.exclusions.lock() {
                Ok(policy) => policy.clone(),
                Err(_) => {
                    h.update(|s, _| s.notice = Some("The capture policy is unavailable. Restart Pickle before capturing another selection.".into()));
                    return;
                }
            };
            // Native selection is captured before any Rune window is shown or focused.
            let revision = services.invocation_revision.load(Ordering::Acquire);
            let captured = pickle_platform::capture_selection(&exclusions);
            if services.invocation_revision.load(Ordering::Acquire) != revision {
                return;
            }
            let policy = services.clone();
            h.update(move |s, tx| {
                // UI Automation can complete after pause, exclusion or settings changes.
                if policy.invocation_revision.load(Ordering::Acquire) != revision {
                    return;
                }
                if s.paused || s.settings_pending {
                    return;
                }
                match captured {
                    Ok(capture) => {
                        if pickle_platform::source_is_excluded(
                            &capture.app_id,
                            &s.settings.exclusions,
                        ) {
                            return;
                        }
                        if automatic {
                            if !s.show_auto_capture(capture) {
                                return;
                            }
                        } else {
                            s.accept_capture(capture);
                        }
                        tx.resize_window_instant((560.0, 300.0));
                    }
                    Err(error) => {
                        if automatic {
                            return;
                        }
                        s.notice = Some(error.to_string());
                        s.route = Route::Reader;
                    }
                }
                if let Some(handle) = &s.handle {
                    handle.place(
                        rune::DisplayTarget::UnderPointer,
                        rune::OverlayPlacement::Edge {
                            edge: rune::ScreenEdge::Bottom,
                            margin: 24.0,
                            align: rune::Align::Center,
                            inset: 0.0,
                        },
                    );
                    handle.show();
                }
            });
            return;
        }
        h.update(move |s, tx| {
            match event {
                PlatformEvent::Quit => {
                    s.prepare_window_close();
                    s.cancel();
                    if let Some(services) = &s.services {
                        services.shutdown.cancel();
                    }
                    tx.quit();
                    return;
                }
                PlatformEvent::Dismiss => {
                    s.prepare_window_close();
                    s.cancel();
                    tx.close_window();
                    return;
                }
                PlatformEvent::TogglePause => {
                    s.toggle_pause();
                    return;
                }
                PlatformEvent::Settings => {
                    s.dismiss_auto_preview();
                    s.stop_reader_follow();
                    s.route = Route::Settings;
                    s.expand(tx, 720.0);
                }
                PlatformEvent::History => {
                    s.dismiss_auto_preview();
                    s.stop_reader_follow();
                    s.route = Route::Library(false);
                    s.expand(tx, 680.0);
                }
                PlatformEvent::Paste => {
                    s.reset();
                    s.expand(tx, 540.0);
                    tx.focus("passage-input", false);
                }
                PlatformEvent::HideChooser => {
                    if s.auto_preview.is_some() {
                        s.dismiss_auto_preview();
                        if let Some(handle) = &s.handle {
                            handle.hide();
                        }
                    }
                    return;
                }
                PlatformEvent::Reopen | PlatformEvent::Invoke | PlatformEvent::AutomaticInvoke => {}
            }
            if let Some(handle) = &s.handle {
                handle.open();
            }
        });
    }) {
        Ok(shell) => {
            s.native_shell = Some(shell);
            s.invocation_registration = crate::invocation_ui::InvocationRegistration::NeedsRetry;
            s.apply_invocation(s.settings.invocation_config());
        }
        Err(error) => s.notice = Some(error.to_string()),
    }
}

fn start_browser(
    services: &std::sync::Arc<crate::controller::Services>,
    handle: &rune::AppHandle<AppState>,
) {
    let Some(repo) = &services.repository else {
        return;
    };
    let directory = repo.directory().join("BrowserBridge");
    let cancel = services.shutdown.child_token();
    let handle = handle.clone();
    services.runtime.spawn(async move {
        let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let _bridge = match pickle_providers::browser::BrowserBridge::start(directory, sender, cancel.clone()).await {
            Ok(bridge) => bridge,
            Err(error) => { handle.update(move |s, _| s.notice = Some(error.to_string())); return; }
        };
        loop {
            let request = tokio::select! { _ = cancel.cancelled() => break, request = requests.recv() => match request { Some(r) => r, None => break } };
            handle.update(move |s, tx| {
                let payload = request.payload;
                let denied = if s.paused { Some("Pickle is paused.") }
                    else if s.settings_pending { Some("Wait for Pickle's settings to finish saving.") }
                    else if !s.settings.web_context_enabled { Some("Enable browser references in Pickle Settings first.") }
                    else if payload.is_excluded(&s.settings.exclusions) { Some("This browser is excluded in Pickle Settings.") }
                    else if payload.validate().is_err() { Some("This browser capture is invalid or expired.") } else { None };
                if let Some(reason) = denied { let _ = request.reply.send(Err(reason.into())); return; }
                s.reference_from_browser(payload.selection, payload.browser_id, payload.reference);
                s.expand(tx, 360.0);
                if let Some(handle) = &s.handle { handle.open(); }
                let _ = request.reply.send(Ok(()));
            });
        }
    });
}
