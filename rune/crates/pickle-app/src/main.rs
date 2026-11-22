#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
mod browser_setup;
mod context;
mod controller;
mod geometry_store;
mod instance;
mod invocation_ui;
mod maintenance;
mod migration;
mod preferences_view;
#[cfg(test)]
mod review_tests;
mod shell;
mod storage;
#[cfg(test)]
mod tests;
mod update;
mod update_ui;
mod views;

use controller::{AppState, Panel, Route, Services};
use rune::{App, AppSpec, Key, KeyEvent, MacWindowOptions, Tx, WindowOptions};
use std::sync::Arc;

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
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
    let headless = arguments
        .iter()
        .any(|arg| ["--snap", "--tree", "--script", "--video"].contains(&arg.as_str()));
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
                    Ok(entries) => initial.entries = entries,
                    Err(error) => startup_error = Some(error),
                }
                match repo.bookmarks() {
                    Ok(entries) => initial.bookmarks = entries,
                    Err(error) => startup_error = Some(error),
                }
                Some(Arc::new(repo))
            }
            Err(error) => {
                startup_error = Some(error);
                None
            }
        }
    };
    let services = Arc::new(Services::new(repository, &initial.settings));
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
    initial.services = Some(services.clone());
    initial.notice = startup_error;
    if sample {
        initial.sample();
    }
    type Scene = (&'static str, fn(&mut AppState));
    let scenes: &'static [Scene] = &[
        ("sample", AppState::sample_result),
        ("choosing", AppState::sample),
        ("settings", |s| s.route = Route::Settings),
        ("history", |s| s.route = Route::Library(false)),
        ("error", |s| {
            s.sample();
            s.panel = Panel::Expanded;
            s.activity = controller::Activity::Failed(
                "Can’t reach the Local server. Check that it’s awake and connected, then retry."
                    .into(),
            );
        }),
    ];
    let spec = AppSpec {
        scenes,
        setup: Some(shell::setup),
        on_key: Some(on_key),
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
                    Ok(instance::InstanceEvent::Reopen) => handle.open(),
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
            s.geometry_close_prepared = false;
            // Rune deliberately leaves accessory apps/nonactivating panels out of its default reopen policy.
            if let Some(handle) = &s.handle {
                handle.open();
            }
            true
        });
    let app = if background { app.windowless() } else { app };
    let app = app.on_ready(move |window| {
        let window = window.clone();
        handle.update(move |s, _| {
            s.window = Some(window);
            if s.panel != Panel::Choosing && s.auto_preview.is_none() {
                s.restore_window_geometry();
            }
        });
    });
    app.run();
    services.shutdown.cancel();
    if let Some(worker) = geometry_worker
        && let Err(error) = worker.finish()
    {
        eprintln!("{error}");
    }
    drop(instance);
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
        Key::Command('1') => s.run_action(pickle_core::ReadingAction::Simplify, tx),
        Key::Command('2') => s.run_action(pickle_core::ReadingAction::Expand, tx),
        Key::Command('3') => s.run_action(pickle_core::ReadingAction::Chart, tx),
        _ => return false,
    }
    true
}
