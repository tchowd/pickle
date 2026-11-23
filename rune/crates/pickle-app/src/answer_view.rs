//! App-owned presentation of the original flat answer blocks.
//! Only literal rich text is emitted: no links, HTML, images or URL handlers.
use super::{GREEN, INK, MUTED, SURFACE};
use pickle_core::{AnswerBlock, AnswerBlockKind, AnswerDocument, answer_document};
use rune::{Align, El, Span, col, rich, row, text, tok::TextStyle};

#[cfg(test)]
thread_local! {
    static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn render(identity: String, source: &str, size: f32, streaming: bool) -> El {
    // Per-window Rune memo lifetime: unchanged answers do not parse on scroll,
    // and a changing draft replaces its old version rather than retaining chunks.
    let document = rune::memo(identity, (source, size.to_bits(), streaming), || {
        #[cfg(test)]
        BUILDS.with(|count| count.set(count.get() + 1));
        render_document(answer_document(source), size)
    });
    col()
        .key(if streaming { "answer-draft" } else { "answer" })
        .w_full()
        .child(document)
}

fn render_document(document: AnswerDocument, size: f32) -> El {
    let marker_style = TextStyle {
        features: "'tnum' 1",
        ..TextStyle::new(size, 600.0).lh(size * 1.55)
    };
    let number_column = document
        .blocks
        .iter()
        .filter_map(|block| match &block.kind {
            AnswerBlockKind::Numbered(marker) => Some(marker.as_str()),
            _ => None,
        })
        .map(|marker| {
            rune::text::measure(marker, &[], marker_style, None)
                .0
                .ceil()
                + size * 0.5
        })
        .fold(size * 1.5, f32::max);
    let expected_keys: Vec<_> = document
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| !block.text().is_empty())
        .map(|(index, _)| format!("block-body-{index}"))
        .collect();
    let plain = document.plain_text();
    let mut content = col().w_full().selection_group(true);
    for (index, block) in document.blocks.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|i| document.blocks.get(i));
        let next = document.blocks.get(index + 1);
        let before = u8::from(previous.is_some())
            * if previous.is_some_and(AnswerBlock::is_list_item) && block.is_list_item() {
                1
            } else {
                2
            };
        let after = if next.is_none() {
            0
        } else if block.is_list_item() && next.is_some_and(AnswerBlock::is_list_item) {
            1
        } else {
            2
        };
        let (value, spans, style) = styled_block(block, size);
        let selectable = !value.is_empty();
        let mut body = rich(value, spans)
            .key(format!("block-body-{index}"))
            .font(style)
            .color(if block.kind == AnswerBlockKind::Quote {
                MUTED
            } else {
                INK
            })
            .exact_lines()
            .wrap_anywhere()
            .w_full()
            .shrink()
            .selectable(selectable)
            .selection_color(GREEN.alpha(0.22))
            .copy_breaks(before, after);
        if let AnswerBlockKind::Heading(level) = block.kind {
            body = body.role("heading").level(u32::from(level));
        }
        let presented = if let Some(marker) = block.marker() {
            let prefix = format!("{}{marker} ", "  ".repeat(block.depth));
            body = body.copy_prefix(prefix);
            let column = if matches!(block.kind, AnswerBlockKind::Numbered(_)) {
                number_column
            } else {
                (size * 1.1).round()
            };
            row()
                .w_full()
                .items_start()
                .pl(block.depth as f32 * (size * 1.3).round())
                .child(
                    text(marker.to_owned())
                        .key(format!("block-marker-{index}"))
                        .font(marker_style)
                        .color(GREEN)
                        .align(Align::Start)
                        .w(column)
                        .min_w(column)
                        .a11y_hidden(),
                )
                .child(body)
        } else {
            match block.kind {
                AnswerBlockKind::Quote => {
                    col().w_full().pl((size * 1.3).round() * 0.75).child(body)
                }
                AnswerBlockKind::Code => col()
                    .w_full()
                    .padding(10.0)
                    .radius(6.0)
                    .fill(SURFACE)
                    .child(body.min_h(size * 0.88 * 1.2 + 3.0)),
                _ => body,
            }
        };
        let gap = if previous.is_none() {
            0.0
        } else if matches!(block.kind, AnswerBlockKind::Heading(_))
            || previous.is_some_and(|block| matches!(block.kind, AnswerBlockKind::Heading(_)))
            || (block.is_list_item() && previous.is_some_and(AnswerBlock::is_list_item))
        {
            (size * 0.4).round()
        } else {
            (size * 0.75).round()
        };
        content = content.child(
            col()
                .key(format!("block-{index}"))
                .w_full()
                .mt(gap)
                .child(presented),
        );
    }
    // Only a genuine whole-document selection uses the canonical writer.
    // Partial selections retain Rune's exact selected ranges and list prefixes.
    content.selection_copy(move |selection| {
        (!expected_keys.is_empty()
            && selection.texts().len() == expected_keys.len()
            && selection
                .texts()
                .iter()
                .zip(&expected_keys)
                .all(|(text, key)| text.key() == Some(key.as_str()) && text.is_whole()))
        .then(|| plain.clone())
    })
}

fn styled_block(block: &AnswerBlock, size: f32) -> (String, Vec<Span>, TextStyle) {
    let scale = match block.kind {
        AnswerBlockKind::Heading(1) => 1.25,
        AnswerBlockKind::Heading(2) => 1.15,
        AnswerBlockKind::Heading(_) => 1.05,
        AnswerBlockKind::Code => 0.88,
        _ => 1.0,
    };
    let heading = matches!(block.kind, AnswerBlockKind::Heading(_));
    let code_block = block.kind == AnswerBlockKind::Code;
    let style =
        TextStyle::new(size * scale, if heading { 700.0 } else { 400.0 }).lh(if code_block {
            size * scale * 1.2 + 3.0
        } else {
            size * scale * 1.55
        });
    let mut value = String::new();
    let mut spans = Vec::new();
    for run in &block.runs {
        let start = value.len();
        value.push_str(&run.text);
        if start == value.len() {
            continue;
        }
        let mut span = Span::new(start, value.len());
        if run.code {
            span = span.mono().size(size * 0.88).weight(400.0);
        } else if run.strong {
            span = span.weight(if heading { 700.0 } else { 600.0 });
        }
        if run.emphasis || block.kind == AnswerBlockKind::Quote {
            span = span.italic();
        }
        if run.code || run.strong || run.emphasis || block.kind == AnswerBlockKind::Quote {
            spans.push(span);
        }
    }
    (value, spans, style)
}

#[cfg(test)]
#[path = "answer_view_tests.rs"]
mod tests;
