use async_trait::async_trait;
use pickle_core::{DecisionClient, DecisionResponse, DecisionState, PickleError, Questions};
use regex::Regex;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

use crate::transport::{error, json_body, open};

#[derive(Clone)]
pub struct JevClient {
    account_id: String,
    token: Zeroizing<String>,
    base: Url,
}
impl JevClient {
    pub const DEFAULT_MODEL: &'static str = "typesafe/jev";
    pub fn new(account_id: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            token: Zeroizing::new(token.into()),
            base: Url::parse("https://api.cloudflare.com/client/v4/").unwrap(),
        }
    }
    #[cfg(test)]
    pub(crate) fn with_test_base(mut self, base: Url) -> Self {
        self.base = base;
        self
    }

    pub fn decode(value: Value) -> Result<DecisionResponse, PickleError> {
        let result = if value.get("success").is_some() {
            if value["success"] != true {
                return Err(error(
                    "Cloudflare could not evaluate this passage with Jev.",
                ));
            }
            let result = value
                .get("result")
                .ok_or_else(|| error("Jev returned no result."))?;
            if let Some(state) = result.get("state") {
                if state != "Completed" {
                    return Err(error(
                        "Cloudflare has not completed the Jev evaluation. Try again shortly.",
                    ));
                }
                result
                    .get("result")
                    .cloned()
                    .ok_or_else(|| error("Jev completed without an answer."))?
            } else {
                result.clone()
            }
        } else {
            value
        };
        serde_json::from_value(result).map_err(|_| error("Jev returned an unreadable evaluation."))
    }
}

#[async_trait]
impl DecisionClient for JevClient {
    async fn evaluate(
        &self,
        state: DecisionState,
        questions: Questions,
        cancel: CancellationToken,
    ) -> Result<DecisionResponse, PickleError> {
        if self.token.is_empty()
            || !Regex::new(r"^[a-fA-F0-9]{32}$")
                .unwrap()
                .is_match(&self.account_id)
        {
            return Err(error(
                "Configure Cloudflare credentials for Jev in Settings.",
            ));
        }
        let url = self
            .base
            .join(&format!("accounts/{}/ai/run", self.account_id))
            .map_err(|_| error("Invalid Jev configuration."))?;
        let response = open(
            url,
            Some(
                json!({"model":Self::DEFAULT_MODEL,"input":{"state":state,"questions":questions}}),
            ),
            &self.token,
            Duration::from_secs(40),
            false,
            false,
            &cancel,
        )
        .await?;
        let response = Self::decode(json_body(response, &cancel).await?)?;
        response.validate(&questions)?;
        Ok(response)
    }
}
