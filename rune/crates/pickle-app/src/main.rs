#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
mod browser_setup;
mod context;
mod controller;
mod diagnostics;
mod geometry_store;
mod instance;
mod invocation_ui;
mod library_search;
mod lifecycle;
mod local_connection;
mod maintenance;
mod migration;
mod preferences_view;
mod release_verify;
#[cfg(test)]
mod review_tests;
mod scenes;
mod service_runtime;
mod shell;
mod startup_presenter;
mod storage;
#[cfg(test)]
mod tests;
mod update;
mod update_ui;
mod views;

use controller::{AppState, Panel, Services};
use rune::{App, AppSpec, Key, KeyEvent, MacWindowOptions, Tx, WindowOptions};
use std::sync::Arc;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if let Some(exit_code) = startup_presenter::handle_helper(&arguments) {
        std::process::exit(exit_code);
    }
    let arguments = scenes::normalize_arguments(arguments);
    if let Some(exit_code) = release_verify::handle(&arguments) {
        std::process::exit(exit_code);
    }
    if arguments.iter().any(|arg| arg == "--verify-windows-ocr") {
        if arguments.len() != 1 {
            eprintln!("The fixed OCR probe cannot be combined with other options.");
            std::process::exit(2);
        }
        match pickle_platform::verify_ocr_runtime() {
            Ok(()) => {
                println!(r#"{{"schema":1,"test":"windows-ocr-fixed-fixture","passed":true}}"#)
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(exit_code) = maintenance::handle(&arguments) {
        std::process::exit(exit_code);
    }
    if arguments.iter().any(|arg| arg == "--version") {
        println!("Pickle Rune {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if arguments.iter().any(|arg| arg == "--capabilities") {
        println!(
            "{}",
            serde_json::to_string_pretty(&pickle_platform::capabilities()).unwrap()
        );
        return;
    }
    if arguments.iter().any(|arg| arg == "--permissions") {
        println!(
            "{}",
            serde_json::to_string_pretty(&pickle_platform::permission_status()).unwrap()
        );
        return;
    }
    let sample = arguments.iter().any(|arg| arg == "--sample");
    let background = arguments.iter().any(|arg| arg == "--background");
    let headless = scenes::isolated_startup(&arguments);
    let interactive_failure = lifecycle::interactive_failure(&arguments, headless);
    let mut instance = if headless {
        None
    } else {
        match instance::InstanceGuard::acquire(&storage::Repository::default_directory()) {
            Ok(instance::InstanceStatus::Primary(guard)) => Some(guard),
            Ok(instance::InstanceStatus::Secondary) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    };
    let mut initial = AppState {
        native_enabled: !headless,
        ..AppState::default()
    };
    let mut startup_error = None;
    let repository = if headless {
        None
    } else {
        match storage::Repository::new(storage::Repository::default_directory()) {
            Ok(repo) => {
                match repo.settings() {
                    Ok(settings) => initial.load_settings(settings),
                    Err(error) => startup_error = Some(error),
                }
                match repo.history() {
                    Ok(entries) => initial.entries = entries.into(),
                    Err(error) => initial.library_errors[0] = Some(error),
                }
                match repo.bookmarks() {
                    Ok(entries) => initial.bookmarks = entries.into(),
                    Err(error) => initial.library_errors[1] = Some(error),
                }
                Some(Arc::new(repo))
            }
            Err(error) => {
                startup_error = Some(error);
                None
            }
        }
    };
    let (services, runtime_owner) = match Services::new(repository, &initial.settings) {
        Ok(started) => started,
        Err(_) => {
            // No app jobs or geometry worker exist yet. Keep the lease through
            // the dialog so installers still see this loaded executable.
            eprintln!("Pickle could not start background services (E_RUNTIME_START).");
            if interactive_failure && !startup_presenter::present() {
                eprintln!("Pickle could not show its startup error (E_ERROR_PRESENTER).");
            }
            drop(instance.take());
            std::process::exit(1);
        }
    };
    let services = Arc::new(services);
    let geometry_worker = services.repository.as_ref().and_then(|repository| {
        match geometry_store::Worker::start(repository.directory().to_owned()) {
            Ok(started) => {
                match started.loaded {
                    Ok(frame) => initial.window_geometry = frame,
                    Err(error) => {
                        startup_error.get_or_insert(error);
                    }
                }
                Some(started.worker)
            }
            Err(error) => {
                startup_error.get_or_insert(error);
                None
            }
        }
    });
    initial.window_geometry_worker = geometry_worker.clone();
    if !headless && let Err(error) = initial.settings.configure_transcriber() {
        startup_error = Some(error);
    }
    if !headless {
        initial.services = Some(services.clone());
    }
    initial.notice = startup_error;
    if sample {
        initial.sample();
    }
    let spec = AppSpec {
        scenes: scenes::SCENES,
        setup: Some(shell::setup),
        on_key: Some(on_key),
        poll: Some(library_search::poll),
        min_size: Some((480.0, 180.0)),
        ..AppSpec::new(
            "Pickle",
            (560.0, if sample { 300.0 } else { 540.0 }),
            AppState::default,
            views::view,
        )
    };
    let options = WindowOptions {
        borderless: true,
        transparent: true,
        always_on_top: false,
        min_size: Some((480.0, 180.0)),
        resizable: true,
        ..Default::default()
    }
    .macos(
        MacWindowOptions::default()
            .nonactivating_panel(true)
            .corner_radius(16.0)
            .level(0)
            .collection_behavior((1 << 0) | (1 << 8)),
    );
    let app = App::new(spec)
        .accessory()
        .keep_running()
        .configured_options(options);
    let handle = app.handle();
    initial.handle = Some(handle.clone());
    if let Some(receiver) = instance.as_mut().and_then(|guard| guard.take_receiver()) {
        let handle = handle.clone();
        let shutdown = services.shutdown.clone();
        std::thread::spawn(move || {
            while !shutdown.is_cancelled() {
                match receiver.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(instance::InstanceEvent::Reopen) => {
                        handle.update(|state, _| state.resume_library_search());
                        handle.open();
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
        });
    }
    let app = app
        .init(move || initial)
        .args(
            arguments
                .into_iter()
                .filter(|arg| !["--sample", "--background"].contains(&arg.as_str())),
        )
        .on_close(|s, _| {
            s.prepare_window_close();
            s.cancel();
            s.geometry_close_prepared = false;
        })
        .on_quit(|s, _| {
            s.prepare_window_close();
            s.cancel();
        })
        .on_reopen(|s, _, _| {
            s.resume_library_search();
            s.geometry_close_prepared = false;
            // Rune deliberately leaves accessory apps/nonactivating panels out of its default reopen policy.
            if let Some(handle) = &s.handle {
                handle.open();
            }
            true
        });
    let app = if background { app.windowless() } else { app };
    let health_handle = handle.clone();
    let app = app.on_ready(move |window| {
        #[cfg(target_os = "macos")]
        let drag_error = if headless {
            None
        } else {
            pickle_platform::configure_panel_drag(window.window_number())
                .err()
                .map(|error| error.to_string())
        };
        let window = window.clone();
        handle.update(move |s, _| {
            s.window = Some(window);
            #[cfg(target_os = "macos")]
            if let Some(error) = drag_error {
                s.notice = Some(error);
            }
            if s.panel != Panel::Choosing && s.auto_preview.is_none() {
                s.restore_window_geometry();
            }
        });
    });
    // Rune otherwise exits the process directly, bypassing durable cleanup.
    rune::orderly_exit();
    let monitor = if headless {
        Ok(None)
    } else {
        let probe = health_handle.clone();
        let quit = health_handle;
        lifecycle::WindowMonitor::start(
            move || probe.window_error().is_some(),
            move || {
                quit.update(|state, tx| {
                    state.cancel();
                    tx.quit();
                })
            },
        )
        .map(Some)
    };
    let (run_failed, monitor) = match monitor {
        Ok(monitor) => (app.try_run().is_err(), monitor),
        Err(()) => {
            drop(app);
            (true, None)
        }
    };
    let window_failed = monitor.is_some_and(lifecycle::WindowMonitor::finish);
    let cleaned = lifecycle::cleanup(
        || services.shutdown.cancel(),
        || {
            let stopped = runtime_owner.shutdown(lifecycle::CLEANUP_TIMEOUT);
            // Whole-runtime completion is mandatory; the wrapper count is an
            // additional consistency check, never evidence of completion alone.
            stopped.finished && stopped.unfinished_blocking == 0
        },
        || match geometry_worker {
            Some(worker) => {
                lifecycle::finish_with_timeout(move || worker.finish(), lifecycle::CLEANUP_TIMEOUT)
            }
            None => Ok(Ok(())),
        },
    );
    match lifecycle::exit_decision(
        run_failed,
        window_failed,
        interactive_failure,
        cleaned,
        || {
            if !startup_presenter::present() {
                eprintln!("Pickle could not show its startup error (E_ERROR_PRESENTER).");
            }
        },
        || {
            drop(instance.take());
        },
    ) {
        lifecycle::Exit::Success => {}
        lifecycle::Exit::UnsafeShutdown => {
            // Keep the lease until process termination. A modal dialog here
            // could leave unfinished writers alive indefinitely awaiting ACK.
            eprintln!("Pickle could not finish shutdown safely (E_SHUTDOWN_TIMEOUT).");
            std::process::exit(1);
        }
        lifecycle::Exit::WriteFailure => {
            eprintln!("Pickle could not finish saving window preferences (E_SHUTDOWN_WRITE).");
            std::process::exit(1);
        }
        lifecycle::Exit::ReaderFailure => {
            eprintln!(
                "{} (E_READER_FAILURE).",
                pickle_platform::READER_FAILURE_TITLE
            );
            std::process::exit(1);
        }
    }
}

fn on_key(s: &mut AppState, event: &KeyEvent, tx: &mut Tx) -> bool {
    match event.key {
        Key::Escape => {
            s.prepare_window_close();
            s.cancel();
            tx.close_window();
        }
        Key::Command('q') => {
            s.prepare_window_close();
            s.cancel();
            tx.quit();
        }
        Key::Command(key @ ('1' | '2' | '3')) => {
            if !s.reading_busy() {
                let action = match key {
                    '1' => pickle_core::ReadingAction::Simplify,
                    '2' => pickle_core::ReadingAction::Expand,
                    _ => pickle_core::ReadingAction::Chart,
                };
                s.run_action(action, tx);
            }
        }
        _ => return false,
    }
    true
}
