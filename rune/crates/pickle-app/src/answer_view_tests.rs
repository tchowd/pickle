//! Real Rune layout, selection and memory-clipboard checks, without native UI.
use super::*;
use rune::{Cx, Env, Key, KeyEvent, Mods, Runtime};

const ORIGINAL_BLOCKS: &str = "## Overview ##\nFirst line\nsecond line\n\n- Parent\n  continued\n    * Child\n- Sibling\n\n---\n> Quoted\n> text\n```\nlet x = 1\n```\n```\nstill streaming";

struct State {
    source: String,
    size: f32,
    streaming: bool,
    unrelated: usize,
    saved: Option<(crate::storage::Entry, bool)>,
}

fn view(state: &State, _: &mut Cx<State>) -> El {
    col()
        .w_full()
        .padding(10.0)
        .child(if let Some((entry, preview)) = &state.saved {
            super::super::saved_answer(entry, state.size, *preview)
        } else {
            render(
                "fixture-document".into(),
                &state.source,
                state.size,
                state.streaming,
            )
        })
        .child(text(format!("Other message {}", state.unrelated)).key("other-message"))
}

fn runtime(source: &str, width: f32) -> Runtime<State> {
    let mut runtime = Runtime::new(
        State {
            source: source.into(),
            size: 16.0,
            streaming: false,
            unrelated: 0,
            saved: None,
        },
        view,
        (width, 1400.0),
        Env::new(true, 1.0),
    );
    settle(&mut runtime);
    runtime
}

fn settle(runtime: &mut Runtime<State>) {
    for _ in 0..4 {
        runtime.frame(1.0 / 60.0);
    }
}

fn command(runtime: &mut Runtime<State>, key: char) {
    runtime.key(KeyEvent {
        key: Key::Command(key),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    });
    settle(runtime);
}

fn select(runtime: &mut Runtime<State>, key: &str, range: std::ops::Range<usize>) {
    runtime.update(|_, tx| tx.select_text(key, range));
    settle(runtime);
}

fn element(runtime: &Runtime<State>, key: &str) -> rune::runtime::ElementInfo {
    runtime
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("missing {key}"))
}

#[test]
fn original_blocks_keep_hard_lines_nested_markers_and_unclosed_code_in_real_layout() {
    let runtime = runtime(ORIGINAL_BLOCKS, 480.0);
    let bodies: Vec<_> = runtime
        .elements()
        .into_iter()
        .filter(|element| {
            element
                .key
                .as_deref()
                .is_some_and(|key| key.starts_with("block-body-"))
        })
        .collect();
    assert_eq!(
        bodies
            .iter()
            .map(|element| element.text.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [
            "Overview",
            "First line\nsecond line",
            "Parent\ncontinued",
            "Child",
            "Sibling",
            "Quoted\ntext",
            "let x = 1",
            "still streaming"
        ]
    );
    assert_eq!(bodies[0].role, "heading");
    assert_eq!(bodies[0].level, Some(2));
    for body in &bodies {
        assert!(body.opacity > 0.99 && body.rect[2] > 0.0 && body.rect[3] > 0.0);
        assert!(body.rect[0] >= 10.0 && body.rect[0] + body.rect[2] <= 480.01);
    }
    assert!(
        bodies[1].rect[3] > bodies[4].rect[3] * 1.9,
        "the explicit line break must paint two lines"
    );
    assert!(bodies[2].rect[3] > bodies[4].rect[3] * 1.9);
    assert_eq!(
        element(&runtime, "block-marker-2").text.as_deref(),
        Some("•")
    );
    assert_eq!(
        element(&runtime, "block-marker-3").text.as_deref(),
        Some("◦")
    );
    assert_eq!(
        element(&runtime, "block-marker-4").text.as_deref(),
        Some("•")
    );
    assert!(bodies[3].rect[0] > bodies[2].rect[0]);
    assert!(bodies[5].rect[0] > bodies[1].rect[0]);
}

#[test]
fn explicit_numbering_and_wrapped_hanging_columns_survive_a_narrow_reader() {
    let source = format!(
        "001. {}\n7. Seven\n10) Ten",
        "A long numbered item with careful evidence and qualifiers. ".repeat(8)
    );
    let runtime = runtime(&source, 480.0);
    assert_eq!(
        (0..3)
            .map(|i| element(&runtime, &format!("block-marker-{i}"))
                .text
                .unwrap())
            .collect::<Vec<_>>(),
        ["001.", "7.", "10."]
    );
    let first = element(&runtime, "block-body-0");
    assert!(first.rect[3] > 80.0);
    for index in 0..3 {
        let body = element(&runtime, &format!("block-body-{index}"));
        let marker = element(&runtime, &format!("block-marker-{index}"));
        assert!((body.rect[0] - first.rect[0]).abs() < 0.1);
        assert!(body.rect[0] > marker.rect[0]);
        assert!(body.rect[0] + body.rect[2] <= 480.01);
    }
}

#[test]
fn real_group_selection_copies_canonical_blocks_and_preserves_partial_unicode_ranges() {
    let source = "Café **energy** and [docs](https://example.com).\n\n1. First\n7. Seventh\n\n- Parent\n    * Child\n\n```\n```\n\n> One\n> two";
    let mut runtime = runtime(source, 480.0);
    let clipboard = rune::clipboard::memory();
    runtime.use_clipboard(&clipboard);
    let body = element(&runtime, "block-body-0");
    let x0 = body.rect[0] + 0.1;
    let y = body.rect[1] + 12.0;
    let x1 = body.rect[0] + rune::text::measure("Café", &[], TextStyle::new(16.0, 400.0), None).0;
    runtime.pointer_move(x0, y);
    runtime.pointer_down(x0, y);
    for step in 1..=6 {
        runtime.pointer_move(x0 + (x1 - x0) * step as f32 / 6.0, y);
        settle(&mut runtime);
    }
    runtime.pointer_up(x1, y);
    command(&mut runtime, 'c');
    assert_eq!(
        clipboard.text().unwrap().as_str(),
        "Café",
        "real pointer selection must retain the Unicode word"
    );
    select(&mut runtime, "block-body-0", 0..5);
    command(&mut runtime, 'c');
    assert_eq!(clipboard.text().unwrap().as_str(), "Café");
    let first = element(&runtime, "block-body-0").text.unwrap();
    select(&mut runtime, "block-body-0", 0..first.len());
    command(&mut runtime, 'c');
    assert_eq!(clipboard.text().unwrap().as_str(), first);
    command(&mut runtime, 'a');
    command(&mut runtime, 'c');
    assert_eq!(
        clipboard.text().unwrap().as_str(),
        "Café energy and docs.\n\n1. First\n7. Seventh\n• Parent\n  ◦ Child\n\n\n\nOne\ntwo"
    );
    assert_eq!(runtime.state.source, source);
    assert!(!clipboard.text().unwrap().contains("Other message"));
}

#[test]
fn memo_reuses_unchanged_answers_and_invalidates_for_text_size_and_draft_changes() {
    BUILDS.with(|count| count.set(0));
    let mut runtime = runtime("First **answer**.", 480.0);
    let baseline = BUILDS.with(std::cell::Cell::get);
    for _ in 0..12 {
        runtime.state.unrelated += 1;
        runtime.invalidate();
        settle(&mut runtime);
    }
    assert_eq!(BUILDS.with(std::cell::Cell::get), baseline);
    let height = element(&runtime, "block-body-0").rect[3];
    runtime.state.size = 24.0;
    runtime.invalidate();
    settle(&mut runtime);
    assert_eq!(BUILDS.with(std::cell::Cell::get), baseline + 1);
    assert!(element(&runtime, "block-body-0").rect[3] > height);
    runtime.state.source = "Fresh draft\nwith another line".into();
    runtime.state.streaming = true;
    runtime.invalidate();
    settle(&mut runtime);
    assert_eq!(BUILDS.with(std::cell::Cell::get), baseline + 2);
    assert_eq!(
        element(&runtime, "block-body-0").text.as_deref(),
        Some("Fresh draft\nwith another line")
    );
    assert!(runtime.find("answer-draft").is_some());
    assert!(
        runtime
            .elements()
            .iter()
            .filter(|element| !element.exiting)
            .all(|element| element.text.as_deref() != Some("First answer."))
    );
}

#[test]
fn styled_unicode_ranges_keep_original_strong_emphasis_code_and_quote_intents() {
    let document =
        answer_document("Café **strong** *soft* `snake_case` <b>literal</b>\n\n> quoted *text*");
    let (body, spans, _) = styled_block(&document.blocks[0], 16.0);
    let strong = spans
        .iter()
        .find(|span| span.weight == Some(600.0))
        .unwrap();
    assert_eq!(&body[strong.start as usize..strong.end as usize], "strong");
    let emphasis = spans.iter().find(|span| span.italic).unwrap();
    assert_eq!(
        &body[emphasis.start as usize..emphasis.end as usize],
        "soft"
    );
    let code = spans.iter().find(|span| span.mono).unwrap();
    assert_eq!(&body[code.start as usize..code.end as usize], "snake_case");
    assert_eq!(code.size, Some(16.0 * 0.88));
    assert!(body.contains("<b>literal</b>"));
    let (_, quote, _) = styled_block(&document.blocks[1], 16.0);
    assert!(quote.iter().all(|span| span.italic));
}

#[test]
fn actual_saved_prose_preview_and_detail_use_blocks_while_chart_alternatives_stay_literal() {
    let mut state = crate::controller::AppState::default();
    state.sample_result();
    state.result.as_mut().unwrap().text = "First\nsecond\n\n1. One\n7. Seven".into();
    let mut entry = state.current_answer_entry().unwrap();
    for preview in [true, false] {
        let mut runtime = runtime("", 480.0);
        runtime.state.saved = Some((entry.clone(), preview));
        runtime.invalidate();
        settle(&mut runtime);
        assert_eq!(
            element(&runtime, "block-body-0").text.as_deref(),
            Some("First\nsecond")
        );
        assert_eq!(
            element(&runtime, "block-marker-1").text.as_deref(),
            Some("1.")
        );
        assert_eq!(
            element(&runtime, "block-marker-2").text.as_deref(),
            Some("7.")
        );
        let clipboard = rune::clipboard::memory();
        runtime.use_clipboard(&clipboard);
        select(&mut runtime, "block-body-0", 0..1);
        command(&mut runtime, 'a');
        command(&mut runtime, 'c');
        assert_eq!(
            clipboard.text().unwrap().as_str(),
            "First\nsecond\n\n1. One\n7. Seven"
        );
        assert_eq!(
            super::super::saved_answer_plain_text(&entry),
            clipboard.text().unwrap().as_str()
        );
    }
    entry.chart = Some("flow_diagram".into());
    entry.answer = "Flow diagram:\n1. **Literal source** → 7) [kept](https://example.com)".into();
    let mut runtime = runtime("", 480.0);
    runtime.state.saved = Some((entry.clone(), false));
    runtime.invalidate();
    settle(&mut runtime);
    assert_eq!(
        element(&runtime, "answer").text.as_deref(),
        Some(entry.answer.as_str())
    );
    assert_eq!(super::super::saved_answer_plain_text(&entry), entry.answer);
    assert!(
        runtime
            .elements()
            .iter()
            .filter(|element| !element.exiting)
            .all(|element| !element
                .key
                .as_deref()
                .is_some_and(|key| key.starts_with("block-body-")))
    );
}
