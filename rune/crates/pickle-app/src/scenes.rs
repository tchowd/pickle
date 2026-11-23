//! Reproducible UI fixtures only: no services, native shell, vault, archives,
//! provider tasks, permission prompts, or browser connections are constructed.
use crate::controller::{Activity, AppState, Panel, Pending, Route, SAMPLE, SAMPLE_ANSWER};
use pickle_core::{
    ChartKind, QualityStatus, ReadingAction, ReadingResult, SelectionSnapshot, ValidatedChart,
    numeric_evidence, validate_chart,
};
use rune::TextEdit;
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub type Scene = (&'static str, fn(&mut AppState));
pub const SCENES: &[Scene] = &[
    ("sample", sample),
    ("choosing", choosing),
    ("settings", settings),
    ("history", empty_history),
    ("error", connection_error),
    ("generating", generating),
    ("streaming", streaming),
    ("reviewing", reviewing),
    ("repairing", repairing),
    ("chart-bar", bar_chart),
    ("chart-flow", flow_chart),
    ("consent", local_consent),
    ("consent-local", local_consent),
    ("consent-online", online_consent),
    ("permission-denied", permission_denied),
    ("history-empty", empty_history),
    ("history-damaged", damaged_history),
];

/// Normalize common flag spellings before Rune parses value pairs. Classify
/// fixture startup before opening repositories or acquiring the native lease.
pub fn normalize_arguments(arguments: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::with_capacity(arguments.len());
    for argument in arguments {
        if let Some((flag, value)) = argument.split_once('=')
            && fixture_flag(flag)
        {
            normalized.push(flag.to_owned());
            normalized.push(value.to_owned());
        } else {
            normalized.push(argument);
        }
    }
    normalized
}
pub fn isolated_startup(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        fixture_flag(
            argument
                .split_once('=')
                .map_or(argument.as_str(), |(flag, _)| flag),
        )
    })
}
fn fixture_flag(argument: &str) -> bool {
    matches!(
        argument,
        "--scene" | "--snap" | "--tree" | "--script" | "--video"
    )
}

fn fixture(state: &mut AppState, passage: &str) {
    *state = AppState::default();
    state.demo = true;
    state.panel = Panel::Expanded;
    state.source = TextEdit::new(passage);
    let mut snapshot = SelectionSnapshot::new(passage, "Fixed UI fixture", "fixture");
    snapshot.id = uuid::Uuid::from_u128(1);
    snapshot.captured_at = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .expect("fixed scene timestamp")
        .with_timezone(&chrono::Utc);
    snapshot.method = "Fixed fixture".into();
    state.snapshot = Some(snapshot);
    state.revision = 1;
    state.context_state.id = uuid::Uuid::from_u128(3);
    state.update_attempt = uuid::Uuid::from_u128(4);
    state.notice =
        Some("Fixed UI fixture. No requests are running and no user data is loaded.".into());
}

fn completed(state: &mut AppState, text: &str, chart: Option<ValidatedChart>) {
    let action = if chart.is_some() {
        ReadingAction::Chart
    } else {
        ReadingAction::Simplify
    };
    state.result = Some(ReadingResult {
        id: uuid::Uuid::from_u128(2),
        action,
        text: text.into(),
        quality: if chart.is_some() {
            QualityStatus::ChartValidated
        } else {
            QualityStatus::NotChecked(QualityStatus::OFFLINE.into())
        },
        chart,
        model: "Fixed fixture".into(),
        evaluator_model: None,
        elapsed: 0.0,
        repairs: 0,
        notes: vec![],
        timing: None,
    });
}

fn sample(state: &mut AppState) {
    fixture(state, SAMPLE);
    completed(state, SAMPLE_ANSWER, None);
}
fn choosing(state: &mut AppState) {
    fixture(state, SAMPLE);
    state.panel = Panel::Choosing;
}
fn settings(state: &mut AppState) {
    fixture(state, SAMPLE);
    state.route = Route::Settings;
}
fn connection_error(state: &mut AppState) {
    fixture(state, SAMPLE);
    state.activity = Activity::Failed(
        "Can’t reach the Local server. Check that it’s awake and connected, then retry.".into(),
    );
    state.last = Some(Pending::action(ReadingAction::Simplify));
}
fn running(state: &mut AppState, progress: &str, draft: &str) {
    fixture(state, SAMPLE);
    state.activity = Activity::Running {
        id: 1,
        cancel: CancellationToken::new(),
        progress: progress.into(),
    };
    state.draft = draft.into();
}
fn generating(state: &mut AppState) {
    running(state, "Simplifying…", "");
}
fn streaming(state: &mut AppState) {
    running(
        state,
        "Simplifying…",
        "The treatment **might help some people**, but the evidence is still limited.\n\nLarger studies are needed to",
    );
}
fn reviewing(state: &mut AppState) {
    running(
        state,
        "Reviewing…",
        "The treatment might help some people, but there is not enough evidence yet. Larger studies are needed before applying it more broadly.",
    );
}
fn repairing(state: &mut AppState) {
    running(
        state,
        "Refining…",
        "The treatment might help some people.\n\nThe explanation is being refined to preserve the passage’s limits and uncertainty.",
    );
}

const BAR_SOURCE: &str =
    "The first batch had a 12% error rate. The second batch had a 5% error rate.";
fn bar_chart(state: &mut AppState) {
    fixture(state, BAR_SOURCE);
    let evidence = numeric_evidence(BAR_SOURCE);
    let payload = json!({ "type": "bar_chart", "bars": evidence.iter().map(|value| json!({ "evidenceID": value.id })).collect::<Vec<_>>() });
    let chart = validate_chart(&payload.to_string(), ChartKind::Bar, BAR_SOURCE)
        .expect("bar fixture must pass the real source validator");
    let alternative = chart.alternative();
    completed(state, &alternative, Some(chart));
}

const FLOW_SOURCE: &str = "If validated, it is saved.";
fn flow_chart(state: &mut AppState) {
    fixture(state, FLOW_SOURCE);
    let payload = json!({
        "type": "flow_diagram",
        "nodes": [{ "id": "validated", "label": "validated" }, { "id": "saved", "label": "saved" }],
        "edges": [{ "from": "validated", "to": "saved", "evidence": FLOW_SOURCE, "condition": "If validated" }],
    });
    let chart = validate_chart(&payload.to_string(), ChartKind::Flow, FLOW_SOURCE)
        .expect("flow fixture must pass the real source validator");
    let alternative = chart.alternative();
    completed(state, &alternative, Some(chart));
}

fn local_consent(state: &mut AppState) {
    fixture(state, SAMPLE);
    state.demo = false;
    state.settings.same_device = true;
    state.settings.local_consent.clear();
    state.activity = Activity::Consent(Pending::action(ReadingAction::Simplify));
}
fn online_consent(state: &mut AppState) {
    fixture(state, SAMPLE);
    state.demo = false;
    state.settings.mode = "Online".into();
    state.settings.jev_enabled = true;
    state.settings.cloud_consent = false;
    state.settings.jev_consent = false;
    state.activity = Activity::Consent(Pending::action(ReadingAction::Simplify));
}
fn permission_denied(state: &mut AppState) {
    fixture(state, SAMPLE);
    state.activity = Activity::Failed("Selection access is unavailable or was denied. Paste the passage, or review selection access in system settings before trying again.".into());
    state.last = None;
}
fn empty_history(state: &mut AppState) {
    fixture(state, SAMPLE);
    state.route = Route::Library(false);
    state.settings.history_enabled = false;
}
fn damaged_history(state: &mut AppState) {
    empty_history(state);
    state.notice = Some("Fixture: history.json is damaged. Existing data has not been changed. Move the file aside before trying again.".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};
    use std::collections::BTreeSet;

    fn runtime(scene: fn(&mut AppState)) -> Runtime<AppState> {
        let mut state = AppState::default();
        scene(&mut state);
        let mut runtime = Runtime::new(
            state,
            crate::views::view,
            (480.0, 720.0),
            Env::new(true, 1.0),
        );
        runtime.frame(0.0);
        runtime
    }
    fn semantic_text(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(value) => value.clone(),
            serde_json::Value::Array(values) => values
                .iter()
                .map(semantic_text)
                .collect::<Vec<_>>()
                .join("\n"),
            serde_json::Value::Object(values) => values
                .values()
                .map(semantic_text)
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }

    #[test]
    fn scene_registry_is_unique_and_never_constructs_native_or_user_services() {
        let mut names = BTreeSet::new();
        for (name, scene) in SCENES {
            assert!(names.insert(*name));
            let runtime = runtime(*scene);
            let state = &runtime.state;
            assert!(!state.native_enabled, "{name}");
            assert!(
                state.services.is_none() && state.native_shell.is_none(),
                "{name}"
            );
            assert!(state.window_geometry_worker.is_none());
            assert!(state.entries.is_empty() && state.bookmarks.is_empty());
            assert!(state.credential.text.is_empty(), "{name}");
            assert!(state.credential_operation.is_none());
            let snapshot = state.snapshot.as_ref().unwrap();
            assert_eq!(snapshot.id, uuid::Uuid::from_u128(1));
            assert_eq!(snapshot.app_id, "fixture");
        }
    }

    #[test]
    fn real_validated_chart_fixtures_expose_evidence_at_minimum_width() {
        for (scene, key, expected) in [
            (
                bar_chart as fn(&mut AppState),
                "bar-chart",
                vec!["12 %", "5 %", "Source-backed bar chart"],
            ),
            (
                flow_chart as fn(&mut AppState),
                "flow-chart",
                vec!["validated", "saved", "If validated", FLOW_SOURCE],
            ),
        ] {
            let runtime = runtime(scene);
            let state = &runtime.state;
            let result = state.result.as_ref().unwrap();
            let chart = result.chart.as_ref().unwrap();
            assert_eq!(result.quality, QualityStatus::ChartValidated);
            let text = semantic_text(&runtime.accessibility_json());
            for expected in expected {
                assert!(text.contains(expected), "Missing {expected}: {text}");
            }
            let elements = runtime.elements();
            let chart_layout = elements
                .iter()
                .find(|element| element.key.as_deref() == Some(key))
                .unwrap();
            assert!(
                chart_layout.rect[0] >= 0.0 && chart_layout.rect[0] + chart_layout.rect[2] <= 480.1
            );
            match chart {
                ValidatedChart::Bar(values) => assert_eq!(*values, numeric_evidence(BAR_SOURCE)),
                ValidatedChart::Flow { edges, .. } => assert_eq!(edges[0].evidence, FLOW_SOURCE),
            }
        }
    }

    #[test]
    fn pipeline_scenes_are_unfinished_states_with_honest_status_and_no_completed_result() {
        for (scene, expected, has_draft) in [
            (generating as fn(&mut AppState), "Simplifying…", false),
            (streaming as fn(&mut AppState), "Simplifying…", true),
            (reviewing as fn(&mut AppState), "Reviewing…", true),
            (repairing as fn(&mut AppState), "Refining…", true),
        ] {
            let runtime = runtime(scene);
            assert!(runtime.state.result.is_none());
            assert!(matches!(runtime.state.activity, Activity::Running { .. }));
            assert_eq!(!runtime.state.draft.is_empty(), has_draft);
            assert!(semantic_text(&runtime.accessibility_json()).contains(expected));
            assert!(runtime.find("cancel-request").is_some());
        }
    }

    #[test]
    fn consent_permissions_and_history_scenes_keep_their_recovery_actions() {
        let online = runtime(online_consent);
        let text = semantic_text(&online.accessibility_json());
        assert!(text.contains("Before sharing this passage") && text.contains("TypeSafe Jev"));
        assert!(
            online.find("consent-continue").is_some() && online.find("consent-cancel").is_some()
        );
        let permission = runtime(permission_denied);
        assert!(semantic_text(&permission.accessibility_json()).contains("Paste the passage"));
        assert!(permission.find("error-settings").is_some());
        let empty = runtime(empty_history);
        assert!(semantic_text(&empty.accessibility_json()).contains("Enable the knowledge base"));
        let damaged = runtime(damaged_history);
        assert!(
            semantic_text(&damaged.accessibility_json())
                .contains("Existing data has not been changed")
        );
    }

    #[test]
    fn fixture_flag_spellings_and_scripted_consent_never_gain_live_services() {
        let args = normalize_arguments(vec![
            "--scene=consent-online".into(),
            "--snap=preview.png".into(),
        ]);
        assert_eq!(args, ["--scene", "consent-online", "--snap", "preview.png"]);
        assert!(isolated_startup(&args));
        assert!(isolated_startup(&["--script=[]".into()]));
        assert!(!isolated_startup(&["--sample".into()]));
        let mut runtime = runtime(online_consent);
        runtime.update(|state, tx| state.consent(tx));
        assert!(runtime.state.services.is_none() && !runtime.state.native_enabled);
        assert!(matches!(runtime.state.activity, Activity::Failed(_)));
        assert!(runtime.state.result.is_none());
        runtime.update(|state, tx| {
            state.review_diagnostics();
            state.export_diagnostics(tx);
        });
        assert!(runtime.file_dialog().is_none());
        assert!(runtime.state.entries.is_empty() && runtime.state.bookmarks.is_empty());
    }
}
