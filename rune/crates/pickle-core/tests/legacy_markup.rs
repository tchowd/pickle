//! User-visible predicates from Tests/PickleCoreTests/AnswerMarkupTests.swift.
//! These test readable copy text, retained inline styles and inert rendering,
//! not Swift's line-oriented Block representation. Rune keeps CommonMark blocks.
use async_trait::async_trait;
use pickle_core::*;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

const LIST: &str = "The design involves several key components:\n\n1.  **Model Inference**: The API will use a large language model.\n2.  **GPU Batching**: The model will be run on a pool of GPUs.\n10) Wide markers stay numbered";
const INLINE: &str = "Use `snake_case` *carefully*, see [docs](https://example.com), keep <b>tags</b> and **unclosed";
const LEADS: [&str; 4] = [
    "Sure! Here's a simpler version:",
    "**Here is a brief summary of the passage:**",
    "Certainly, here are some examples:",
    "Below is an overview:",
];

fn styled_text(source: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut strong, mut emphasis, mut code) = (Vec::new(), Vec::new(), Vec::new());
    let (mut strong_depth, mut emphasis_depth) = (0, 0);
    for event in Parser::new_ext(&safe_answer_markup(source), Options::empty()) {
        match event {
            Event::Start(Tag::Strong) => strong_depth += 1,
            Event::End(TagEnd::Strong) => strong_depth -= 1,
            Event::Start(Tag::Emphasis) => emphasis_depth += 1,
            Event::End(TagEnd::Emphasis) => emphasis_depth -= 1,
            Event::Text(text) => {
                if strong_depth > 0 {
                    strong.push(text.to_string());
                }
                if emphasis_depth > 0 {
                    emphasis.push(text.to_string());
                }
            }
            Event::Code(text) => code.push(text.to_string()),
            _ => {}
        }
    }
    (strong, emphasis, code)
}

#[test]
fn legacy_numbered_list_copy_preserves_original_one_two_ten_markers() {
    assert_eq!(
        answer_plain_text(LIST),
        "The design involves several key components:\n\n1. Model Inference: The API will use a large language model.\n2. GPU Batching: The model will be run on a pool of GPUs.\n10. Wide markers stay numbered"
    );
}

#[test]
fn legacy_numbered_list_strong_runs_survive_sanitization() {
    assert_eq!(styled_text(LIST).0, ["Model Inference", "GPU Batching"]);
}

fn rendered_numbers(markup: &str) -> (Vec<u64>, Vec<u64>) {
    let mut starts = Vec::new();
    let mut items = Vec::new();
    let mut lists = Vec::new();
    for event in Parser::new_ext(markup, Options::empty()) {
        match event {
            Event::Start(Tag::List(start)) => {
                if let Some(number) = start {
                    starts.push(number);
                }
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                if let Some(Some(number)) = lists.last_mut() {
                    items.push(*number);
                    *number += 1;
                }
            }
            _ => {}
        }
    }
    (starts, items)
}

#[test]
fn legacy_distinct_list_starts_survive_actual_safe_markdown_reparsing() {
    for source in [
        LIST,
        "1) First\n2) Second\n\n10. Tenth",
        "  1. First\n  2. Second\n\n  10) Tenth",
    ] {
        let safe = safe_answer_markup(source);
        assert_eq!(rendered_numbers(&safe), (vec![1, 10], vec![1, 2, 10]));
        let again = safe_answer_markup(&safe);
        assert_eq!(rendered_numbers(&again), (vec![1, 10], vec![1, 2, 10]));
        assert_eq!(answer_plain_text(&safe), answer_plain_text(&again));
    }
    assert_eq!(
        answer_plain_text("```\n1. code\n10) literal marker\n```"),
        "1. code\n10) literal marker"
    );
}

#[test]
fn legacy_inline_copy_and_styles_remain_inert() {
    assert_eq!(
        answer_plain_text(INLINE),
        "Use snake_case carefully, see docs, keep <b>tags</b> and **unclosed"
    );
    let (_, emphasis, code) = styled_text(INLINE);
    assert_eq!(emphasis, ["carefully"]);
    assert_eq!(code, ["snake_case"]);
    let safe = safe_answer_markup(INLINE);
    assert!(!safe.contains("https://example.com"));
    assert!(safe.contains("&lt;b&gt;tags&lt;/b&gt;"));
    assert!(
        Parser::new_ext(&safe, Options::empty()).all(|event| !matches!(
            event,
            Event::Start(Tag::Link { .. } | Tag::Image { .. })
                | Event::Html(_)
                | Event::InlineHtml(_)
        ))
    );
}

#[test]
fn legacy_ordinary_punctuation_unclosed_strong_and_empty_copy() {
    let punctuation = "-5 degrees, #hashtag, 2024. A year\n2 * 3 = 6";
    assert_eq!(answer_plain_text(punctuation), punctuation);
    assert_eq!(answer_plain_text("**unclosed"), "**unclosed");
    assert_eq!(answer_plain_text(""), "");
}

#[test]
fn legacy_complete_announcement_variants_are_removed() {
    let mut mismatches = Vec::new();
    for lead in LEADS {
        for newline in ["\n", "\r\n"] {
            let source = format!("{lead}{newline}- First point");
            let actual = without_preamble(&source);
            if actual != "- First point" {
                mismatches.push((source, actual));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "Legacy preamble mismatches: {mismatches:?}"
    );
}

#[test]
fn legacy_content_bearing_inline_and_lone_lead_ins_stay_intact() {
    for source in [
        "Here are the three causes:\n1. Heat",
        "Here is a simplified explanation: it batches requests.\nMore.",
        "Here is a simplified explanation:",
        "The explanation has two parts:\nFirst.",
    ] {
        for newline in ["\n", "\r\n"] {
            let source = source.replace('\n', newline);
            assert_eq!(without_preamble(&source), source);
        }
    }
}

struct LegacyWriter {
    prompts: Mutex<Vec<GenerationPrompt>>,
    output: String,
}
#[async_trait]
impl GenerativeProvider for LegacyWriter {
    fn is_remote(&self) -> bool {
        false
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        _: UnboundedSender<PipelineEvent>,
        _: CancellationToken,
    ) -> Result<Generation> {
        self.prompts.lock().unwrap().push(prompt);
        Ok(Generation::new(&self.output, "legacy-fixture"))
    }
}

#[tokio::test]
async fn legacy_actual_pipeline_result_and_format_instructions_match_reader() {
    let expected = "The problem is to design an API.";
    let mut outputs = vec![format!(
        "Here is a simplified explanation of the problem statement:\n\n{expected}"
    )];
    for lead in LEADS {
        for newline in ["\n", "\r\n"] {
            outputs.push(format!("{lead}{newline}{expected}"));
        }
    }
    for output in outputs {
        let writer = Arc::new(LegacyWriter {
            prompts: Mutex::new(Vec::new()),
            output,
        });
        let (events, _) = unbounded_channel();
        let outcome = Pipeline::new(writer.clone(), None, false)
            .run(
                RequestInput::new(
                    SelectionSnapshot::new("Design an API.", "Test", "test"),
                    ReadingAction::Simplify,
                ),
                events,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let PipelineOutcome::Result(result) = outcome else {
            panic!("Expected the original fixture's accepted result");
        };
        assert_eq!(result.text, expected);
        assert_eq!(answer_plain_text(&result.text), expected);
        let prompts = writer.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        for instruction in [
            "Begin directly with the answer",
            "Light Markdown is fine",
            "No HTML, tables",
        ] {
            assert!(prompts[0].system.contains(instruction));
        }
    }
}
