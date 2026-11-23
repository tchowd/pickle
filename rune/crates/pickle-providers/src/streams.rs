use futures_util::StreamExt;
use pickle_core::{Generation, LocalGenerationTiming, PickleError, Usage};
use reqwest::Response;
use serde_json::Value;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::transport::{OUTPUT_BYTES, RESPONSE_BYTES, check_cancel, error, valid_text};

/// Byte framing is independent from UTF-8 decoding: a multi-byte code point may span chunks.
#[derive(Default)]
pub(crate) struct Lines {
    buffer: Vec<u8>,
    total: usize,
}
impl Lines {
    pub fn push(
        &mut self,
        bytes: &[u8],
        mut line: impl FnMut(&str) -> Result<(), PickleError>,
    ) -> Result<(), PickleError> {
        self.total = self.total.saturating_add(bytes.len());
        if self.total > RESPONSE_BYTES {
            return Err(error("Provider response exceeded the size limit."));
        }
        for &byte in bytes {
            if byte == b'\n' {
                let text = std::str::from_utf8(&self.buffer)
                    .map_err(|_| error("The provider stream contained invalid text."))?;
                line(text.trim_end_matches('\r'))?;
                self.buffer.clear();
            } else {
                if self.buffer.len() >= 65_536 {
                    return Err(error("A provider stream event exceeded the size limit."));
                }
                self.buffer.push(byte);
            }
        }
        Ok(())
    }
    pub fn finish(
        &mut self,
        mut line: impl FnMut(&str) -> Result<(), PickleError>,
    ) -> Result<(), PickleError> {
        if !self.buffer.is_empty() {
            let text = std::str::from_utf8(&self.buffer)
                .map_err(|_| error("The provider stream ended in invalid text."))?;
            line(text.trim_end_matches('\r'))?;
            self.buffer.clear();
        }
        Ok(())
    }
}

pub(crate) async fn read_lines(
    response: Response,
    cancel: &CancellationToken,
    mut line: impl FnMut(&str) -> Result<(), PickleError>,
) -> Result<(), PickleError> {
    let mut bytes = response.bytes_stream();
    let mut lines = Lines::default();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(PickleError::Cancelled),
            item = bytes.next() => item,
        };
        let Some(chunk) = chunk else { break };
        let chunk =
            chunk.map_err(|_| error("The provider stream was interrupted. Please retry."))?;
        lines.push(&chunk, |text| {
            check_cancel(cancel)?;
            line(text)
        })?;
    }
    check_cancel(cancel)?;
    lines.finish(line)
}

#[derive(Default)]
pub(crate) struct Sse {
    data: String,
    text: String,
    done: bool,
    model: Option<String>,
    usage: Option<Usage>,
}
impl Sse {
    /// Returns a cumulative draft only when a complete event supplies new prose.
    pub fn line(&mut self, line: &str) -> Result<Option<String>, PickleError> {
        if line.is_empty() {
            return self.dispatch();
        }
        if let Some(data) = line.strip_prefix("data:") {
            if self.done {
                return Err(error("Unexpected data after the answer finished."));
            }
            let data = data.strip_prefix(' ').unwrap_or(data);
            if !self.data.is_empty() {
                self.data.push('\n');
            }
            self.data.push_str(data);
        }
        Ok(None)
    }
    fn dispatch(&mut self) -> Result<Option<String>, PickleError> {
        let data = std::mem::take(&mut self.data);
        if data.is_empty() {
            return Ok(None);
        }
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(None);
        }
        let chunk: Value = serde_json::from_str(&data)
            .map_err(|_| error("The answer stream contained an invalid event. Please retry."))?;
        if !chunk.is_object()
            || chunk
                .get("response")
                .is_some_and(|value| !value.is_null() && !value.is_string())
            || chunk
                .get("model")
                .is_some_and(|value| !value.is_null() && !value.is_string())
            || chunk
                .get("choices")
                .is_some_and(|value| !value.is_null() && !value.is_array())
            || chunk
                .pointer("/choices/0/delta/content")
                .is_some_and(|value| !value.is_null() && !value.is_string())
        {
            return Err(error(
                "The answer stream contained an invalid event. Please retry.",
            ));
        }
        if chunk.get("error").is_some_and(|v| !v.is_null()) {
            return Err(error("The provider interrupted the answer. Please retry."));
        }
        if let Some(reason) = chunk
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
            && reason == "length"
        {
            return Err(error(
                "The answer reached its length limit. Ask a narrower question.",
            ));
        }
        let delta = chunk
            .get("response")
            .and_then(Value::as_str)
            .or_else(|| {
                chunk
                    .pointer("/choices/0/delta/content")
                    .and_then(Value::as_str)
            })
            .unwrap_or("");
        if self.text.len().saturating_add(delta.len()) > OUTPUT_BYTES {
            return Err(error("This answer is too long. Try a shorter passage."));
        }
        self.text.push_str(delta);
        if let Some(model) = chunk.get("model").and_then(Value::as_str) {
            self.model = Some(model.into());
        }
        if let Some(usage) = chunk.get("usage").filter(|v| !v.is_null()) {
            self.usage = Some(
                serde_json::from_value(usage.clone())
                    .map_err(|_| error("The provider returned invalid usage metadata."))?,
            );
        }
        Ok((!delta.is_empty()).then(|| self.text.clone()))
    }
    pub fn finish(mut self, fallback_model: &str) -> Result<Generation, PickleError> {
        self.dispatch()?;
        if !self.done {
            return Err(error(
                "The connection ended before the answer finished. Please retry.",
            ));
        }
        valid_text(&self.text)?;
        Ok(Generation {
            text: self.text,
            model: self.model.unwrap_or_else(|| fallback_model.into()),
            usage: self.usage,
            timing: None,
        })
    }
}

pub(crate) struct Ndjson {
    text: String,
    done: bool,
    final_chunk: Value,
    start: Instant,
    first_token_observable: bool,
    first_token: Option<f64>,
}
impl Default for Ndjson {
    fn default() -> Self {
        Self::new(Instant::now(), true)
    }
}
impl Ndjson {
    /// Buffered responses expose only completion, not when the first token was
    /// generated. The caller starts the clock before opening the chat request.
    pub fn new(start: Instant, first_token_observable: bool) -> Self {
        Self {
            text: String::new(),
            done: false,
            final_chunk: Value::Null,
            start,
            first_token_observable,
            first_token: None,
        }
    }
}
impl Ndjson {
    pub fn line(&mut self, line: &str) -> Result<Option<String>, PickleError> {
        if line.trim().is_empty() {
            return Ok(None);
        }
        if self.done {
            return Err(error("Unexpected data after the Local answer finished."));
        }
        let chunk: Value = serde_json::from_str(line)
            .map_err(|_| error("The Local server returned an invalid answer."))?;
        if chunk
            .pointer("/message/content")
            .is_some_and(|value| !value.is_string())
        {
            return Err(error("The Local server returned invalid answer text."));
        }
        if chunk.get("error").is_some_and(|v| !v.is_null()) {
            return Err(error("The Local server could not complete the answer."));
        }
        let done = chunk
            .get("done")
            .and_then(Value::as_bool)
            .ok_or_else(|| error("The Local server returned an invalid completion marker."))?;
        let delta = chunk
            .pointer("/message/content")
            .and_then(Value::as_str)
            .unwrap_or("");
        if self.text.len().saturating_add(delta.len()) > OUTPUT_BYTES {
            return Err(error(
                "The Local answer is too long. Ask a narrower question.",
            ));
        }
        self.text.push_str(delta);
        if self.first_token_observable && !delta.is_empty() && self.first_token.is_none() {
            self.first_token = Some(self.start.elapsed().as_secs_f64());
        }
        if done {
            if chunk["done_reason"] == "length" {
                return Err(error(
                    "The Local answer reached its length limit. Ask a narrower question.",
                ));
            }
            self.done = true;
            self.final_chunk = chunk.clone();
        }
        Ok((!delta.is_empty()).then(|| self.text.clone()))
    }
    pub fn finish(self, model: &str) -> Result<Generation, PickleError> {
        if !self.done {
            return Err(error(
                "The Local connection ended before the answer finished. Please retry.",
            ));
        }
        valid_text(&self.text)?;
        let seconds = |key| {
            self.final_chunk
                .get(key)
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                / 1_000_000_000.0
        };
        let output_tokens = self.final_chunk["eval_count"].as_u64();
        Ok(Generation {
            text: self.text,
            model: self.final_chunk["model"].as_str().unwrap_or(model).into(),
            usage: Some(Usage {
                input_tokens: self.final_chunk["prompt_eval_count"].as_u64(),
                output_tokens,
            }),
            timing: Some(LocalGenerationTiming {
                load_seconds: seconds("load_duration"),
                generation_seconds: seconds("eval_duration"),
                first_token_seconds: self.first_token,
                total_seconds: self.start.elapsed().as_secs_f64(),
                output_tokens: output_tokens.unwrap_or(0),
            }),
        })
    }
}
