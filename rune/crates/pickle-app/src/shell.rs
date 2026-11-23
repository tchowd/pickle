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
                        s.cancel_remove_all();
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
                    s.cancel_remove_all();
                    s.prepare_window_close();
                    s.cancel();
                    if let Some(services) = &s.services {
                        services.shutdown.cancel();
                    }
                    tx.quit();
                    return;
                }
                PlatformEvent::Dismiss => {
                    s.cancel_remove_all();
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
                    s.cancel_remove_all();
                    s.dismiss_auto_preview();
                    s.stop_reader_follow();
                    s.route = Route::Settings;
                    s.expand(tx, 720.0);
                }
                PlatformEvent::History => {
                    s.cancel_remove_all();
                    s.dismiss_auto_preview();
                    s.stop_reader_follow();
                    s.library_selected = None;
                    s.library_page = 0;
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
            handle.update(move |s, _| prepare_browser_request(s, request));
        }
    });
}

fn browser_denial(
    state: &AppState,
    payload: &pickle_providers::browser::BrowserPayload,
) -> Option<&'static str> {
    if state.paused {
        Some("Pickle is paused.")
    } else if state.settings_pending {
        Some("Wait for Pickle's settings to finish saving.")
    } else if !state.settings.web_context_enabled {
        Some("Enable browser references in Pickle Settings first.")
    } else if payload.is_excluded(&state.settings.exclusions) {
        Some("This browser is excluded in Pickle Settings.")
    } else if payload.validate().is_err() {
        Some("This browser capture is invalid or expired.")
    } else {
        None
    }
}

fn prepare_browser_request(
    state: &mut AppState,
    request: pickle_providers::browser::BrowserRequest,
) {
    let (Some(services), Some(handle)) = (state.services.clone(), state.handle.clone()) else {
        let _ = request
            .reply
            .send(Err("The reader is not ready. Try again.".into()));
        return;
    };
    let attempt = match stage_browser_intent(state, &request) {
        Ok(attempt) => attempt,
        Err(error) => {
            let _ = request.reply.send(Err(error.into()));
            return;
        }
    };
    let cancel = services.shutdown.child_token();
    let expected = request.payload.browser_id.clone();
    services.runtime.spawn(async move {
        let target = tokio::select! {
            _ = cancel.cancelled() => return,
            target = resolve_browser_target(move || pickle_platform::capture_browser_target(&expected)) => target,
        };
        handle.update(move |state, tx| complete_browser_request(state, request, target, attempt, tx));
    });
}

#[derive(Clone, Copy)]
struct BrowserAttempt {
    revision: u64,
    id: uuid::Uuid,
}
fn stage_browser_intent(
    state: &mut AppState,
    request: &pickle_providers::browser::BrowserRequest,
) -> Result<BrowserAttempt, &'static str> {
    if let Some(reason) = browser_denial(state, &request.payload) {
        return Err(reason);
    }
    if !request.is_pending() {
        return Err("This browser capture expired. Send it again from the browser.");
    }
    let id = uuid::Uuid::new_v4();
    state.context_state.browser_attempt = Some(id);
    Ok(BrowserAttempt {
        revision: state.revision,
        id,
    })
}

async fn resolve_browser_target(
    resolver: impl FnOnce()
        -> Result<pickle_platform::CapturedSelection, pickle_platform::PlatformError>
    + Send
    + 'static,
) -> Result<pickle_platform::CapturedSelection, pickle_platform::PlatformError> {
    // Blocking native metadata never runs on the UI thread. A timed-out worker
    // remains tracked by whole-runtime shutdown and retains the platform's
    // single-flight gate until it actually returns.
    tokio::time::timeout(std::time::Duration::from_millis(400), tokio::task::spawn_blocking(resolver))
        .await.ok().and_then(Result::ok).unwrap_or_else(|| Err(pickle_platform::PlatformError("Native browser source resolution timed out. Send the reference again before listening.".into())))
}

fn complete_browser_request(
    state: &mut AppState,
    request: pickle_providers::browser::BrowserRequest,
    target: Result<pickle_platform::CapturedSelection, pickle_platform::PlatformError>,
    attempt: BrowserAttempt,
    tx: &mut rune::Tx,
) {
    if state.revision != attempt.revision || state.context_state.browser_attempt != Some(attempt.id)
    {
        let _ = request.reply.send(Err(
            "The reading source changed. Send the browser reference again.".into(),
        ));
        return;
    }
    if let Some(reason) = browser_denial(state, &request.payload) {
        let _ = request.reply.send(Err(reason.into()));
        return;
    }
    // This is the atomic, monotonic admission boundary immediately before any
    // session mutation or focus change, after potentially delayed OS work.
    if let Err(error) = request.try_admit() {
        let _ = request.reply.send(Err(error));
        return;
    }
    let payload = request.payload;
    state.reference_from_browser(
        payload.selection,
        payload.browser_id,
        payload.reference,
        target,
    );
    state.expand(tx, 360.0);
    if let Some(handle) = &state.handle {
        handle.open();
    }
    let _ = request.reply.send(Ok(()));
}

#[cfg(test)]
#[path = "browser_context_tests.rs"]
mod browser_context_tests;
