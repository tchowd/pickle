//! Saved labels survive real archives and render as historical plain text.
use super::*;
use crate::storage::{Entry, Repository};
use pickle_core::QualityStatus;
use rune::{Env, Runtime};
use serde_json::Value;

fn entry(id: &str, quality: Option<String>) -> Entry {
    Entry {
        id: id.into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        app_name: "Fixed quality fixture".into(),
        app_id: "fixture".into(),
        action: "Simplify".into(),
        question: None,
        passage: format!("The controlled passage for {id}."),
        answer: format!("A **fixed answer** for {id}."),
        quality,
        chart: None,
        reference: None,
    }
}
fn settle(runtime: &mut Runtime<AppState>) {
    for _ in 0..90 {
        runtime.frame(1.0 / 60.0);
    }
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
fn click(runtime: &mut Runtime<AppState>, key: &str) {
    let (x, y) = runtime.find(key).unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    settle(runtime);
}
fn assert_quality(tree: &Value, entry: &Entry) {
    let found = node(tree, &format!("entry-quality-{}", entry.id));
    match &entry.quality {
        Some(label) => {
            let found = found.expect("Stored quality label must remain accessible");
            assert_eq!(found["name"], *label);
            assert_eq!(found["role"], "label");
        }
        None => assert!(
            found.is_none(),
            "Do not invent a label for an unchecked answer"
        ),
    }
}

#[test]
fn repository_reopen_retains_quality_labels_in_cards_and_detail_for_both_stores() {
    let records = vec![
        entry(
            "concerns",
            QualityStatus::Concerns(vec!["changed_quantities".into()]).visible_label(),
        ),
        entry("uncertain", QualityStatus::Uncertain.visible_label()),
        entry(
            "failed-check",
            QualityStatus::NotChecked("Jev check or repair failed".into()).visible_label(),
        ),
        entry(
            "checks-disabled",
            QualityStatus::NotChecked(QualityStatus::CHECKS_OFF.into()).visible_label(),
        ),
        entry(
            "offline",
            QualityStatus::NotChecked(QualityStatus::OFFLINE.into()).visible_label(),
        ),
    ];
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        {
            let repo = Repository::new(directory.path().to_owned()).unwrap();
            for record in &records {
                repo.append(bookmark, record.clone()).unwrap();
            }
        }
        // A different Repository and entirely fresh app state model restart;
        // nothing can be inherited from a live result or previous UI notice.
        let reopened = Repository::new(directory.path().to_owned()).unwrap();
        let loaded = if bookmark {
            reopened.bookmarks()
        } else {
            reopened.history()
        }
        .unwrap();
        for expected in &records {
            assert_eq!(
                loaded.iter().find(|entry| entry.id == expected.id).unwrap(),
                expected
            );
        }
        let archive = directory.path().join(if bookmark {
            "bookmarks.json"
        } else {
            "history.json"
        });
        let bytes = std::fs::read(&archive).unwrap();
        let state = AppState {
            route: Route::Library(bookmark),
            entries: if bookmark { vec![] } else { loaded.clone() }.into(),
            bookmarks: if bookmark { loaded.clone() } else { vec![] }.into(),
            ..AppState::default()
        };
        let mut runtime = Runtime::new(state, view, (480.0, 1800.0), Env::new(true, 1.0));
        settle(&mut runtime);
        for expected in &records {
            let tree = runtime.accessibility_json();
            let card = node(&tree, &format!("entry-{}", expected.id)).unwrap();
            assert_quality(card, expected);
            click(&mut runtime, &format!("read-{}", expected.id));
            assert_eq!(
                runtime.state.library_selected.as_deref(),
                Some(expected.id.as_str())
            );
            assert_quality(&runtime.accessibility_json(), expected);
            assert!(runtime.find("copy-saved-detail").is_some());
            click(&mut runtime, "entry-back");
            assert!(runtime.state.library_selected.is_none());
        }
        assert_eq!(
            std::fs::read(archive).unwrap(),
            bytes,
            "Viewing must not rewrite quality history"
        );
        assert!(runtime.state.services.is_none());
        assert!(runtime.state.snapshot.is_none() && runtime.state.result.is_none());
        assert_eq!(
            if bookmark {
                &runtime.state.bookmarks
            } else {
                &runtime.state.entries
            },
            &loaded
        );
    }
}

#[test]
fn long_stored_quality_is_plain_text_and_fits_the_minimum_width_in_both_views() {
    let label = format!(
        "Historical check text: **{}** <img src=\"https://example.invalid/x\">",
        "unverified".repeat(50)
    );
    for bookmark in [false, true] {
        for detail in [false, true] {
            let record = entry("long-label", Some(label.clone()));
            let state = AppState {
                route: Route::Library(bookmark),
                entries: (if bookmark {
                    vec![]
                } else {
                    vec![record.clone()]
                })
                .into(),
                bookmarks: (if bookmark {
                    vec![record.clone()]
                } else {
                    vec![]
                })
                .into(),
                library_selected: detail.then(|| record.id.clone()),
                ..AppState::default()
            };
            let mut runtime = Runtime::new(state, view, (480.0, 900.0), Env::new(true, 1.0));
            settle(&mut runtime);
            assert_quality(&runtime.accessibility_json(), &record);
            let (rect, _) = runtime.presented("entry-quality-long-label").unwrap();
            assert!(
                rect[0] >= 0.0 && rect[0] + rect[2] <= 480.0,
                "Quality text escaped the viewport: {rect:?}"
            );
            assert!(
                rect[3] > 18.0,
                "Long label must wrap instead of clipping to one line"
            );
            assert!(runtime.state.services.is_none());
        }
    }
}
