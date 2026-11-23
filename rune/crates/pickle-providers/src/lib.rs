//! Bounded provider clients. Constructors never contact a server; live requests are explicit.
mod article;
pub mod browser;
mod cloudflare;
mod config;
mod jev;
mod local_budget;
mod ollama;
mod sample;
mod streams;
mod transport;
mod vision;

pub use article::{extract_article_html, fetch_public_reference, public_article_url};
pub use cloudflare::CloudflareProvider;
pub use config::LocalAIConfiguration;
pub use jev::JevClient;
pub use ollama::OllamaProvider;
pub use sample::SampleProvider;
pub use vision::VisualContextClient;

#[cfg(test)]
mod legacy_local_https_tests;
#[cfg(test)]
mod tests;
