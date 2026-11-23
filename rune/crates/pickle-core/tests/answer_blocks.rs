use pickle_core::{AnswerBlockKind as Kind, AnswerRun, answer_document, answer_plain_text};
use serde_json::{Value, json};

const STRUCTURE: &str = "## Overview ##\nFirst line\nsecond line\n\n- Parent\n  continued\n    * Child\n- Sibling\n\n---\n> Quoted\n> text\n```\nlet x = 1\n```\n```\nstill streaming";

#[test]
fn original_block_structure_keeps_hard_lines_markers_quote_and_streaming_code() {
    let document = answer_document(STRUCTURE);
    assert_eq!(
        document.blocks.iter().map(|b| &b.kind).collect::<Vec<_>>(),
        [
            &Kind::Heading(2),
            &Kind::Paragraph,
            &Kind::Bullet,
            &Kind::Bullet,
            &Kind::Bullet,
            &Kind::Quote,
            &Kind::Code,
            &Kind::Code
        ]
    );
    assert_eq!(
        document.blocks.iter().map(|b| b.depth).collect::<Vec<_>>(),
        [0, 0, 0, 1, 0, 0, 0, 0]
    );
    assert_eq!(document.blocks[0].text(), "Overview");
    assert_eq!(document.blocks[1].text(), "First line\nsecond line");
    assert_eq!(document.blocks[2].text(), "Parent\ncontinued");
    assert_eq!(document.blocks[3].marker(), Some("◦"));
    assert_eq!(document.blocks[4].marker(), Some("•"));
    assert_eq!(document.blocks[5].text(), "Quoted\ntext");
    assert_eq!(
        document.blocks[6].runs,
        [AnswerRun {
            text: "let x = 1".into(),
            code: true,
            ..AnswerRun::default()
        }]
    );
    assert_eq!(document.blocks[7].text(), "still streaming");
    assert_eq!(
        document.plain_text(),
        "Overview\n\nFirst line\nsecond line\n\n• Parent\ncontinued\n  ◦ Child\n• Sibling\n\nQuoted\ntext\n\nlet x = 1\n\nstill streaming"
    );
    assert_eq!(document.plain_text(), answer_plain_text(STRUCTURE));
}

#[test]
fn foundation_oracle_covers_real_blocks_and_inline_only_edge_cases() {
    // Expected values came from the actual original AnswerMarkup.swift compiled
    // against Foundation, not from this parser or a rewritten Swift algorithm.
    // All inputs are fixed synthetic developer fixtures; no native test/UI IO.
    let fixtures: Value =
        serde_json::from_str(include_str!("fixtures/answer_blocks_foundation.json")).unwrap();
    let cases = fixtures["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 99);
    for fixture in cases {
        let source = fixture["source"].as_str().unwrap();
        let document = answer_document(source);
        let blocks: Vec<Value> = document.blocks.iter().map(|block| {
            let (kind, level, number) = match &block.kind {
                Kind::Paragraph => ("paragraph", 0, ""),
                Kind::Heading(level) => ("heading", *level, ""),
                Kind::Bullet => ("bullet", 0, ""),
                Kind::Numbered(number) => ("numbered", 0, number.as_str()),
                Kind::Quote => ("quote", 0, ""),
                Kind::Code => ("code", 0, ""),
            };
            json!({"kind":kind,"level":level,"number":number,"depth":block.depth,"text":block.text(),"marker":block.marker(),"runs":block.runs.iter().map(|run|json!({"text":run.text,"strong":run.strong,"emphasis":run.emphasis,"code":run.code})).collect::<Vec<_>>()})
        }).collect();
        assert_eq!(
            json!(blocks),
            fixture["blocks"],
            "blocks/runs for {}",
            fixture["id"]
        );
        assert_eq!(
            document.plain_text(),
            fixture["plain_text"].as_str().unwrap(),
            "copy for {}",
            fixture["id"]
        );
        assert_eq!(
            answer_plain_text(source),
            document.plain_text(),
            "single copy writer for {}",
            fixture["id"]
        );
    }
}

#[test]
fn literal_numbers_unicode_bullets_and_depth_are_not_commonmark_renumbered() {
    let document = answer_document(
        "001) One\n7. Seven\n• Parent\n\t– Child\n\t\t- Grandchild\n\t\t\t+ Third\n\t\t\t\t- Fourth",
    );
    assert_eq!(document.blocks[0].marker(), Some("001."));
    assert_eq!(document.blocks[1].marker(), Some("7."));
    assert_eq!(
        document.blocks.iter().map(|b| b.depth).collect::<Vec<_>>(),
        [0, 0, 0, 1, 2, 3, 3]
    );
    assert_eq!(document.blocks[6].marker(), Some("◦"));
    assert_eq!(answer_plain_text("## Title###"), "Title");
}

#[test]
fn labels_and_html_remain_inert_even_for_unsafe_destinations() {
    let document = answer_document(
        "[run](javascript:alert(1)) [file](file:///synthetic) ![alt](data:image/png;base64,AA==) <img src=\"https://synthetic.invalid/pixel\">\n\n```mermaid\ngraph LR; A-->B\n```",
    );
    assert_eq!(
        document.blocks[0].text(),
        "run file alt <img src=\"https://synthetic.invalid/pixel\">"
    );
    assert!(
        document.blocks[0]
            .runs
            .iter()
            .all(|r| !r.code && !r.strong && !r.emphasis)
    );
    assert_eq!(document.blocks[1].kind, Kind::Code);
    assert_eq!(document.blocks[1].text(), "graph LR; A-->B");
    // The document API has only literal text/style flags, no URL/image/HTML
    // handler, destination, code language, or external resource representation.
}

#[test]
fn repeated_unmatched_strike_closers_stay_literal_and_new_openers_still_work() {
    let prefix = "word~ ".repeat(2_000);
    assert_eq!(answer_plain_text(&prefix), prefix.trim_end());
    assert_eq!(
        answer_plain_text(&(prefix.clone() + "~one~ ~~two~~")),
        prefix + "one two"
    );
    // A wrapper may consume an opener into another inline subtree. A later
    // fresh opener must still pair, without leaking any previous search hint.
    assert_eq!(answer_plain_text("**~old** end~ ~new~"), "~old end~ new");
}

#[test]
fn nested_inline_parse_and_consuming_drop_fit_a_one_megabyte_thread_stack() {
    let worker = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let deep = format!("{}x{}", "*".repeat(16_383), "*".repeat(16_383));
            let document = answer_document(&deep);
            assert_eq!(document.blocks.len(), 1);
            assert_eq!(document.blocks[0].text(), "x");
            assert!(document.blocks[0].runs[0].strong && document.blocks[0].runs[0].emphasis);
            drop(document);
            for source in [
                format!("{}x{}", "_*".repeat(2_999), "*_".repeat(2_999)),
                format!("{}{}", "[".repeat(6_000), "]".repeat(6_000)),
                "**one\n\n`two\n\nthree` four** ".repeat(400),
            ] {
                let document = answer_document(&source);
                assert!(!document.blocks.is_empty());
                assert!(!document.plain_text().is_empty());
                drop(document);
            }
        })
        .unwrap();
    worker.join().unwrap();
}
