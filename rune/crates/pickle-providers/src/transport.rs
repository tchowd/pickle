use futures_util::StreamExt;
use pickle_core::PickleError;
use reqwest::{Client, Response};
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use url::Url;

pub(crate) const RESPONSE_BYTES: usize = 256_000;
pub(crate) const OUTPUT_BYTES: usize = 12_000;

pub(crate) fn error(message: impl Into<String>) -> PickleError {
    PickleError::message(message.into())
}

pub(crate) fn check_cancel(cancel: &CancellationToken) -> Result<(), PickleError> {
    if cancel.is_cancelled() {
        Err(PickleError::Cancelled)
    } else {
        Ok(())
    }
}

/// No redirects, shared cookie jar, response cache, retries, or raw server errors.
/// Timeout covers the response body as well as connection establishment.
pub(crate) async fn open(
    url: Url,
    body: Option<Value>,
    token: &str,
    timeout: Duration,
    local: bool,
    sse: bool,
    cancel: &CancellationToken,
) -> Result<Response, PickleError> {
    check_cancel(cancel)?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(timeout)
        .no_proxy()
        .build()
        .map_err(|_| error("Could not initialize the secure provider connection."))?;
    let mut request = if let Some(body) = body {
        client.post(url).json(&body)
    } else {
        client.get(url)
    };
    if !token.is_empty() {
        request = request.bearer_auth(token);
    }
    if sse {
        request = request.header(reqwest::header::ACCEPT, "text/event-stream");
    }
    let response = tokio::select! {
        _ = cancel.cancelled() => return Err(PickleError::Cancelled),
        result = request.send() => result.map_err(|_| connection_error(local))?,
    };
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        if status == 403 && !local {
            let data = bounded_body(response, 16_384, cancel)
                .await
                .unwrap_or_default();
            check_cancel(cancel)?;
            if serde_json::from_slice::<Value>(&data)
                .ok()
                .and_then(|v| v.get("errors")?.as_array().cloned())
                .is_some_and(|entries| entries.iter().any(|e| e["code"] == 5025))
            {
                return Err(error(
                    "This model does not support the structured output needed for diagrams.",
                ));
            }
        }
        check_cancel(cancel)?;
        return Err(status_error(status, local));
    }
    if sse
        && !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
            })
    {
        return Err(error(
            "This model did not return a text stream. Turn off streaming and retry.",
        ));
    }
    Ok(response)
}

pub(crate) fn connection_error(local: bool) -> PickleError {
    error(if local {
        "Can’t reach the Local server. Check that it is awake and connected, then retry."
    } else {
        "The provider connection failed. Check your connection and retry."
    })
}

pub(crate) fn status_error(status: u16, local: bool) -> PickleError {
    error(match status {
        401 | 403 if local => {
            "Local server access denied. Check its access key in Settings.".into()
        }
        401 | 403 => "Provider access denied. Check your API key and account permissions.".into(),
        402 => "Cloudflare requires billing setup for this model. Check your AI billing settings."
            .into(),
        404 if local => "The local model or API is unavailable. Choose an installed model.".into(),
        409 | 429 | 503 | 529 => {
            "The provider is busy or rate limited. Wait before retrying.".into()
        }
        _ => format!("Provider request failed (HTTP {status}). Try again later."),
    })
}

pub(crate) async fn bounded_body(
    response: Response,
    budget: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, PickleError> {
    if response
        .content_length()
        .is_some_and(|length| length > budget as u64)
    {
        return Err(error("Provider response exceeded the size limit."));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => return Err(PickleError::Cancelled),
            item = stream.next() => item,
        };
        let Some(chunk) = chunk else { break };
        let chunk =
            chunk.map_err(|_| error("The provider response was interrupted. Please retry."))?;
        if bytes.len().saturating_add(chunk.len()) > budget {
            return Err(error("Provider response exceeded the size limit."));
        }
        bytes.extend_from_slice(&chunk);
    }
    check_cancel(cancel)?;
    Ok(bytes)
}

pub(crate) async fn json_body(
    response: Response,
    cancel: &CancellationToken,
) -> Result<Value, PickleError> {
    let bytes = bounded_body(response, RESPONSE_BYTES, cancel).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| error("The provider returned an unreadable response."))
}

pub(crate) fn valid_text(text: &str) -> Result<(), PickleError> {
    if text.trim().is_empty() || text.len() > OUTPUT_BYTES {
        Err(error(
            "The provider returned an empty or oversized answer. Try a narrower question.",
        ))
    } else {
        Ok(())
    }
}
