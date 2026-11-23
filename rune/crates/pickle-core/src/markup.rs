//! Re-emit a conservative CommonMark subset before passing untrusted answers to Rune.
//! Render the result with Rune Markdown `autolink(false)`; no URL handler is needed.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Presentation-only Markdown: paragraphs, headings, lists, emphasis, quotes and plain code.
/// Links/images keep their label, HTML stays literal, and code languages are discarded.
/// Escaping every text event prevents a stripped link's label from becoming fresh markup.
pub fn safe_answer_markup(source: &str) -> String {
    let mut output = String::new();
    let mut lists: Vec<Option<(u64, char)>> = Vec::new();
    let mut quote_depth = 0usize;
    let mut code: Option<String> = None;
    // No optional extensions: tables, math, task controls, footnotes and strike-through
    // are not an implicit expansion of Pickle's presentation contract.
    for (event, range) in Parser::new_ext(source, Options::empty()).into_offset_iter() {
        if let Some(body) = code.as_mut() {
            match event {
                Event::End(TagEnd::CodeBlock) => {
                    let body = code.take().unwrap();
                    blank_line(&mut output);
                    let fence = "`".repeat(longest_backtick_run(&body).saturating_add(1).max(3));
                    output.push_str(&fence);
                    output.push('\n');
                    output.push_str(&body);
                    if !output.ends_with('\n') {
                        output.push('\n');
                    }
                    output.push_str(&fence);
                    output.push_str("\n\n");
                }
                Event::Text(text) | Event::Code(text) => body.push_str(&text),
                Event::SoftBreak | Event::HardBreak => body.push('\n'),
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    blank_line(&mut output);
                    output.push_str(&"#".repeat(level as usize));
                    output.push(' ');
                }
                Tag::BlockQuote(_) => {
                    blank_line(&mut output);
                    quote_depth += 1;
                    output.push_str(&"> ".repeat(quote_depth));
                }
                Tag::List(start) => {
                    if lists.is_empty() {
                        blank_line(&mut output);
                    }
                    // CommonMark distinguishes `1.` and `1)` lists. Emitting
                    // both as `.` merges adjacent lists on the next parse and
                    // can silently renumber an explicit later start (e.g. 10).
                    lists.push(start.map(|number| (number, ordered_delimiter(&source[range]))));
                }
                Tag::Item => {
                    line_break(&mut output);
                    output.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                    match lists.last_mut() {
                        Some(Some((number, delimiter))) => {
                            output.push_str(&format!("{number}{delimiter} "));
                            *number = number.saturating_add(1);
                        }
                        _ => output.push_str("- "),
                    }
                }
                Tag::Emphasis => output.push('*'),
                Tag::Strong => output.push_str("**"),
                Tag::CodeBlock(_) => code = Some(String::new()),
                // Destination/title/attributes are intentionally never copied.
                Tag::Link { .. } | Tag::Image { .. } | Tag::HtmlBlock => {}
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    if lists.is_empty() {
                        blank_line(&mut output);
                    } else {
                        line_break(&mut output);
                    }
                }
                TagEnd::Heading(_) => blank_line(&mut output),
                TagEnd::BlockQuote(_) => {
                    quote_depth = quote_depth.saturating_sub(1);
                    blank_line(&mut output);
                }
                TagEnd::List(_) => {
                    lists.pop();
                    if lists.is_empty() {
                        blank_line(&mut output);
                    }
                }
                TagEnd::Item => line_break(&mut output),
                TagEnd::Emphasis => output.push('*'),
                TagEnd::Strong => output.push_str("**"),
                _ => {}
            },
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                escape_text(&text, &mut output)
            }
            Event::Code(text) => {
                let fence = "`".repeat(longest_backtick_run(&text).saturating_add(1).max(1));
                let pad = text.starts_with('`')
                    || text.ends_with('`')
                    || (text.starts_with(' ') && text.ends_with(' '));
                output.push_str(&fence);
                if pad {
                    output.push(' ');
                }
                output.push_str(&text);
                if pad {
                    output.push(' ');
                }
                output.push_str(&fence);
            }
            Event::SoftBreak | Event::HardBreak => {
                output.push('\n');
                output.push_str(&"> ".repeat(quote_depth));
                output.push_str(&"  ".repeat(lists.len()));
            }
            Event::Rule => blank_line(&mut output),
            Event::FootnoteReference(text) => escape_text(&text, &mut output),
            Event::InlineMath(text) | Event::DisplayMath(text) => escape_text(&text, &mut output),
            Event::TaskListMarker(done) => {
                escape_text(if done { "[x] " } else { "[ ] " }, &mut output)
            }
        }
    }
    output.trim().to_owned()
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}
fn ordered_delimiter(list_source: &str) -> char {
    // The parser has already classified this exact source range as an ordered
    // list. Inspect only its ASCII marker; no content becomes new markup.
    list_source
        .trim_start_matches([' ', '\t'])
        .trim_start_matches(|ch: char| ch.is_ascii_digit())
        .chars()
        .next()
        .filter(|delimiter| *delimiter == '.' || *delimiter == ')')
        .unwrap_or('.')
}
fn blank_line(output: &mut String) {
    if !output.is_empty() {
        line_break(output);
        if !output.ends_with("\n\n") {
            output.push('\n');
        }
    }
}
fn line_break(output: &mut String) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
}
fn escape_text(text: &str, output: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '.'
            | '!' | '|' | '~' | '$' => {
                output.push('\\');
                output.push(ch);
            }
            _ => output.push(ch),
        }
    }
}

/// Plain readable copy text from the same supported presentation subset.
pub fn answer_plain_text(source: &str) -> String {
    let safe = safe_answer_markup(source);
    let mut output = String::new();
    let mut lists: Vec<Option<u64>> = vec![];
    let mut after_top_level_list = false;
    for event in Parser::new_ext(&safe, Options::empty()) {
        let adjacent_list = after_top_level_list;
        after_top_level_list = false;
        match event {
            Event::Start(Tag::List(start)) => {
                // Separate CommonMark lists retain independent numbering in
                // the renderer, while readable copy joins adjacent list items
                // with one newline, as for items within the same list.
                if lists.is_empty() && adjacent_list && output.ends_with("\n\n") {
                    output.pop();
                }
                lists.push(start);
            }
            Event::Start(Tag::Item) => {
                line_break(&mut output);
                output.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                match lists.last_mut() {
                    Some(Some(number)) => {
                        output.push_str(&format!("{number}. "));
                        *number = number.saturating_add(1);
                    }
                    _ => output.push_str(if lists.len() > 1 { "◦ " } else { "• " }),
                }
            }
            Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                output.push_str(&text)
            }
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::End(TagEnd::Paragraph) => {
                if lists.is_empty() {
                    blank_line(&mut output);
                } else {
                    line_break(&mut output);
                }
            }
            Event::End(TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::BlockQuote(_)) => {
                blank_line(&mut output)
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                if lists.is_empty() {
                    blank_line(&mut output);
                    after_top_level_list = true;
                }
            }
            Event::End(TagEnd::Item) => line_break(&mut output),
            _ => {}
        }
    }
    output.trim().to_owned()
}
