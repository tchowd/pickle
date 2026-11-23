//! Library confirmation and recovery exercise real controls and temporary archives.
use super::*;
use crate::{
    controller::{
        Services,
        library_controller::{LibraryOutcome, read_library},
    },
    storage::{Entry, Repository},
};
use rune::{Env, Runtime};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

fn entry(id: &str) -> Entry {
    Entry {
        id: id.into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        app_name: "Library fixture".into(),
        app_id: "fixture".into(),
        action: "Simplify".into(),
        question: None,
        passage: format!("Controlled passage {id}."),
        answer: format!("Controlled answer {id}."),
        quality: None,
        chart: None,
        reference: None,
    }
}
fn state(bookmark: bool, records: Vec<Entry>) -> AppState {
    AppState {
        route: Route::Library(bookmark),
        entries: if bookmark { vec![] } else { records.clone() }.into(),
        bookmarks: if bookmark { records } else { vec![] }.into(),
        ..AppState::default()
    }
}
fn runtime(state: AppState) -> Runtime<AppState> {
    let mut rt = Runtime::new(state, view, (480.0, 1800.0), Env::new(true, 1.0));
    settle(&mut rt);
    rt
}
fn settle(rt: &mut Runtime<AppState>) {
    for _ in 0..90 {
        rt.frame(1.0 / 60.0);
    }
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("Missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}
fn node<'a>(tree: &'a Value, key: &str) -> Option<&'a Value> {
    if tree["key"] == key {
        return Some(tree);
    }
    tree["children"]
        .as_array()?
        .iter()
        .find_map(|child| node(child, key))
}
fn all_text(tree: &Value) -> String {
    let mut result = tree["name"].as_str().unwrap_or_default().to_owned();
    if let Some(children) = tree["children"].as_array() {
        for child in children {
            result.push_str(&all_text(child));
        }
    }
    result
}
fn assert_disabled(rt: &Runtime<AppState>, key: &str) {
    let tree = rt.accessibility_json();
    let states = node(&tree, key).unwrap()["states"].as_array().unwrap();
    assert!(
        states.iter().any(|value| value == "disabled"),
        "{key}: {states:?}"
    );
}

/// A real service executor and real temporary repository, without App::run,
/// native shell setup, archives outside the fixture, clipboard or provider IO.
/// Unit Runtime does not drain AppHandle's private host queue; tests feed its
/// production completion reducer after the executor has demonstrably drained.
fn with_services(
    rt: &mut Runtime<AppState>,
    repo: Arc<Repository>,
    work: impl FnOnce(&mut Runtime<AppState>),
) {
    let app = rune::App::new(rune::AppSpec::new(
        "Library fixture",
        (480.0, 900.0),
        AppState::default,
        |_, _| rune::el(),
    ));
    let (services, owner) = Services::new(Some(repo), &rt.state.settings).unwrap();
    rt.update(|s, _| {
        s.handle = Some(app.handle());
        s.services = Some(Arc::new(services));
    });
    // Drain even if an assertion fails: production intentionally terminates on
    // an unwinding RuntimeOwner to preserve its application lease invariant.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(rt)));
    let report = owner.shutdown(Duration::from_secs(5));
    drop(app);
    assert!(report.finished && report.unfinished_blocking == 0);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
fn finish_reload(rt: &mut Runtime<AppState>, repo: &Repository, bookmark: bool) {
    let id = rt.state.library_state.operations[usize::from(bookmark)].unwrap();
    let outcome = LibraryOutcome {
        entries: read_library(repo, bookmark),
        operation_error: None,
    };
    rt.update(move |s, _| s.finish_library_operation(bookmark, id, outcome, true, None));
    settle(rt);
}

#[test]
fn confirmation_uses_total_store_count_and_cancel_never_writes_either_archive() {
    for bookmark in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().to_owned()).unwrap();
        let records: Vec<_> = (0..25).map(|i| entry(&format!("record-{i}"))).collect();
        for record in &records {
            repo.append(bookmark, record.clone()).unwrap();
        }
        repo.append(!bookmark, entry("other-store")).unwrap();
        let before = [
            std::fs::read(dir.path().join("history.json")).unwrap(),
            std::fs::read(dir.path().join("bookmarks.json")).unwrap(),
        ];
        let mut rt = runtime(state(bookmark, records.clone()));
        rt.update(|s, _| {
            s.search = TextEdit::new("record-24");
            s.library_page = 5;
            s.complete_library_search_fixture(bookmark);
        });
        settle(&mut rt);
        click(&mut rt, "library-delete-all");
        assert_eq!(rt.state.library_delete_confirmation, Some(bookmark));
        let tree = rt.accessibility_json();
        let summary = node(&tree, "library-delete-summary").unwrap()["name"]
            .as_str()
            .unwrap();
        assert_eq!(
            summary,
            format!(
                "Delete all 25 answers in {}?",
                if bookmark {
                    "Saved answers"
                } else {
                    "Knowledge base"
                }
            )
        );
        assert!(
            all_text(node(&tree, "library-delete-confirmation").unwrap())
                .contains("cannot be undone")
        );
        assert_disabled(&rt, "library-delete-all");
        assert_disabled(&rt, "remove-record-24");
        for key in [
            "library-delete-all",
            "library-confirm-delete-all",
            "library-cancel-delete-all",
        ] {
            let (rect, _) = rt.presented(key).unwrap();
            assert!(
                rect[0] >= 0.0 && rect[0] + rect[2] <= 480.0,
                "{key}: {rect:?}"
            );
        }
        click(&mut rt, "remove-record-24");
        assert_eq!(
            if bookmark {
                &rt.state.bookmarks
            } else {
                &rt.state.entries
            },
            &records
        );
        assert_eq!(rt.state.library_delete_confirmation, Some(bookmark));
        click(&mut rt, "library-cancel-delete-all");
        assert!(rt.state.library_delete_confirmation.is_none());
        assert_eq!(
            std::fs::read(dir.path().join("history.json")).unwrap(),
            before[0]
        );
        assert_eq!(
            std::fs::read(dir.path().join("bookmarks.json")).unwrap(),
            before[1]
        );
        assert!(rt.state.services.is_none());
    }
}

#[test]
fn tab_header_and_back_navigation_clear_confirmation_selection_and_page() {
    for (key, route) in [
        ("library-history", Route::Library(false)),
        ("library-bookmarks", Route::Library(true)),
        ("library-back", Route::Reader),
        ("open-settings", Route::Settings),
        ("open-saved", Route::Library(true)),
    ] {
        let mut rt = runtime(state(true, vec![entry("one")]));
        click(&mut rt, "library-delete-all");
        rt.update(|s, _| {
            s.library_selected = Some("one".into());
            s.library_page = 8;
        });
        settle(&mut rt);
        click(&mut rt, key);
        assert_eq!(rt.state.route, route, "{key}");
        assert!(rt.state.library_delete_confirmation.is_none(), "{key}");
        assert!(rt.state.library_selected.is_none(), "{key}");
        assert_eq!(rt.state.library_page, 0, "{key}");
    }
}

#[test]
fn confirmed_buttons_delete_only_the_reviewed_temporary_store() {
    for bookmark in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = Arc::new(Repository::new(dir.path().to_owned()).unwrap());
        let records = vec![entry("first"), entry("second")];
        for record in &records {
            repo.append(bookmark, record.clone()).unwrap();
        }
        let other = entry("untouched");
        repo.append(!bookmark, other.clone()).unwrap();
        let mut rt = runtime(state(bookmark, read_library(&repo, bookmark).unwrap()));
        with_services(&mut rt, repo.clone(), |rt| {
            click(rt, "library-delete-all");
            assert_eq!(
                read_library(&repo, bookmark).unwrap().len(),
                2,
                "Opening must not delete"
            );
            click(rt, "library-cancel-delete-all");
            assert_eq!(
                read_library(&repo, bookmark).unwrap().len(),
                2,
                "Cancel must not delete"
            );
            click(rt, "library-delete-all");
            click(rt, "library-confirm-delete-all");
            assert!(rt.state.library_busy(bookmark));
            assert!(rt.state.library_delete_confirmation.is_none());
            assert!(rt.find("library-busy").is_some());
        });
        let reopened = Repository::new(dir.path().to_owned()).unwrap();
        assert!(read_library(&reopened, bookmark).unwrap().is_empty());
        assert_eq!(read_library(&reopened, !bookmark).unwrap(), vec![other]);
        let operation = rt.state.library_state.operations[usize::from(bookmark)].unwrap();
        let outcome = LibraryOutcome {
            entries: read_library(&reopened, bookmark),
            operation_error: None,
        };
        rt.update(move |s, _| {
            s.finish_library_operation(bookmark, operation, outcome, false, None)
        });
        settle(&mut rt);
        assert!(rt.find("library-busy").is_none());
        assert!(rt.find("library-delete-confirmation").is_none());
    }
}

#[test]
fn empty_unreadable_and_busy_stores_block_destructive_pointer_actions() {
    for bookmark in [false, true] {
        for condition in ["empty", "error", "busy"] {
            let mut s = state(
                bookmark,
                if condition == "empty" {
                    vec![]
                } else {
                    vec![entry("one")]
                },
            );
            if condition == "error" {
                s.library_errors[usize::from(bookmark)] = Some("Malformed archive".into());
            }
            if condition == "busy" {
                s.library_state.operations[usize::from(bookmark)] = Some(uuid::Uuid::new_v4());
            }
            let mut rt = runtime(s);
            let before = if bookmark {
                rt.state.bookmarks.clone()
            } else {
                rt.state.entries.clone()
            };
            assert_disabled(&rt, "library-delete-all");
            if !before.is_empty() {
                assert_disabled(&rt, "remove-one");
            }
            click(&mut rt, "library-delete-all");
            assert!(rt.state.library_delete_confirmation.is_none());
            if !before.is_empty() {
                click(&mut rt, "remove-one");
            }
            assert_eq!(
                if bookmark {
                    &rt.state.bookmarks
                } else {
                    &rt.state.entries
                },
                &before
            );
            assert!(rt.state.services.is_none());
            if condition == "busy" {
                assert!(rt.find("library-busy").is_some());
            }
        }
    }
}

#[test]
fn unreadable_archive_survives_retry_then_explicit_repair_restores_results() {
    for bookmark in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let repo = Arc::new(Repository::new(dir.path().to_owned()).unwrap());
        let path = dir.path().join(if bookmark {
            "bookmarks.json"
        } else {
            "history.json"
        });
        let broken = b"{ damaged fixture";
        std::fs::write(&path, broken).unwrap();
        let mut s = state(bookmark, vec![]);
        s.library_errors[usize::from(bookmark)] = Some(read_library(&repo, bookmark).unwrap_err());
        let mut rt = runtime(s);
        assert!(rt.find("library-error").is_some());
        let text = all_text(&rt.accessibility_json());
        assert!(!text.contains("Choose Save beside an answer"));
        assert!(!text.contains("Enable the knowledge base in Settings"));
        assert!(text.contains("Keep a backup, then repair or move"));
        with_services(&mut rt, repo.clone(), |rt| click(rt, "library-retry"));
        assert_eq!(std::fs::read(&path).unwrap(), broken);
        finish_reload(&mut rt, &repo, bookmark);
        assert!(rt.state.library_error(bookmark).is_some());
        // Simulate the user's documented explicit recovery, not an app reset.
        std::fs::rename(&path, dir.path().join("broken-backup.json")).unwrap();
        let repaired = entry("repaired");
        repo.append(bookmark, repaired.clone()).unwrap();
        with_services(&mut rt, repo.clone(), |rt| click(rt, "library-retry"));
        finish_reload(&mut rt, &repo, bookmark);
        assert!(rt.state.library_error(bookmark).is_none());
        assert!(rt.find("library-error").is_none());
        assert!(rt.find("read-repaired").is_some());
        assert_eq!(read_library(&repo, bookmark).unwrap(), vec![repaired]);
        assert_eq!(
            std::fs::read(dir.path().join("broken-backup.json")).unwrap(),
            broken
        );
    }
}

#[test]
fn long_literal_archive_error_wraps_and_headless_reveal_is_disabled() {
    let error = format!(
        "**Archive diagnostic** <a href='https://example.invalid/'> {}",
        "壊れた資料".repeat(90)
    );
    for bookmark in [false, true] {
        let mut s = state(bookmark, vec![]);
        s.library_errors[usize::from(bookmark)] = Some(error.clone());
        let mut rt = runtime(s);
        let tree = rt.accessibility_json();
        assert_eq!(node(&tree, "library-error-message").unwrap()["name"], error);
        let (rect, _) = rt.presented("library-error-message").unwrap();
        assert!(rect[0] >= 0.0 && rect[0] + rect[2] <= 480.0, "{rect:?}");
        assert!(rect[3] > 35.0);
        assert_disabled(&rt, "library-show-archive");
        click(&mut rt, "library-show-archive");
        assert!(rt.state.services.is_none() && !rt.state.native_enabled);
        assert!(rt.state.notice.is_none());
        assert!(!rt.state.library_busy(bookmark));
    }
}
