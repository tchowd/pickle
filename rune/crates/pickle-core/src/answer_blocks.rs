//! Pickle's flat, presentation-only answer format. Structure follows the legacy
//! reader; inline parsing never creates active links, HTML or image resources.
use markdown_it::parser::{
    block::builtin::BlockParserRule,
    core::{CoreRule, Root},
    inline::{InlineRoot, InlineRule, InlineState, Text, TextSpecial, builtin::InlineParserRule},
    linkfmt::LinkFormatter,
};
use markdown_it::plugins::{
    cmark::inline::{
        backticks::CodeInline,
        emphasis::{Em, Strong},
        image::Image,
        newline::{Hardbreak, Softbreak},
    },
    html::html_inline::HtmlInline,
};
use markdown_it::{MarkdownIt, Node, NodeValue};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnswerDocument {
    pub blocks: Vec<AnswerBlock>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerBlock {
    pub kind: AnswerBlockKind,
    pub depth: usize,
    pub runs: Vec<AnswerRun>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnswerBlockKind {
    Paragraph,
    Heading(u8),
    Bullet,
    Numbered(String),
    Quote,
    Code,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnswerRun {
    pub text: String,
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
}
impl AnswerBlock {
    pub fn marker(&self) -> Option<&str> {
        match &self.kind {
            AnswerBlockKind::Bullet => Some(if self.depth == 0 { "•" } else { "◦" }),
            AnswerBlockKind::Numbered(number) => Some(number),
            _ => None,
        }
    }
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
    pub fn is_list_item(&self) -> bool {
        self.marker().is_some()
    }
}
impl AnswerDocument {
    /// One canonical readable copy representation, also used for matching terms.
    pub fn plain_text(&self) -> String {
        let mut result = String::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if index != 0 {
                result.push_str(
                    if block.is_list_item() && self.blocks[index - 1].is_list_item() {
                        "\n"
                    } else {
                        "\n\n"
                    },
                );
            }
            if let Some(marker) = block.marker() {
                result.push_str(&"  ".repeat(block.depth));
                result.push_str(marker);
                result.push(' ');
            }
            for run in &block.runs {
                result.push_str(&run.text);
            }
        }
        result
    }
}

/// Parse untrusted answer text without enabling block HTML, active URLs or any
/// optional rich feature. This owns no global or thread-local content cache.
pub fn answer_document(source: &str) -> AnswerDocument {
    let parser = inline_parser();
    let mut document = AnswerDocument::default();
    let mut lines = Vec::<String>::new();
    let mut kind = AnswerBlockKind::Paragraph;
    let mut depth = 0;
    let mut fence: Option<Vec<String>> = None;
    let mut indents = Vec::<usize>::new();
    for raw in source.replace("\r\n", "\n").split('\n') {
        let line = trim_spaces(raw);
        if let Some(code) = fence.as_mut() {
            if line.starts_with("```") {
                close_fence(&mut document, &mut fence);
            } else {
                code.push(raw.into());
            }
            continue;
        }
        if line.starts_with("```") {
            flush(&parser, &mut document, &mut lines, &kind, depth);
            indents.clear();
            fence = Some(vec![]);
            continue;
        }
        if line.is_empty() {
            flush(&parser, &mut document, &mut lines, &kind, depth);
            continue;
        }
        let indent = raw
            .chars()
            .take_while(|ch| matches!(ch, ' ' | '\t'))
            .map(|ch| if ch == '\t' { 4 } else { 1 })
            .sum();
        if let Some((level, title)) = heading(line) {
            flush(&parser, &mut document, &mut lines, &kind, depth);
            indents.clear();
            document.blocks.push(AnswerBlock {
                kind: AnswerBlockKind::Heading(level),
                depth: 0,
                runs: inline_runs(&parser, title),
            });
        } else if is_rule(line) {
            flush(&parser, &mut document, &mut lines, &kind, depth);
            indents.clear();
        } else if let Some((item, content)) = list_item(line) {
            flush(&parser, &mut document, &mut lines, &kind, depth);
            while indents.last().is_some_and(|last| *last > indent) {
                indents.pop();
            }
            if indents.last() != Some(&indent) {
                indents.push(indent);
            }
            kind = item;
            depth = (indents.len() - 1).min(3);
            lines.push(content.into());
        } else if let Some(quote) = line.strip_prefix('>') {
            if kind != AnswerBlockKind::Quote {
                flush(&parser, &mut document, &mut lines, &kind, depth);
                indents.clear();
            }
            kind = AnswerBlockKind::Quote;
            depth = 0;
            lines.push(trim_spaces(quote).into());
        } else if !lines.is_empty()
            && (kind == AnswerBlockKind::Paragraph
                || (indent > 0 && kind != AnswerBlockKind::Quote))
        {
            lines.push(line.into());
        } else {
            flush(&parser, &mut document, &mut lines, &kind, depth);
            indents.clear();
            kind = AnswerBlockKind::Paragraph;
            depth = 0;
            lines.push(line.into());
        }
    }
    close_fence(&mut document, &mut fence);
    flush(&parser, &mut document, &mut lines, &kind, depth);
    document
}
fn flush(
    parser: &MarkdownIt,
    document: &mut AnswerDocument,
    lines: &mut Vec<String>,
    kind: &AnswerBlockKind,
    depth: usize,
) {
    if lines.is_empty() {
        return;
    }
    document.blocks.push(AnswerBlock {
        kind: kind.clone(),
        depth,
        runs: inline_runs(parser, &lines.join("\n")),
    });
    lines.clear();
}
fn close_fence(document: &mut AnswerDocument, fence: &mut Option<Vec<String>>) {
    if let Some(lines) = fence.take() {
        document.blocks.push(AnswerBlock {
            kind: AnswerBlockKind::Code,
            depth: 0,
            runs: vec![AnswerRun {
                text: lines.join("\n"),
                code: true,
                ..AnswerRun::default()
            }],
        });
    }
}
fn trim_spaces(text: &str) -> &str {
    // Foundation CharacterSet.whitespaces, not whitespacesAndNewlines. It also
    // includes ZERO WIDTH SPACE; LF/CR/NEL/line/paragraph separators stay literal.
    text.trim_matches(|ch| {
        matches!(
            ch,
            '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200b}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
        )
    })
}
fn heading(line: &str) -> Option<(u8, &str)> {
    let count = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&count) || line.as_bytes().get(count) != Some(&b' ') {
        return None;
    }
    let title = trim_spaces(trim_spaces(&line[count..]).trim_end_matches('#'));
    (!title.is_empty()).then_some((count as u8, title))
}
fn is_rule(line: &str) -> bool {
    let mut marks = line.chars().filter(|ch| *ch != ' ');
    let Some(first) = marks.next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_') && marks.clone().count() >= 2 && marks.all(|ch| ch == first)
}
fn list_item(line: &str) -> Option<(AnswerBlockKind, &str)> {
    let first = line.chars().next()?;
    if "-*+•–".contains(first) && line[first.len_utf8()..].starts_with(' ') {
        return Some((
            AnswerBlockKind::Bullet,
            trim_spaces(&line[first.len_utf8() + 1..]),
        ));
    }
    let count = line.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=3).contains(&count)
        || !matches!(line.as_bytes().get(count), Some(b'.' | b')'))
        || line.as_bytes().get(count + 1) != Some(&b' ')
    {
        return None;
    }
    Some((
        AnswerBlockKind::Numbered(format!("{}.", &line[..count])),
        trim_spaces(&line[count + 2..]),
    ))
}

struct SeedInline;
impl CoreRule for SeedInline {
    fn run(root: &mut Node, _: &MarkdownIt) {
        let content = root.cast::<Root>().expect("Markdown root").content.clone();
        root.children
            .push(Node::new(InlineRoot::new(content, vec![(0, 0)])));
    }
}
#[derive(Debug)]
struct LabelsOnly;
impl LinkFormatter for LabelsOnly {
    // Only syntax recognition is retained: no URL or title enters AnswerRun.
    // Rendering/activation of this intermediate Markdown AST is never called.
    fn validate_link(&self, _: &str) -> Option<()> {
        Some(())
    }
    fn normalize_link(&self, value: &str) -> String {
        value.into()
    }
    fn normalize_link_text(&self, value: &str) -> String {
        value.into()
    }
}
#[derive(Debug)]
struct IgnoredStrike;
impl NodeValue for IgnoredStrike {}
#[derive(Debug)]
struct StrikeMarker {
    length: usize,
    open: bool,
}
impl NodeValue for StrikeMarker {}
#[derive(Debug, Default)]
struct StrikeOpeners([bool; 2]);
impl markdown_it::parser::extset::NodeExt for StrikeOpeners {}
struct StrikeScanner;
impl InlineRule for StrikeScanner {
    const MARKER: char = '~';
    fn run(state: &mut InlineState) -> Option<(Node, usize)> {
        if !state.src[state.pos..state.pos_max].starts_with('~') {
            return None;
        }
        let scan = state.scan_delims(state.pos, true);
        let length = scan.length;
        // Foundation supports matching single/double runs, never splitting an
        // unequal or 3+-character run. Strike intent is deliberately not styled.
        if length > 2 {
            return Some((
                Node::new(Text {
                    content: "~".repeat(length),
                }),
                length,
            ));
        }
        // An unsuccessful search is remembered for each run length. Repeated
        // closing-only tildes must not rescan all previous literal text. Other
        // inline rules may move an opener into a child; one failed lookup then
        // clears this hint. A newly admitted opener always re-arms it.
        let available = state.node.ext.get_or_insert_default::<StrikeOpeners>().0[length - 1];
        if scan.can_close && available {
            let opener = state
                .node
                .children
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, node)| {
                    node.cast::<StrikeMarker>()
                        .filter(|marker| marker.open && marker.length == length)
                        .map(|_| index)
                });
            if let Some(index) = opener {
                let children = state.node.children.split_off(index + 1);
                let previous = state.node.children.pop().expect("Strike opener");
                let start = previous.srcmap.expect("Opener map").get_byte_offsets().0;
                let end = state
                    .get_map(state.pos, state.pos + length)
                    .expect("Closer map")
                    .get_byte_offsets()
                    .1;
                let mut result = Node::new(IgnoredStrike);
                result.children = children;
                result.srcmap = Some(markdown_it::common::sourcemap::SourcePos::new(start, end));
                let consumed = end - start;
                state.pos = state.pos + length - consumed;
                return Some((result, consumed));
            }
            state.node.ext.get_or_insert_default::<StrikeOpeners>().0[length - 1] = false;
        }
        if !scan.can_open {
            return Some((
                Node::new(Text {
                    content: "~".repeat(length),
                }),
                length,
            ));
        }
        state.node.ext.get_or_insert_default::<StrikeOpeners>().0[length - 1] = true;
        Some((
            Node::new(StrikeMarker {
                length,
                open: scan.can_open,
            }),
            length,
        ))
    }
}
struct CleanStrike;
impl CoreRule for CleanStrike {
    fn run(root: &mut Node, _: &MarkdownIt) {
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if let Some(marker) = node.cast::<StrikeMarker>() {
                node.replace(Text {
                    content: "~".repeat(marker.length),
                });
            }
            pending.extend(node.children.iter_mut());
        }
    }
}
fn inline_parser() -> MarkdownIt {
    let mut parser = MarkdownIt::new();
    markdown_it::plugins::cmark::add(&mut parser);
    markdown_it::plugins::html::html_inline::add(&mut parser);
    parser.link_formatter = Box::new(LabelsOnly);
    parser.inline.add_rule::<StrikeScanner>();
    parser
        .add_rule::<CleanStrike>()
        .after::<InlineParserRule>()
        .before::<markdown_it::generics::inline::emph_pair::FragmentsJoin>();
    parser.remove_rule::<BlockParserRule>();
    parser.add_rule::<SeedInline>().before::<InlineParserRule>();
    parser
}
#[derive(Clone, Copy, Default)]
struct Style {
    strong: bool,
    emphasis: bool,
    code: bool,
}
fn append(runs: &mut Vec<AnswerRun>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.strong == style.strong
        && last.emphasis == style.emphasis
        && last.code == style.code
    {
        last.text.push_str(text);
        return;
    }
    runs.push(AnswerRun {
        text: text.into(),
        strong: style.strong,
        emphasis: style.emphasis,
        code: style.code,
    });
}
fn inline_runs(parser: &MarkdownIt, body: &str) -> Vec<AnswerRun> {
    // Match Foundation/CommonMark's replacement of NUL in interpreted inline
    // content. Fenced code deliberately remains the literal source instead.
    let normalized = body.replace('\0', "\u{fffd}");
    let tree = parser.parse(&normalized);
    let mut pending = vec![(tree, Style::default(), false)];
    let mut runs = vec![];
    while let Some((mut node, mut style, mut image)) = pending.pop() {
        if node.is::<Image>() {
            image = true;
        }
        if !image {
            style.strong |= node.is::<Strong>();
            style.emphasis |= node.is::<Em>();
            style.code |= node.is::<CodeInline>();
        }
        if let Some(text) = node.cast::<Text>() {
            append(&mut runs, &text.content, style);
        } else if let Some(text) = node.cast::<TextSpecial>() {
            append(&mut runs, &text.content, style);
        } else if let Some(html) = node.cast::<HtmlInline>() {
            append(&mut runs, &html.content, style);
        } else if node.is::<Hardbreak>() || node.is::<Softbreak>() {
            append(&mut runs, "\n", style);
        }
        // Consume all child vectors before dropping their parents: a legal
        // deeply nested inline answer must not recurse during Node destruction.
        pending.extend(
            std::mem::take(&mut node.children)
                .into_iter()
                .rev()
                .map(|child| (child, style, image)),
        );
    }
    runs
}
