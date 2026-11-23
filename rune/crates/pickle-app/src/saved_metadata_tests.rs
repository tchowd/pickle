//! Historical request/reference metadata stays literal across repository reopen.
use super::*;
use crate::storage::{Entry, EntryReference, Repository};
use rune::{Env, Runtime};
use serde_json::{Value, json};

fn entry(id: &str, kind: ChartKind) -> Entry {
    let source = match kind {
        ChartKind::Bar => "Revenue for **A** was $10 million and $15 million.",
        ChartKind::Flow => "The request is received, validated, and saved.",
    };
    let spec = match kind {
        ChartKind::Bar => {
            json!({"type":"bar_chart","bars":pickle_core::numeric_evidence(source).iter().map(|value|json!({"evidenceID":value.id})).collect::<Vec<_>>()})
        }
        ChartKind::Flow => json!({"type":"flow_diagram",
            "nodes":[{"id":"a","label":"received"},{"id":"b","label":"validated"},{"id":"c","label":"saved"}],
            "edges":[{"from":"a","to":"b","evidence":source,"condition":""},{"from":"b","to":"c","evidence":source,"condition":""}]}),
    };
    let chart = pickle_core::validate_chart(&spec.to_string(), kind, source).unwrap();
    Entry {
        id: id.into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        app_name: "Fixed metadata fixture".into(),
        app_id: "fixture".into(),
        action: "Visualize".into(),
        question: Some("What does **this** mean? <literal request>".into()),
        passage: source.into(),
        answer: chart.alternative(),
        quality: pickle_core::QualityStatus::ChartValidated.visible_label(),
        chart: Some(kind.as_str().into()),
        reference: Some(EntryReference {
            kind: "article".into(),
            title: "Reference **title**, not formatting".into(),
            url: "https://example.invalid/context?x=1&y=2".into(),
        }),
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
fn keys(tree: &Value, out: &mut Vec<String>) {
    if let Some(key) = tree["key"].as_str() {
        out.push(key.into());
    }
    if let Some(children) = tree["children"].as_array() {
        for child in children {
            keys(child, out);
        }
    }
}
fn text_labels(tree: &Value, out: &mut Vec<String>) {
    if tree["role"] == "label"
        && let Some(text) = tree["name"].as_str()
    {
        out.push(text.into());
    }
    if let Some(children) = tree["children"].as_array() {
        for child in children {
            text_labels(child, out);
        }
    }
}
fn plain_field(tree: &Value, key: &str, value: &str) {
    let found = node(tree, key).expect("Stored metadata must be accessible");
    assert_eq!(found["name"], value);
    assert_eq!(found["role"], "label");
    assert!(found.get("url").is_none());
    assert!(
        !found["actions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| action == "click"))
    );
}
fn assert_metadata(tree: &Value, entry: &Entry) {
    plain_field(
        tree,
        &format!("entry-question-{}", entry.id),
        entry.question.as_ref().unwrap(),
    );
    let reference = entry.reference.as_ref().unwrap();
    plain_field(
        tree,
        &format!("entry-reference-kind-{}", entry.id),
        &reference.kind,
    );
    plain_field(
        tree,
        &format!("entry-reference-title-{}", entry.id),
        &reference.title,
    );
    plain_field(
        tree,
        &format!("entry-reference-url-{}", entry.id),
        &reference.url,
    );
    let kind = match entry.chart.as_deref().unwrap() {
        "bar_chart" => "Bar chart · saved text alternative",
        "flow_diagram" => "Flow diagram · saved text alternative",
        _ => panic!("Fixture must have a recognized chart kind"),
    };
    plain_field(tree, &format!("entry-chart-kind-{}", entry.id), kind);
    let mut order = Vec::new();
    keys(tree, &mut order);
    assert!(
        order
            .iter()
            .position(|key| key == &format!("entry-quality-{}", entry.id))
            .unwrap()
            < order.iter().position(|key| key == "answer").unwrap(),
        "Historical quality must precede its answer"
    );
    assert!(
        node(tree, "bar-chart").is_none() && node(tree, "flow-chart").is_none(),
        "Saved records must not reconstruct graphics"
    );
}
fn state(entries: Vec<Entry>, bookmark: bool, detail: Option<String>) -> AppState {
    AppState {
        route: Route::Library(bookmark),
        library_selected: detail,
        entries: if bookmark { vec![] } else { entries.clone() }.into(),
        bookmarks: if bookmark { entries } else { vec![] }.into(),
        ..AppState::default()
    }
}
fn click(runtime: &mut Runtime<AppState>, key: &str) {
    let (x, y) = runtime.find(key).unwrap();
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    settle(runtime);
}

#[test]
fn metadata_and_chart_alternatives_survive_reopen_in_both_stores_and_views() {
    let expected = vec![entry("bar", ChartKind::Bar), entry("flow", ChartKind::Flow)];
    for bookmark in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        {
            let repo = Repository::new(directory.path().to_owned()).unwrap();
            for entry in &expected {
                repo.append(bookmark, entry.clone()).unwrap();
            }
        }
        let reopened = Repository::new(directory.path().to_owned()).unwrap();
        let loaded = if bookmark {
            reopened.bookmarks()
        } else {
            reopened.history()
        }
        .unwrap();
        for entry in &expected {
            assert_eq!(
                loaded.iter().find(|value| value.id == entry.id).unwrap(),
                entry
            );
        }
        let archive = directory.path().join(if bookmark {
            "bookmarks.json"
        } else {
            "history.json"
        });
        let before = std::fs::read(&archive).unwrap();
        let mut runtime = Runtime::new(
            state(loaded.clone(), bookmark, None),
            view,
            (480.0, 1800.0),
            Env::new(true, 1.0),
        );
        let clipboard = rune::clipboard::memory();
        runtime.use_clipboard(&clipboard);
        settle(&mut runtime);
        assert_eq!(
            node(&runtime.accessibility_json(), "search-library").unwrap()["name"],
            "Search passages, answers, questions and sources"
        );
        for entry in &expected {
            let tree = runtime.accessibility_json();
            let card = node(&tree, &format!("entry-{}", entry.id)).unwrap();
            assert_metadata(card, entry);
            plain_field(
                card,
                "answer",
                &entry.answer.chars().take(500).collect::<String>(),
            );
            click(&mut runtime, &format!("copy-{}", entry.id));
            assert_eq!(clipboard.text().unwrap().as_str(), entry.answer);
            click(&mut runtime, &format!("read-{}", entry.id));
            assert_metadata(&runtime.accessibility_json(), entry);
            plain_field(&runtime.accessibility_json(), "answer", &entry.answer);
            click(&mut runtime, "copy-saved-detail");
            assert_eq!(clipboard.text().unwrap().as_str(), entry.answer);
            click(&mut runtime, &format!("entry-reference-url-{}", entry.id));
            assert!(
                runtime.take_url_requests().is_empty(),
                "A literal reference click must not open a URL"
            );
            click(&mut runtime, "entry-back");
        }
        assert_eq!(std::fs::read(archive).unwrap(), before);
        assert_eq!(
            if bookmark {
                &runtime.state.bookmarks
            } else {
                &runtime.state.entries
            },
            &loaded
        );
        assert!(
            runtime.state.services.is_none()
                && runtime.state.snapshot.is_none()
                && runtime.state.result.is_none()
        );
    }
}

#[test]
fn prose_and_unknown_chart_kinds_keep_markdown_display_and_plain_copy() {
    for bookmark in [false, true] {
        for detail in [false, true] {
            for chart in [None, Some("future_chart_kind")] {
                let mut record = entry("prose", ChartKind::Bar);
                record.chart = chart.map(str::to_owned);
                record.question = None;
                record.reference = None;
                record.quality = None;
                record.answer = "A **formatted** answer.\nSecond line.".into();
                let mut runtime = Runtime::new(
                    state(
                        vec![record.clone()],
                        bookmark,
                        detail.then(|| record.id.clone()),
                    ),
                    view,
                    (480.0, 900.0),
                    Env::new(true, 1.0),
                );
                let clipboard = rune::clipboard::memory();
                runtime.use_clipboard(&clipboard);
                settle(&mut runtime);
                let tree = runtime.accessibility_json();
                let answer = node(&tree, "answer").unwrap();
                let mut displayed = Vec::new();
                text_labels(answer, &mut displayed);
                assert_eq!(displayed, vec!["A formatted answer. Second line."]);
                click(
                    &mut runtime,
                    if detail {
                        "copy-saved-detail"
                    } else {
                        "copy-prose"
                    },
                );
                assert_eq!(
                    clipboard.text().unwrap().as_str(),
                    "A formatted answer.\nSecond line."
                );
                assert_eq!(
                    if bookmark {
                        &runtime.state.bookmarks[0]
                    } else {
                        &runtime.state.entries[0]
                    },
                    &record
                );
            }
        }
    }
}

#[test]
fn absent_or_unknown_metadata_never_invents_chart_validation_or_a_graph() {
    for bookmark in [false, true] {
        for detail in [false, true] {
            for chart in [None, Some("future_chart_kind")] {
                let mut record = entry("absent", ChartKind::Bar);
                record.question = None;
                record.reference = None;
                record.quality = None;
                record.chart = chart.map(str::to_owned);
                record.answer = r#"{"type":"flow_diagram","nodes":[],"edges":[]}"#.into();
                let before = record.clone();
                let mut runtime = Runtime::new(
                    state(vec![record], bookmark, detail.then(|| "absent".into())),
                    view,
                    (480.0, 900.0),
                    Env::new(true, 1.0),
                );
                settle(&mut runtime);
                let tree = runtime.accessibility_json();
                for key in [
                    "entry-request-absent",
                    "entry-reference-absent",
                    "entry-chart-kind-absent",
                    "entry-quality-absent",
                    "bar-chart",
                    "flow-chart",
                ] {
                    assert!(
                        node(&tree, key).is_none(),
                        "Invented metadata or reconstructed graph: {key}"
                    );
                }
                assert_eq!(
                    if bookmark {
                        &runtime.state.bookmarks[0]
                    } else {
                        &runtime.state.entries[0]
                    },
                    &before
                );
                assert!(runtime.state.services.is_none());
            }
        }
    }
}

#[test]
fn long_unicode_request_and_literal_reference_wrap_at_480_pixels() {
    let mut record = entry("long", ChartKind::Bar);
    record.question = Some(format!("{} **literal** <request>", "どの条件？".repeat(50)));
    record.reference.as_mut().unwrap().title =
        format!("{} **literal** <title>", "出典名".repeat(80));
    record.reference.as_mut().unwrap().url =
        format!("https://example.invalid/context?value={}", "x".repeat(350));
    for bookmark in [false, true] {
        for detail in [false, true] {
            let mut runtime = Runtime::new(
                state(
                    vec![record.clone()],
                    bookmark,
                    detail.then(|| record.id.clone()),
                ),
                view,
                (480.0, 1800.0),
                Env::new(true, 1.0),
            );
            settle(&mut runtime);
            assert_metadata(&runtime.accessibility_json(), &record);
            for key in [
                "entry-question-long",
                "entry-reference-title-long",
                "entry-reference-url-long",
            ] {
                let (rect, _) = runtime.presented(key).unwrap();
                assert!(
                    rect[0] >= 0.0 && rect[0] + rect[2] <= 480.0,
                    "Metadata escaped the viewport: {key} {rect:?}"
                );
                assert!(rect[3] > 18.0, "Long metadata must wrap: {key} {rect:?}");
            }
            assert!(runtime.take_url_requests().is_empty());
            assert!(runtime.state.services.is_none());
        }
    }
}
