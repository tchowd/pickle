use pickle_core::*;
use serde_json::json;
use std::collections::BTreeMap;

fn bars(source: &str, ids: &[&str]) -> String {
    let _ = source;
    json!({"type":"bar_chart","bars":ids.iter().map(|id|json!({"evidenceID":id})).collect::<Vec<_>>()}).to_string()
}

#[test]
fn explicit_quantities_preserve_scale_and_units() {
    let values = numeric_evidence("Revenue was $10 million in 2024 and $15 million in 2025.");
    assert_eq!(
        values.iter().map(|v| v.value).collect::<Vec<_>>(),
        vec![10_000_000.0, 15_000_000.0]
    );
    assert!(values.iter().all(|v| v.unit == "USD"));
    assert!(numeric_evidence("2024 had 12 and 2025 had 16.").is_empty());
}
#[test]
fn malformed_numbers_cannot_become_partial_evidence() {
    assert!(numeric_evidence("$10,5 and $1e6 and 1.2.3 kg").is_empty());
    assert_eq!(numeric_evidence("The price is $10.")[0].value, 10.0);
    assert_eq!(
        numeric_evidence("Change was -12% and then 5 percent.")
            .iter()
            .map(|v| v.value)
            .collect::<Vec<_>>(),
        vec![-12.0, 5.0]
    );
}
#[test]
fn numeric_evidence_is_unicode_safe_and_excludes_examples() {
    let values = numeric_evidence("🫛 $10 and $20");
    assert_eq!(values[0].id, "n3");
    assert!(numeric_evidence("For example, $10 and $20 are hypothetical.").is_empty());
    assert!(numeric_evidence("$10 kg and $20%").is_empty());
}
#[test]
fn bars_resolve_only_source_ids() {
    let source = "$10 and $15";
    let values = numeric_evidence(source);
    let text = bars(source, &[&values[0].id, &values[1].id]);
    assert_eq!(
        validate_chart(&text, ChartKind::Bar, source).unwrap(),
        ValidatedChart::Bar(values)
    );
    assert!(validate_chart(&bars(source, &["n0", "fake"]), ChartKind::Bar, source).is_err());
    assert!(validate_chart(&bars(source, &["n0", "n0"]), ChartKind::Bar, source).is_err());
    assert!(
        validate_chart(
            r#"{"type":"bar_chart","bars":[{"evidenceID":"n0","value":10},{"evidenceID":"n8"}]}"#,
            ChartKind::Bar,
            source
        )
        .is_err()
    );
}
#[test]
fn mixed_units_and_unknown_fields_are_rejected() {
    let source = "$10 and 15%";
    let values = numeric_evidence(source);
    assert!(
        validate_chart(
            &bars(source, &[&values[0].id, &values[1].id]),
            ChartKind::Bar,
            source
        )
        .is_err()
    );
    assert!(
        validate_chart(
            r#"{"type":"bar_chart","bars":[],"html":"<script>"}"#,
            ChartKind::Bar,
            source
        )
        .is_err()
    );
}
fn flow(source: &str, labels: &[&str], condition: &str) -> String {
    json!({"type":"flow_diagram","nodes":labels.iter().enumerate().map(|(i,label)|json!({"id":format!("n{i}"),"label":label})).collect::<Vec<_>>(),"edges":labels.windows(2).enumerate().map(|(i,_)|json!({"from":format!("n{i}"),"to":format!("n{}",i+1),"evidence":source,"condition":condition})).collect::<Vec<_>>()}).to_string()
}
#[test]
fn flow_edges_and_conditions_require_literal_evidence() {
    let source = "If validated, it is saved.";
    assert!(
        validate_chart(
            &flow(source, &["validated", "saved"], ""),
            ChartKind::Flow,
            source
        )
        .is_err()
    );
    assert!(
        validate_chart(
            &flow(source, &["validated", "saved"], "If validated"),
            ChartKind::Flow,
            source
        )
        .is_ok()
    );
    assert!(
        validate_chart(
            &flow(source, &["validated", "invented"], "If validated"),
            ChartKind::Flow,
            source
        )
        .is_err()
    );
}
#[test]
fn explicit_process_cannot_omit_steps() {
    let source = "The request is received, validated, and saved.";
    assert_eq!(
        explicit_ordered_steps(source),
        vec!["received", "validated", "saved"]
    );
    assert!(
        validate_chart(
            &flow(source, &["received", "validated"], ""),
            ChartKind::Flow,
            source
        )
        .is_err()
    );
    let chart = validate_chart(
        &flow(source, &["received", "validated", "saved"], ""),
        ChartKind::Flow,
        source,
    )
    .unwrap();
    assert!(chart.alternative().contains("validated → saved"));
}
#[test]
fn schemas_exclude_unknown_fields_and_pin_evidence() {
    let schema = chart_schema(ChartKind::Bar, "$10 and $20");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["bars"]["items"]["properties"]["evidenceID"]["enum"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn probability_and_choice_validation_is_strict() {
    let questions = BTreeMap::from([("q".into(), DecisionQuestion::noul("Concern?"))]);
    for p in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(
            DecisionResponse {
                model: "test".into(),
                answers: BTreeMap::from([("q".into(), DecisionAnswer::noul(p))]),
                usage: None
            }
            .validate(&questions)
            .is_err()
        );
    }
    let choice = BTreeMap::from([(
        "q".into(),
        DecisionQuestion::choice(
            "Difficulty?",
            BTreeMap::from([
                ("easy".into(), "Easy".into()),
                ("hard".into(), "Hard".into()),
            ]),
        ),
    )]);
    let answer = DecisionAnswer {
        kind: "choice".into(),
        noul: None,
        choice: Some("easy".into()),
        confidence: Some(0.9),
        probabilities: Some(BTreeMap::from([("easy".into(), 0.1), ("hard".into(), 0.9)])),
    };
    assert!(
        DecisionResponse {
            model: "test".into(),
            answers: BTreeMap::from([("q".into(), answer)]),
            usage: None
        }
        .validate(&choice)
        .is_err()
    );
}
#[test]
fn threshold_boundaries_and_missing_answers_are_honest() {
    let q = BTreeMap::from([("q".into(), DecisionQuestion::noul("Concern?"))]);
    let response = |p| DecisionResponse {
        model: "test".into(),
        answers: BTreeMap::from([("q".into(), DecisionAnswer::noul(p))]),
        usage: None,
    };
    let policy = DecisionPolicy::default();
    assert_eq!(policy.quality(&response(0.2), &q), QualityStatus::Checked);
    assert_eq!(
        policy.quality(&response(0.8), &q),
        QualityStatus::Concerns(vec!["q".into()])
    );
    assert_eq!(
        policy.quality(
            &DecisionResponse {
                model: "test".into(),
                answers: BTreeMap::new(),
                usage: None
            },
            &q
        ),
        QualityStatus::Uncertain
    );
}
#[test]
fn request_limits_count_utf8_and_bound_history() {
    let mut input = RequestInput::new(
        SelectionSnapshot::new("Test", "Test", "test"),
        ReadingAction::Simplify,
    );
    input.context = "🫛".repeat(2000);
    assert!(input.validate().is_err());
    input.context.clear();
    input.conversation = (0..7)
        .map(|_| ConversationTurn {
            question: "q".into(),
            answer: "a".into(),
            quality: QualityStatus::Checked,
            label: None,
        })
        .collect();
    assert!(input.validate().is_err());
}
#[test]
fn public_reference_urls_are_syntactically_gated() {
    for url in [
        "http://127.0.0.1",
        "https://localhost",
        "https://user:pass@example.com",
        "https://device.local",
        "file:///tmp/test",
        "https://example.com:8080",
        "https://[::1]",
        "http://localhost.",
        "https://device.local.",
        "https://service.localhost.",
        "https://service.internal.",
    ] {
        assert!(WebReference::public_url(url).is_none(), "{url}");
    }
    assert_eq!(
        WebReference::public_url("https://example.com/a#secret")
            .unwrap()
            .as_str(),
        "https://example.com/a"
    );
}
#[test]
fn prompts_separate_supplemental_context_from_chart_evidence() {
    let mut input = RequestInput::new(
        SelectionSnapshot::new("A selected passage", "Test", "test"),
        ReadingAction::Simplify,
    );
    input.page_context = "OCR $10 and $20".into();
    input.visual_context = "Unverified summary".into();
    let prose = make_prompt(&input, None, None).unwrap();
    assert!(prose.user.contains("OCR $10"));
    input.action = ReadingAction::Chart;
    let chart = make_prompt(&input, None, Some(ChartKind::Bar)).unwrap();
    assert!(!chart.user.contains("OCR $10"));
    assert!(
        chart.schema.unwrap()["properties"]["bars"]["items"]["properties"]["evidenceID"]["enum"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn announcement_removal_preserves_substantive_openings() {
    assert_eq!(
        without_preamble("Here is a simplified explanation:\n\nIt might help."),
        "It might help."
    );
    assert_eq!(
        without_preamble("Here are the three causes:\nA, B, C."),
        "Here are the three causes:\nA, B, C."
    );
    assert_eq!(
        without_preamble("Here is a summary: it might help."),
        "Here is a summary: it might help."
    );
}

#[test]
fn references_preserve_provenance_and_bounded_relevant_excerpts() {
    let text = format!(
        "{}The selected mitochondrial process uses energy.\nMore information.",
        "Unrelated paragraph.\n".repeat(100)
    );
    let excerpt = WebReference::excerpt(&text, "mitochondrial", 100);
    assert!(excerpt.contains("mitochondrial"));
    assert!(excerpt.len() <= 100);
    let unicode = WebReference::excerpt(&"🌍".repeat(9000), "", 12000);
    assert_eq!(unicode.len(), 12000);
    let mut reference = WebReference {
        url: "https://example.com/article".into(),
        title: "Title".into(),
        kind: "video captions".into(),
        text: excerpt,
        timestamp: Some(32.5),
    };
    assert!(reference.validate().is_ok());
    assert!(reference.source().contains("Playback time: 32 seconds"));
    assert!(
        reference
            .source()
            .contains("URL: https://example.com/article")
    );
    reference.timestamp = Some(f64::NAN);
    assert!(reference.validate().is_err());
}

#[test]
fn page_only_sessions_validate_but_screen_text_cannot_create_chart_evidence() {
    let mut request = RequestInput::new(
        SelectionSnapshot::new("", "Browser", "browser"),
        ReadingAction::Simplify,
    );
    assert!(request.validate().is_err());
    request.reference = Some(WebReference {
        url: "https://example.com".into(),
        title: "Title".into(),
        kind: "article".into(),
        text: "Revenue was $20 and $40".into(),
        timestamp: None,
    });
    assert!(request.validate().is_ok());
    assert!(numeric_evidence(&request.source()).is_empty());
    assert!(request.reading_source().contains("$20"));
    request.page_context = "a".repeat(6001);
    assert!(request.validate().is_err());
    request.page_context.clear();
    request.visual_context = "a".repeat(3001);
    assert!(request.validate().is_err());
}
