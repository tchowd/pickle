use pickle_core::PickleError;
use regex::Regex;
use url::Url;

use crate::transport::error;

/// The key is deliberately omitted from Debug output. A loopback endpoint may be an SSH tunnel.
#[derive(Clone)]
pub struct LocalAIConfiguration {
    pub address: String,
    pub model: String,
    pub token: String,
    pub same_device: bool,
}

impl Default for LocalAIConfiguration {
    fn default() -> Self {
        Self {
            address: "http://127.0.0.1:11434".into(),
            model: "llama3.2:1b".into(),
            token: String::new(),
            same_device: false,
        }
    }
}

impl std::fmt::Debug for LocalAIConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalAIConfiguration")
            .field("model", &self.model)
            .field("same_device", &self.same_device)
            .finish_non_exhaustive()
    }
}

impl LocalAIConfiguration {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            ..Default::default()
        }
    }

    pub fn base_url(&self) -> Result<Url, PickleError> {
        let url = Url::parse(self.address.trim())
            .map_err(|_| error("Enter a valid Local server address."))?;
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        let loopback = Self::is_loopback(&host);
        if host.is_empty()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.path(), "" | "/")
            || host == "ollama.com"
            || host.ends_with(".ollama.com")
            || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || (self.same_device && !loopback)
        {
            return Err(error(
                "Use your trusted HTTPS server address, or localhost for a local server or SSH tunnel.",
            ));
        }
        Ok(url)
    }

    pub fn is_loopback(host: &str) -> bool {
        matches!(
            host.to_ascii_lowercase().as_str(),
            "localhost" | "127.0.0.1" | "::1" | "[::1]"
        )
    }

    pub fn validate(&self, strict_offline: bool) -> Result<(), PickleError> {
        self.base_url()?;
        let model = &self.model;
        if model.is_empty()
            || model.len() > 200
            || model.to_ascii_lowercase().contains("cloud")
            || !Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._:/-]*$")
                .unwrap()
                .is_match(model)
        {
            return Err(error(
                "Choose a downloaded local model. Cloud aliases are blocked in Local mode.",
            ));
        }
        if strict_offline && !self.same_device {
            return Err(error(
                "Offline mode blocks inference on another computer, including SSH tunnels. Choose a model on this device.",
            ));
        }
        Ok(())
    }
}
