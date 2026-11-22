use pickle_core::{answer_plain_text, safe_answer_markup};
use pulldown_cmark::{Event, Options, Parser, Tag};

fn assert_inert(source: &str) -> String {
    let safe = safe_answer_markup(source);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH;
    for event in Parser::new_ext(&safe, options) {
        assert!(
            !matches!(
                event,
                Event::Start(
                    Tag::Link { .. }
                        | Tag::Image { .. }
                        | Tag::Table(_)
                        | Tag::FootnoteDefinition(_)
                        | Tag::Strikethrough
                ) | Event::Html(_)
                    | Event::InlineHtml(_)
                    | Event::InlineMath(_)
                    | Event::DisplayMath(_)
                    | Event::TaskListMarker(_)
            ),
            "Unsafe event: {event:?} in {safe}"
        );
    }
    safe
}
#[test]
fn keeps_light_formatting_and_numbered_lists() {
    let safe = assert_inert(
        "## Overview\n\n1. **Model inference**: an explanation.\n2. *Batching* uses `snake_case`.\n\n- Parent\n  - Child",
    );
    assert!(safe.contains("## Overview"));
    assert!(safe.contains("**Model inference**"));
    assert!(safe.contains("*Batching*"));
    assert!(safe.contains("`snake_case`"));
    assert!(safe.contains("1. "));
    assert!(safe.contains("2. "));
    assert!(safe.contains("  - Child"));
}
#[test]
fn strips_destinations_but_preserves_labels_and_image_alt_text() {
    let safe = assert_inert(
        "See [docs](https://example.com/private \"title\") and ![A diagram](https://tracker.example/image.png). [reference][r]\n\n[r]: https://secret.example",
    );
    assert!(safe.contains("docs"));
    assert!(safe.contains("A diagram"));
    assert!(safe.contains("reference"));
    assert!(!safe.contains("example"));
    assert!(!safe.contains("tracker"));
}
#[test]
fn link_labels_cannot_become_new_markup() {
    let safe = assert_inert("[\\[click\\]\\(javascript:alert\\(1\\)\\)](https://example.com)");
    assert!(safe.contains("click"));
}
#[test]
fn html_is_literal_even_when_attributes_contain_urls() {
    let input =
        "Keep <b>tags</b> and <img src=\"https://example.com/x\">.\n\n<script>alert(1)</script>";
    let safe = assert_inert(input);
    assert!(safe.contains("&lt;b&gt;"));
    assert!(answer_plain_text(input).contains("<b>tags</b>"));
}
#[test]
fn tables_tasklists_math_and_strikethrough_stay_plain() {
    let safe = assert_inert(
        "| A | B |\n| --- | --- |\n| 1 | 2 |\n\n- [ ] unchecked\n\n~~strike~~ and $x+y$ and $$z$$",
    );
    assert!(safe.contains("\\|"));
    assert!(safe.contains("\\[ \\]"));
    assert!(safe.contains("\\~"));
    assert!(safe.contains("\\$"));
}
#[test]
fn code_languages_never_enable_diagram_rendering() {
    let safe = assert_inert(
        "```mermaid\ngraph LR; A-->B\n```\n\n```html\n<img src='https://example.com'>\n```",
    );
    assert!(!safe.contains("```mermaid"));
    assert!(!safe.contains("```html"));
    assert!(safe.contains("graph LR; A-->B"));
}
#[test]
fn streaming_unclosed_fences_and_unicode_are_safe() {
    let safe = assert_inert("```mermaid\n🫛 flow --> target\n");
    assert!(safe.ends_with("```"));
    assert!(answer_plain_text("🌍 **café** and `🫛`").contains("🌍 café and 🫛"));
}
#[test]
fn literal_entities_and_punctuation_keep_their_reading_meaning() {
    let plain = answer_plain_text("-5 degrees, #hashtag, 2024. A year\n2 * 3 = 6 &amp; 1 < 5");
    assert!(plain.contains("-5 degrees, #hashtag, 2024. A year"));
    assert!(plain.contains("2 * 3 = 6 & 1 < 5"));
}
#[test]
fn sanitizer_is_safe_under_repeated_application() {
    let source = "**Bold** [label](https://example.com) <b>literal</b>\n\n- A\n- B";
    let once = assert_inert(source);
    let twice = assert_inert(&once);
    assert_eq!(answer_plain_text(&once), answer_plain_text(&twice));
}
#[test]
fn copy_text_preserves_list_markers_without_formatting_syntax() {
    let plain = answer_plain_text(
        "The components:\n\n1. **Model inference**\n2. GPU batching\n\n- Parent\n  - Child",
    );
    assert!(plain.contains("1. Model inference\n2. GPU batching"));
    assert!(plain.contains("• Parent\n  ◦ Child"));
    assert!(!plain.contains("**"));
}
