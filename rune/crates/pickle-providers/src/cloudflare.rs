use async_trait::async_trait;
use pickle_core::{Generation, GenerationPrompt, GenerativeProvider, PickleError, PipelineEvent};
use regex::Regex;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

use crate::{
    streams::{Sse, read_lines},
    transport::{check_cancel, error, json_body, open, valid_text},
};

#[derive(Clone)]
pub struct CloudflareProvider {
    account_id: String,
    token: Zeroizing<String>,
    model: String,
    streaming: bool,
    base: Url,
}
impl CloudflareProvider {
    pub const DEFAULT_MODEL: &'static str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";
    pub const DIAGRAM_MODEL: &'static str = Self::DEFAULT_MODEL;

    pub fn new(
        account_id: impl Into<String>,
        token: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            token: Zeroizing::new(token.into()),
            model: model.into(),
            streaming: true,
            base: Url::parse("https://api.cloudflare.com/client/v4/").unwrap(),
        }
    }
    pub fn with_streaming(mut self, enabled: bool) -> Self {
        self.streaming = enabled;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_test_base(mut self, base: Url) -> Self {
        self.base = base;
        self
    }

    fn endpoint(&self, structured: bool) -> Result<(Url, &str), PickleError> {
        if !Regex::new(r"^[a-fA-F0-9]{32}$")
            .unwrap()
            .is_match(&self.account_id)
            || self.token.is_empty()
            || !Regex::new(r"^@cf/[a-zA-Z0-9_-]+/[a-zA-Z0-9_.-]+$")
                .unwrap()
                .is_match(&self.model)
        {
            return Err(error(
                "Configure a valid Cloudflare account ID, model, and API token in Settings.",
            ));
        }
        let model = if structured {
            Self::DIAGRAM_MODEL
        } else {
            &self.model
        };
        let url = self
            .base
            .join(&format!("accounts/{}/ai/run/{model}", self.account_id))
            .map_err(|_| error("Invalid provider configuration."))?;
        Ok((url, model))
    }
}

#[async_trait]
impl GenerativeProvider for CloudflareProvider {
    fn is_remote(&self) -> bool {
        true
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: UnboundedSender<PipelineEvent>,
        cancel: CancellationToken,
    ) -> Result<Generation, PickleError> {
        let (url, model) = self.endpoint(prompt.structured)?;
        let stream = self.streaming && !prompt.structured;
        let mut body = json!({"messages": [{"role":"system","content":prompt.system}, {"role":"user","content":prompt.user}], "max_tokens":1800,"temperature":0.2,"stream":stream});
        if prompt.structured {
            body["response_format"] = prompt
                .schema
                .map(|schema| json!({"type":"json_schema","json_schema":schema}))
                .unwrap_or_else(|| json!({"type":"json_object"}));
        }
        let response = open(
            url,
            Some(body),
            &self.token,
            Duration::from_secs(40),
            false,
            stream,
            &cancel,
        )
        .await?;
        if stream {
            let mut accumulator = Sse::default();
            read_lines(response, &cancel, |line| {
                if let Some(draft) = accumulator.line(line)? {
                    let _ = events.send(PipelineEvent::Draft(draft));
                }
                Ok(())
            })
            .await?;
            check_cancel(&cancel)?;
            return accumulator.finish(model);
        }
        let envelope = json_body(response, &cancel).await?;
        if envelope["success"] != true {
            return Err(error("Cloudflare could not generate a response."));
        }
        let result = &envelope["result"];
        let text = match result.get("response") {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Object(object)) if prompt.structured => {
                serde_json::to_string(object).map_err(|_| error("Invalid structured answer."))?
            }
            _ => return Err(error("Cloudflare returned an unsupported response format.")),
        };
        valid_text(&text)?;
        let usage = result
            .get("usage")
            .filter(|v| !v.is_null())
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| error("Cloudflare returned invalid usage metadata."))?;
        Ok(Generation {
            text,
            model: result["model"].as_str().unwrap_or(model).into(),
            usage,
            timing: None,
        })
    }
}
