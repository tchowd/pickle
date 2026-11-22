use pickle_core::{GenerationPrompt, PickleError, ReadingAction};
use std::collections::BTreeMap;

use crate::transport::error;

pub(crate) const LOCAL_CONTEXT: usize = 8192;
pub(crate) const LOCAL_OUTPUT: usize = 1024;
pub(crate) const LOCAL_PROMPT_BYTES: usize = LOCAL_CONTEXT - LOCAL_OUTPUT - 512;

fn encoded(fields: &BTreeMap<&str, String>) -> String {
    serde_json::to_string(fields).expect("string map serialization")
}

pub(crate) fn truncate_utf8(value: &str, budget: usize) -> &str {
    let mut end = budget.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(crate) fn fit(mut prompt: GenerationPrompt) -> Result<GenerationPrompt, PickleError> {
    let schema_bytes = prompt
        .schema
        .as_ref()
        .map(|v| v.to_string().len())
        .unwrap_or(0);
    let fixed = prompt.system.len().saturating_add(schema_bytes);
    let Some(input) = prompt.local_input.take() else {
        if fixed.saturating_add(prompt.user.len()) > LOCAL_PROMPT_BYTES {
            return Err(error(
                "This passage is too long for Local mode. Select a shorter passage.",
            ));
        }
        return Ok(prompt);
    };
    let mut fields = BTreeMap::from([
        ("source", input.source()),
        ("follow_up_question", input.question.clone()),
    ]);
    if fixed.saturating_add(encoded(&fields).len()) > LOCAL_PROMPT_BYTES {
        return Err(error(
            "Your passage, added context, or question is too long for this local model. Shorten them; they won’t be cut silently.",
        ));
    }
    let mut has_source = !input.snapshot.text.trim().is_empty() || !input.context.trim().is_empty();
    let mut add = |name: &'static str, value: &str, cap: usize| -> bool {
        let used = fixed
            .saturating_add(encoded(&fields).len())
            .saturating_add(name.len())
            .saturating_add(100);
        let room = LOCAL_PROMPT_BYTES.saturating_sub(used).min(cap);
        if room == 0 || value.trim().is_empty() {
            return false;
        }
        let mut excerpt = truncate_utf8(value, room).to_owned();
        if excerpt.len() < value.len() {
            let note = "\n[Excerpt; remaining context omitted.]";
            excerpt = format!(
                "{}{}",
                truncate_utf8(&excerpt, room.saturating_sub(note.len())),
                note
            );
        }
        fields.insert(name, excerpt);
        while fixed.saturating_add(encoded(&fields).len()) > LOCAL_PROMPT_BYTES {
            let text = fields.get_mut(name).unwrap();
            if text.pop().is_none() {
                fields.remove(name);
                return false;
            }
        }
        fields
            .get(name)
            .is_some_and(|value| !value.trim().is_empty())
    };
    if input.action != ReadingAction::Chart {
        add(
            "prior_explanation_not_evidence",
            &input.previous_result,
            1000,
        );
        if let Some(reference) = &input.reference {
            let excerpt =
                pickle_core::WebReference::excerpt(&reference.text, &input.snapshot.text, 2600);
            has_source |= add("reference_excerpt", &excerpt, 2600);
        }
        has_source |= add("page_OCR_may_contain_errors", &input.page_context, 1400);
        add("visual_summary_not_evidence", &input.visual_context, 500);
        let recent = input
            .conversation
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|turn| format!("Q: {}\nA (not evidence): {}", turn.question, turn.answer))
            .collect::<Vec<_>>()
            .join("\n");
        add("recent_conversation_not_evidence", &recent, 800);
    }
    if !has_source {
        return Err(error(
            "No page text fits this request. Select a passage or add a shorter excerpt.",
        ));
    }
    prompt.user = encoded(&fields);
    Ok(prompt)
}
