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
fn unicode_word_boundaries_cannot_turn_identifiers_into_chart_quantities() {
    // Foundation's original \w guards reject all these word characters on
    // either side of a numeric token, including non-ASCII marks/connectors.
    for character in ['\u{301}', '\u{93e}', '\u{203f}', '\u{200c}', '\u{200d}'] {
        for source in [
            format!("The identifiers are v{character}10users and v{character}20users."),
            format!("The identifiers are $10{character}x and $20{character}x."),
            format!("The identifiers are 10%{character}x and 20%{character}x."),
        ] {
            assert!(numeric_evidence(&source).is_empty(), "{source:?}");
        }
    }
    // This exact graph was accepted before the boundary fix. The IDs still
    // point at the same source positions, but neither substring is evidence.
    let source = "The identifiers are v\u{301}10users and v\u{301}20users.";
    let candidate = bars(source, &["n22", "n36"]);
    assert!(validate_chart(&candidate, ChartKind::Bar, source).is_err());
    assert_eq!(
        chart_schema(ChartKind::Bar, source)["properties"]["bars"]["items"]["properties"]["evidenceID"]
            ["enum"],
        json!([]),
    );
}

#[test]
fn independent_quantities_surrounded_by_unicode_keep_values_and_source_ids() {
    for source in [
        "測定値は「10users」と「20users」。",
        "🫛 Café: 10users — 結果: 20users.",
    ] {
        let values = numeric_evidence(source);
        assert_eq!(
            values.iter().map(|value| value.value).collect::<Vec<_>>(),
            vec![10.0, 20.0]
        );
        assert!(values.iter().all(|value| value.unit == "users"));
        for value in &values {
            let offset = source.find(&value.quote).unwrap();
            assert_eq!(
                value.id,
                format!("n{}", source[..offset].encode_utf16().count())
            );
        }
        let candidate = bars(source, &[&values[0].id, &values[1].id]);
        assert_eq!(
            validate_chart(&candidate, ChartKind::Bar, source).unwrap(),
            ValidatedChart::Bar(values)
        );
    }
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
fn long_flow_evidence_keeps_versions_decimals_and_unicode_sentences_literal() {
    // These expected boundaries were compared with Foundation .bySentences.
    for (passage, expected) in [
        (
            "The service migrates from v1.2 to v2.0.",
            vec!["The service migrates from v1.2 to v2.0."],
        ),
        (
            "The dose falls from 1.5 kg to 0.5 kg.",
            vec!["The dose falls from 1.5 kg to 0.5 kg."],
        ),
        (
            "受付したら検証する。次に保存する。",
            vec!["受付したら検証する。", "次に保存する。"],
        ),
        ("First step\n\nNext step", vec!["First step", "Next step"]),
        (
            "Dr. Smith receives it, then forwards it.",
            vec!["Dr.", "Smith receives it, then forwards it."],
        ),
        (
            "The U.S. service receives requests, then saves them.",
            vec!["The U.S. service receives requests, then saves them."],
        ),
        (
            "Use e.g. staging before production.",
            vec!["Use e.g. staging before production."],
        ),
    ] {
        let prefix = "Background. ".repeat(110);
        let source = format!("{prefix}{passage}");
        let quotes = flow_evidence(&source);
        assert_eq!(&quotes[110..], expected, "{passage}");
        assert!(quotes.iter().all(|quote| !quote.is_empty()
            && quote.chars().count() <= 1200
            && source.contains(quote)));
    }
}

#[test]
fn flow_evidence_preserves_whole_short_sources_and_refuses_oversize_sentences() {
    for source in [
        " First sentence.\n\nSecond sentence. ".to_owned(),
        "🫛".repeat(1200),
    ] {
        assert_eq!(flow_evidence(&source), vec![source.clone()]);
    }
    assert!(flow_evidence(&"🫛".repeat(1201)).is_empty());
    let source = format!("{}.\nShort source.", "x".repeat(1201));
    assert_eq!(flow_evidence(&source), vec!["Short source."]);
}

#[test]
fn long_version_relationship_has_schema_permitted_evidence_and_validates() {
    let quote = "The service migrates from v1.2 to v2.0.";
    let source = format!("{}{quote}", "Background. ".repeat(110));
    let schema = chart_schema(ChartKind::Flow, &source);
    let allowed = schema["properties"]["edges"]["items"]["properties"]["evidence"]["enum"]
        .as_array()
        .unwrap();
    assert!(allowed.iter().any(|value| value == quote));
    let mut candidate = json!({"type":"flow_diagram","nodes":[{"id":"old","label":"v1.2"},{"id":"new","label":"v2.0"}],"edges":[{"from":"old","to":"new","evidence":quote,"condition":""}]});
    let validated = validate_chart(&candidate.to_string(), ChartKind::Flow, &source).unwrap();
    assert!(matches!(validated, ValidatedChart::Flow { .. }));
    candidate["edges"][0]["evidence"] = json!("The service migrates from v1.2 directly to v2.0.");
    assert!(
        validate_chart(&candidate.to_string(), ChartKind::Flow, &source).is_err(),
        "Sentence segmentation must not authorize an invented quote"
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
fn explicit_flow_schema_and_instruction_map_share_finite_step_names() {
    let source = "The request is received, validated, and saved.";
    let schema = chart_schema(ChartKind::Flow, source);
    let nodes = &schema["properties"]["nodes"];
    let edges = &schema["properties"]["edges"];
    let names = json!({"type":"string","enum":["s0","s1","s2"]});
    assert_eq!(nodes["items"]["properties"]["id"], names);
    assert_eq!(edges["items"]["properties"]["from"], names);
    assert_eq!(edges["items"]["properties"]["to"], names);
    assert_eq!(
        nodes["items"]["properties"]["label"]["enum"],
        json!(["received", "validated", "saved"])
    );
    assert_eq!(nodes["minItems"], 3);
    assert_eq!(nodes["maxItems"], 3);
    assert_eq!(edges["minItems"], 2);
    assert_eq!(edges["maxItems"], 2);
    assert_eq!(
        edges["items"]["properties"]["evidence"]["enum"],
        json!([source])
    );
    assert_eq!(
        edges["items"]["properties"]["condition"],
        json!({"type":"string","enum":[""]})
    );
    let instructions = chart_instructions(ChartKind::Flow, source);
    let mapping = instructions
        .split_once("Source step node IDs (names only): ")
        .unwrap()
        .1;
    let mapping = serde_json::Deserializer::from_str(mapping)
        .into_iter::<serde_json::Value>()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(
        mapping,
        json!([
            {"id":"s0","label":"received"},
            {"id":"s1","label":"validated"},
            {"id":"s2","label":"saved"},
        ])
    );
    assert!(
        mapping
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node.as_object().unwrap().len() == 2)
    );
    assert!(
        instructions.contains("Edge from/to must use these IDs, never labels or source sentences.")
    );
}

#[test]
fn general_flow_schema_and_instructions_remain_unconstrained_by_step_names() {
    let source = "If validated, it is saved.";
    assert!(explicit_ordered_steps(source).is_empty());
    let schema = chart_schema(ChartKind::Flow, source);
    let nodes = &schema["properties"]["nodes"];
    let edges = &schema["properties"]["edges"];
    let id = json!({"type":"string","maxLength":40});
    assert_eq!(nodes["items"]["properties"]["id"], id);
    assert_eq!(edges["items"]["properties"]["from"], id);
    assert_eq!(edges["items"]["properties"]["to"], id);
    assert_eq!(
        nodes["items"]["properties"]["label"],
        json!({"type":"string","maxLength":180})
    );
    assert_eq!(nodes["minItems"], 2);
    assert_eq!(nodes["maxItems"], 10);
    assert!(!chart_instructions(ChartKind::Flow, source).contains("Source step node IDs"));
    assert!(
        validate_chart(
            &flow(source, &["validated", "saved"], "If validated"),
            ChartKind::Flow,
            source
        )
        .is_ok()
    );
}

fn named_step_flow(source: &str, condition: &str) -> serde_json::Value {
    json!({"type":"flow_diagram",
        "nodes":[{"id":"s0","label":"received"},{"id":"s1","label":"validated"},{"id":"s2","label":"saved"}],
        "edges":[{"from":"s0","to":"s1","evidence":source,"condition":condition},{"from":"s1","to":"s2","evidence":source,"condition":condition}]
    })
}

#[test]
fn finite_step_names_do_not_bypass_existing_flow_semantic_rejections() {
    let source = "The request is received, validated, and saved.";
    let valid = named_step_flow(source, "");
    let chart = validate_chart(&valid.to_string(), ChartKind::Flow, source).unwrap();
    assert!(chart.alternative().contains("received → validated"));
    let mut invalid = Vec::new();
    let mut duplicate = valid.clone();
    duplicate["nodes"][1]["id"] = json!("s0");
    invalid.push(duplicate);
    let mut sentence_endpoint = valid.clone();
    sentence_endpoint["edges"][0]["from"] = json!("The request");
    invalid.push(sentence_endpoint);
    let mut omitted_step = valid.clone();
    omitted_step["nodes"].as_array_mut().unwrap().remove(1);
    omitted_step["edges"] = json!([{"from":"s0","to":"s2","evidence":source,"condition":""}]);
    invalid.push(omitted_step);
    let mut reversed = valid.clone();
    reversed["edges"][0]["from"] = json!("s1");
    reversed["edges"][0]["to"] = json!("s0");
    invalid.push(reversed);
    let mut invented_condition = valid.clone();
    invented_condition["edges"][0]["condition"] = json!("exact");
    invalid.push(invented_condition);
    let mut invented_evidence = valid.clone();
    invented_evidence["edges"][0]["evidence"] = json!("received and validated");
    invalid.push(invented_evidence);
    let mut disconnected = valid.clone();
    disconnected["edges"].as_array_mut().unwrap().pop();
    invalid.push(disconnected);
    let mut self_loop = valid.clone();
    self_loop["edges"][0]["to"] = json!("s0");
    invalid.push(self_loop);
    for candidate in invalid {
        assert!(
            validate_chart(&candidate.to_string(), ChartKind::Flow, source).is_err(),
            "Accepted invalid candidate: {candidate}"
        );
    }
    // The validator still accepts arbitrary consistent names: only generation
    // guidance/schema changed, not the supported graph semantics.
    assert!(
        validate_chart(
            &flow(source, &["received", "validated", "saved"], ""),
            ChartKind::Flow,
            source
        )
        .is_ok()
    );
}

#[test]
fn finite_named_steps_preserve_conditional_requirements_and_graph_size_limits() {
    let source = "If approved, the request is received, validated, and saved.";
    assert!(
        validate_chart(
            &named_step_flow(source, "").to_string(),
            ChartKind::Flow,
            source
        )
        .is_err()
    );
    assert!(
        validate_chart(
            &named_step_flow(source, "If approved").to_string(),
            ChartKind::Flow,
            source
        )
        .is_ok()
    );
    let simple = "The request is received, validated, and saved.";
    let mut too_many_nodes = named_step_flow(simple, "");
    too_many_nodes["nodes"] = json!(
        (0..11)
            .map(|i| json!({"id":format!("s{i}"),"label":"received"}))
            .collect::<Vec<_>>()
    );
    assert!(validate_chart(&too_many_nodes.to_string(), ChartKind::Flow, simple).is_err());
    let mut too_many_edges = named_step_flow(simple, "");
    too_many_edges["edges"] = json!(vec![too_many_edges["edges"][0].clone(); 17]);
    assert!(validate_chart(&too_many_edges.to_string(), ChartKind::Flow, simple).is_err());
}

#[test]
fn sole_operation_list_rejects_observed_cycles_and_nonconditional_condition_text() {
    let source = "The request is received, validated, and saved.";
    let valid = named_step_flow(source, "");
    assert!(validate_chart(&valid.to_string(), ChartKind::Flow, source).is_ok());
    let mut cyclic = valid.clone();
    cyclic["edges"].as_array_mut().unwrap().push(json!({
        "from":"s2","to":"s0","evidence":source,"condition":""
    }));
    assert!(validate_chart(&cyclic.to_string(), ChartKind::Flow, source).is_err());
    let mut shortcut = valid.clone();
    shortcut["edges"].as_array_mut().unwrap().push(json!({
        "from":"s0","to":"s2","evidence":source,"condition":""
    }));
    assert!(validate_chart(&shortcut.to_string(), ChartKind::Flow, source).is_err());
    for condition in [source, "received", "validated"] {
        let mut candidate = valid.clone();
        candidate["edges"][0]["condition"] = json!(condition);
        assert!(validate_chart(&candidate.to_string(), ChartKind::Flow, source).is_err());
    }
    let mut duplicate_label = valid;
    duplicate_label["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"extra","label":"validated"}));
    duplicate_label["edges"][1]["from"] = json!("extra");
    assert!(validate_chart(&duplicate_label.to_string(), ChartKind::Flow, source).is_err());
    // Names are presentation identifiers, not a new semantic requirement.
    assert!(
        validate_chart(
            &flow(source, &["received", "validated", "saved"], ""),
            ChartKind::Flow,
            source
        )
        .is_ok()
    );
}

#[test]
fn sole_operation_constraints_decline_ambiguous_or_partial_source_forms() {
    for (source, count) in [
        ("The request is received, validated, and saved.", 3),
        ("Records are created, checked and stored", 3),
        ("A library request is queued and reviewed.", 2),
        (
            "The event is recorded, processed, delivered, and archived.",
            4,
        ),
    ] {
        let schema = chart_schema(ChartKind::Flow, source);
        let edges = &schema["properties"]["edges"];
        assert_eq!(edges["minItems"], count - 1, "{source}");
        assert_eq!(edges["maxItems"], count - 1, "{source}");
        assert_eq!(
            edges["items"]["properties"]["condition"],
            json!({"type":"string","enum":[""]})
        );
    }
    for source in [
        "The patient is tired, bored, and worried.",
        "The request is received, decorated, and saved.",
        "The request is not received, validated, and saved.",
        "If approved, the request is received, validated, and saved.",
        "The request is received, validated, and saved unless retried.",
        "The request that is approved is received, validated, and saved.",
        "The request is received, validated, and saved. A saved request is received again.",
        "The saved request is received, validated, and saved.",
        "The request may be received, validated, and saved.",
        "The iterative request is received, validated, and saved.",
        "The request is received, validated, and saved!",
        "The request is received,\nvalidated, and saved.",
        "The request is received,\u{2028}validated, and saved.",
        "The request is received, received, and saved.",
        "The request is RECEIVED, received, and SAVED.",
        "Request: it is received, validated, and saved.",
    ] {
        let schema = chart_schema(ChartKind::Flow, source);
        let edges = &schema["properties"]["edges"];
        assert_eq!(edges["minItems"], 1, "Narrowed ambiguous source: {source}");
        assert_eq!(edges["maxItems"], 16, "Narrowed ambiguous source: {source}");
        assert_eq!(
            edges["items"]["properties"]["condition"],
            json!({"type":"string","maxLength":180}),
            "{source}"
        );
        assert!(
            !chart_instructions(ChartKind::Flow, source)
                .contains("For this whole-source operation-list format")
        );
    }
}

#[test]
fn conditional_multiclause_and_multisentence_cycles_keep_the_general_path() {
    for (source, forward_condition, back_condition) in [
        (
            "The request is received, validated, and saved. A saved request is received again.",
            "",
            "",
        ),
        (
            "If approved, the request is received, validated, and saved; if retried, the saved request is received again.",
            "If approved",
            "if retried",
        ),
    ] {
        let mut spec = named_step_flow(source, forward_condition);
        spec["edges"].as_array_mut().unwrap().push(json!({
            "from":"s2","to":"s0","evidence":source,"condition":back_condition
        }));
        assert!(
            validate_chart(&spec.to_string(), ChartKind::Flow, source).is_ok(),
            "Rejected existing general flow: {source}"
        );
        let schema = chart_schema(ChartKind::Flow, source);
        assert_eq!(schema["properties"]["edges"]["maxItems"], 16);
        assert!(
            schema["properties"]["edges"]["items"]["properties"]["condition"]
                .get("enum")
                .is_none()
        );
    }
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
    // Exact legacy absence predicate belongs to response validation, before
    // policy's separate missing-answer -> Uncertain fallback can run.
    assert!(
        DecisionResponse {
            model: "test".into(),
            answers: BTreeMap::new(),
            usage: None,
        }
        .validate(&questions)
        .is_err()
    );
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
