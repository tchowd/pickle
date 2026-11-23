use super::*;
use std::{path::Path, time::Duration};

fn entry(name: &str) -> Entry {
    let mut result = AppState::demonstration_result();
    result.text = format!("Controlled answer {name}.");
    make_entry(
        &SelectionSnapshot::new(
            format!("Controlled passage {name}."),
            "Owned fixture",
            "fixture",
        ),
        &result,
        &Pending::action(ReadingAction::Simplify),
    )
}

fn loaded(repo: &Repository) -> AppState {
    AppState {
        entries: repo.history().unwrap().into(),
        bookmarks: repo.bookmarks().unwrap().into(),
        settings: Settings {
            history_enabled: true,
            ..Settings::default()
        },
        ..AppState::default()
    }
}

fn archive(directory: &Path, bookmark: bool) -> std::path::PathBuf {
    directory.join(if bookmark {
        "bookmarks.json"
    } else {
        "history.json"
    })
}

fn run_sync(state: &mut AppState, repo: &Repository, bookmark: bool, work: LibraryOperation) {
    let reload = matches!(work, LibraryOperation::Reload);
    let reservation = state.begin_library_operation(bookmark);
    let previous = reservation.wait();
    let mut outcome = perform_library_operation(repo, bookmark, work);
    reservation.complete(&mut outcome, previous);
    let id = reservation.id;
    drop(reservation);
    state.finish_library_operation(bookmark, id, outcome, reload, None);
}

/// AppHandle owns only an update queue here; no native loop or dialogs run.
/// Drain all actual service jobs before asserting/returning, even on test panic.
fn with_services(
    repo: Arc<Repository>,
    mut state: AppState,
    work: impl FnOnce(&mut AppState),
) -> AppState {
    let app = rune::App::new(rune::AppSpec::new(
        "Library controller fixture",
        (480.0, 640.0),
        AppState::default,
        |_, _| rune::el(),
    ));
    let (services, owner) = Services::new(Some(repo), &state.settings).unwrap();
    state.services = Some(Arc::new(services));
    state.handle = Some(app.handle());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&mut state)));
    let report = owner.shutdown(Duration::from_secs(5));
    assert!(report.finished && report.unfinished_blocking == 0);
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
    state
}

fn finish_drained(state: &mut AppState, repo: &Repository, bookmark: bool) {
    let index = usize::from(bookmark);
    let id = state.library_state.operations[index].unwrap();
    let failures = state.library_state.tails[index].as_ref().unwrap().wait();
    let entries = failures
        .read
        .map_or_else(|| read_library(repo, bookmark), Err);
    state.finish_library_operation(
        bookmark,
        id,
        LibraryOutcome {
            entries,
            operation_error: failures.operation,
        },
        false,
        None,
    );
}

#[test]
fn unreadable_store_keeps_rows_and_bytes_until_its_own_successful_reload() {
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().to_owned()).unwrap();
        repo.append(bookmark, entry("current")).unwrap();
        repo.append(!bookmark, entry("other")).unwrap();
        let mut state = loaded(&repo);
        let original = std::fs::read(archive(directory.path(), bookmark)).unwrap();
        let rows = state.library_entries(bookmark).to_vec();
        state.library_errors[usize::from(!bookmark)] = Some("Other archive error".into());
        state.route = Route::Library(bookmark);
        state.library_selected = Some(rows[0].id.clone());
        state.library_page = 9;
        std::fs::write(archive(directory.path(), bookmark), b"not-an-archive").unwrap();
        run_sync(
            &mut state,
            &repo,
            bookmark,
            LibraryOperation::Remove(rows[0].id.clone()),
        );
        assert!(state.library_error(bookmark).is_some());
        assert_eq!(state.library_entries(bookmark), rows);
        assert_eq!(
            std::fs::read(archive(directory.path(), bookmark)).unwrap(),
            b"not-an-archive"
        );
        run_sync(&mut state, &repo, bookmark, LibraryOperation::Reload);
        assert!(state.library_error(bookmark).is_some());
        assert_eq!(state.library_entries(bookmark), rows);
        state.request_remove_all(bookmark);
        assert!(state.library_delete_confirmation.is_none());
        // Retry only reads the repaired file and clears only this archive's guard.
        std::fs::write(archive(directory.path(), bookmark), &original).unwrap();
        run_sync(&mut state, &repo, bookmark, LibraryOperation::Reload);
        assert!(state.library_error(bookmark).is_none());
        assert_eq!(state.library_error(!bookmark), Some("Other archive error"));
        assert_eq!(state.library_entries(bookmark), rows);
        assert!(state.library_selected.is_none());
        assert_eq!(state.library_page, 0);
        assert_eq!(
            std::fs::read(archive(directory.path(), bookmark)).unwrap(),
            original
        );
        assert!(state.notice.is_none()); // No stale copy of the repaired read error.
    }
}

#[test]
fn mutation_failure_with_a_readable_archive_is_a_notice_not_an_unreadable_guard() {
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::new(directory.path().to_owned()).unwrap();
        repo.append(bookmark, entry("retained")).unwrap();
        let mut state = loaded(&repo);
        let original = std::fs::read(archive(directory.path(), bookmark)).unwrap();
        let mut invalid = entry("invalid");
        invalid.answer = "A".repeat(12_001);
        run_sync(
            &mut state,
            &repo,
            bookmark,
            LibraryOperation::Append(Box::new(invalid)),
        );
        assert!(state.library_error(bookmark).is_none());
        assert!(
            state
                .notice
                .as_deref()
                .unwrap()
                .contains("supported limits")
        );
        assert_eq!(state.library_entries(bookmark).len(), 1);
        assert_eq!(
            std::fs::read(archive(directory.path(), bookmark)).unwrap(),
            original
        );
    }
}

#[test]
fn confirmation_captures_store_and_snapshot_and_rechecks_current_guards() {
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let repo = Arc::new(Repository::new(directory.path().to_owned()).unwrap());
        repo.append(bookmark, entry("delete")).unwrap();
        repo.append(!bookmark, entry("keep-other")).unwrap();
        let state = with_services(repo.clone(), loaded(&repo), |state| {
            state.request_remove_all(bookmark);
            assert_eq!(state.library_delete_confirmation, Some(bookmark));
            state.route = Route::Library(!bookmark);
            state.confirm_remove_all();
            assert!(state.library_busy(bookmark));
            assert!(!state.library_busy(!bookmark));
            assert!(state.library_delete_confirmation.is_none());
        });
        assert!(read_library(&repo, bookmark).unwrap().is_empty());
        assert_eq!(read_library(&repo, !bookmark).unwrap().len(), 1);
        assert!(state.library_busy(bookmark)); // No host callback was consumed.

        repo.append(bookmark, entry("guarded")).unwrap();
        let state = with_services(repo.clone(), loaded(&repo), |state| {
            state.request_remove_all(bookmark);
            state.library_errors[usize::from(bookmark)] = Some("Unreadable now".into());
            state.confirm_remove_all();
            assert!(!state.library_busy(bookmark));
            assert!(state.library_delete_confirmation.is_none());
        });
        assert!(state.library_error(bookmark).is_some());
        assert_eq!(read_library(&repo, bookmark).unwrap().len(), 1);
    }
}

#[test]
fn deletion_then_new_history_append_drains_without_ui_delivery_and_stale_rows_cannot_return() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(Repository::new(directory.path().to_owned()).unwrap());
    let old = entry("old");
    let new = entry("after-confirmation");
    repo.append(false, old.clone()).unwrap();
    let mut old_operation = None;
    let mut state = with_services(repo.clone(), loaded(&repo), |state| {
        state.request_remove_all(false);
        state.confirm_remove_all();
        old_operation = state.library_state.operations[0];
        state.record_history_entry(new.clone());
        assert_ne!(old_operation, state.library_state.operations[0]);
        assert!(state.library_busy(false));
    });
    assert_eq!(repo.history().unwrap(), vec![new.clone()]);
    let latest = state.library_state.operations[0];
    state.finish_library_operation(
        false,
        old_operation.unwrap(),
        LibraryOutcome {
            entries: Ok(vec![old]),
            operation_error: Some("Obsolete result".into()),
        },
        false,
        None,
    );
    assert_eq!(state.library_state.operations[0], latest);
    assert!(state.notice.is_none());
    finish_drained(&mut state, &repo, false);
    assert_eq!(state.entries, vec![new]);
    assert!(!state.library_busy(false));
    state.finish_library_operation(
        false,
        old_operation.unwrap(),
        LibraryOutcome {
            entries: Ok(vec![]),
            operation_error: None,
        },
        false,
        None,
    );
    assert_eq!(state.entries.len(), 1);
}

#[test]
fn changed_snapshot_delete_refusal_survives_a_later_successful_history_append() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(Repository::new(directory.path().to_owned()).unwrap());
    repo.append(false, entry("reviewed")).unwrap();
    let initial = loaded(&repo);
    let unseen = entry("unseen");
    let later = entry("later-accepted-answer");
    let mut state = with_services(repo.clone(), initial, |state| {
        state.request_remove_all(false);
        repo.append(false, unseen.clone()).unwrap();
        state.confirm_remove_all();
        state.record_history_entry(later.clone());
    });
    assert_eq!(repo.history().unwrap().len(), 3);
    finish_drained(&mut state, &repo, false);
    assert_eq!(state.entries.len(), 3);
    assert!(state.entries.contains(&unseen));
    assert!(state.entries.contains(&later));
    assert!(state.notice.as_deref().unwrap().contains("changed"));
    assert!(state.library_error(false).is_none());
}

#[test]
fn queued_history_cannot_write_after_an_unpublished_failed_read_even_if_file_is_repaired() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(Repository::new(directory.path().to_owned()).unwrap());
    repo.append(false, entry("retained")).unwrap();
    let original = std::fs::read(archive(directory.path(), false)).unwrap();
    let mut state = loaded(&repo);
    std::fs::write(archive(directory.path(), false), b"damaged").unwrap();
    // Actual failed worker result exists, but its UI callback has not arrived.
    let first = state.begin_library_operation(false);
    let mut failed_read = perform_library_operation(&repo, false, LibraryOperation::Reload);
    assert!(failed_read.entries.is_err());
    first.complete(&mut failed_read, first.wait());
    drop(first);
    std::fs::write(archive(directory.path(), false), &original).unwrap();
    let mut state = with_services(repo.clone(), state, |state| {
        state.record_history_entry(entry("must-not-be-recorded-before-explicit-retry"));
    });
    assert_eq!(
        std::fs::read(archive(directory.path(), false)).unwrap(),
        original
    );
    finish_drained(&mut state, &repo, false);
    assert!(state.library_error(false).is_some());
    assert_eq!(state.entries.len(), 1);
    assert!(
        state
            .notice
            .as_deref()
            .unwrap()
            .contains("preceding archive read failed")
    );
    run_sync(&mut state, &repo, false, LibraryOperation::Reload);
    assert!(state.library_error(false).is_none());
    assert_eq!(state.entries.len(), 1);
    assert!(state.notice.is_none());
}

#[test]
fn history_backlog_is_bounded_and_admitted_entries_survive_shutdown_without_callbacks() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Arc::new(Repository::new(directory.path().to_owned()).unwrap());
    repo.append(false, entry("reviewed")).unwrap();
    let mut state = with_services(repo.clone(), loaded(&repo), |state| {
        state.request_remove_all(false);
        state.confirm_remove_all();
        for index in 0..17 {
            state.record_history_entry(entry(&format!("queued-{index}")));
        }
        assert_eq!(state.library_state.queued_history, 16);
        assert!(state.notice.as_deref().unwrap().contains("still busy"));
    });
    assert_eq!(repo.history().unwrap().len(), 16);
    assert!(
        !repo
            .history()
            .unwrap()
            .iter()
            .any(|record| record.passage.contains("queued-16"))
    );
    finish_drained(&mut state, &repo, false);
    assert_eq!(state.entries.len(), 16);
    assert_eq!(state.library_state.queued_history, 0);
    assert!(!state.library_busy(false));
}

#[test]
fn successful_retry_preserves_an_unrelated_newer_notice() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::new(directory.path().to_owned()).unwrap();
    repo.append(true, entry("retained")).unwrap();
    let mut state = loaded(&repo);
    state.library_errors[1] = Some("Old archive read error".into());
    state.append_saved_entry(entry("not-saved"));
    assert!(state.notice.as_deref().unwrap().contains("not saved"));
    state.notice = Some("Unrelated current diagnostic result".into());
    run_sync(&mut state, &repo, true, LibraryOperation::Reload);
    assert!(state.library_error(true).is_none());
    assert_eq!(
        state.notice.as_deref(),
        Some("Unrelated current diagnostic result")
    );
}

#[test]
fn stale_success_and_failure_callbacks_cannot_replace_newer_rows_or_read_errors() {
    for bookmark in [false, true] {
        let mut state = AppState::default();
        let earlier = state.begin_library_operation(bookmark);
        let old_id = earlier.id;
        drop(earlier);
        let current = state.begin_library_operation(bookmark);
        let current_id = current.id;
        drop(current);
        state.finish_library_operation(
            bookmark,
            current_id,
            LibraryOutcome {
                entries: Err("Current read failed".into()),
                operation_error: None,
            },
            true,
            None,
        );
        state.finish_library_operation(
            bookmark,
            old_id,
            LibraryOutcome {
                entries: Ok(vec![entry("stale")]),
                operation_error: Some("Stale mutation failed".into()),
            },
            true,
            Some("Stale success"),
        );
        assert_eq!(state.library_error(bookmark), Some("Current read failed"));
        assert!(state.library_entries(bookmark).is_empty());
        assert!(state.notice.is_none());
        assert!(!state.library_busy(bookmark));
    }
}

#[test]
fn no_services_headless_controls_cannot_write_or_launch_and_busy_save_is_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::new(directory.path().to_owned()).unwrap();
    repo.append(true, entry("retained")).unwrap();
    let mut state = loaded(&repo);
    let original = std::fs::read(archive(directory.path(), true)).unwrap();
    state.request_remove_all(true);
    state.confirm_remove_all();
    state.reload_library(true);
    state.reveal_library(true);
    state.remove_entry(true, state.bookmarks[0].id.clone());
    assert!(!state.library_busy(true));
    assert_eq!(
        std::fs::read(archive(directory.path(), true)).unwrap(),
        original
    );
    let reservation = state.begin_library_operation(true);
    state.append_saved_entry(entry("busy"));
    assert!(state.notice.as_deref().unwrap().contains("busy"));
    assert_eq!(state.library_state.operations[1], Some(reservation.id));
    drop(reservation);
    state.library_errors[0] = Some("History unreadable".into());
    state.record_history_entry(entry("not-recorded"));
    assert!(state.notice.as_deref().unwrap().contains("not recorded"));
    assert!(state.library_state.operations[0].is_none());
    // Even an explicitly provided test service cannot launch a file manager
    // while native UI is disabled. Archive operations need no native app.
    let repo = Arc::new(repo);
    let _state = with_services(repo.clone(), loaded(&repo), |state| {
        assert!(!state.native_enabled);
        state.reveal_library(true);
        assert!(!state.library_busy(true));
        assert!(state.notice.is_none());
    });
}

#[test]
fn dropping_a_predecessor_during_unwind_releases_waiters_with_an_unreadable_guard() {
    let mut state = AppState::default();
    let first = state.begin_library_operation(false);
    let second = state.begin_library_operation(false);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _first = first;
        panic!("Controlled library worker panic");
    }));
    assert!(caught.is_err());
    let previous = second.wait();
    assert!(
        previous
            .read
            .as_deref()
            .unwrap()
            .contains("did not complete")
    );
    drop(second);
}
