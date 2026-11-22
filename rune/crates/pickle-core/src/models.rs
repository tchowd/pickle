use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{ChartKind, ValidatedChart};

pub mod limits {
    pub const SELECTION: usize = 12_000;
    pub const CONTEXT: usize = 6_000;
    pub const QUESTION: usize = 2_000;
    pub const OUTPUT: usize = 12_000;
    pub const CONVERSATION: usize = 16_000;
    pub const TURNS: usize = 6;
    pub const RESPONSE: usize = 256_000;
    pub const PAGE_CONTEXT: usize = 6_000;
    pub const VISUAL_CONTEXT: usize = 3_000;
    pub const REFERENCE: usize = 12_000;
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum PickleError {
    #[error("{0}")]
    Message(String),
    #[error("The request was cancelled.")]
    Cancelled,
}
impl PickleError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}
pub type Result<T> = std::result::Result<T, PickleError>;
pub fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(PickleError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ReadingAction {
    #[default]
    Simplify,
    Expand,
    Chart,
    FollowUp,
}
impl ReadingAction {
    pub fn title(self) -> &'static str {
        match self {
            Self::Simplify => "Simplify",
            Self::Expand => "Explain more",
            Self::Chart => "Visualize",
            Self::FollowUp => "Follow-up",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ReadingLevel {
    #[default]
    Automatic,
    Plain,
    Balanced,
    Technical,
}
impl ReadingLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Plain => "Plain language",
            Self::Balanced => "Balanced",
            Self::Technical => "Keep technical detail",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectionBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectionSnapshot {
    pub id: Uuid,
    pub text: String,
    pub app_name: String,
    pub app_id: String,
    pub captured_at: DateTime<Utc>,
    pub bounds: Option<SelectionBounds>,
    pub method: String,
    pub limitations: Vec<String>,
    pub source_title: Option<String>,
    pub source_url: Option<String>,
}
impl SelectionSnapshot {
    pub fn new(
        text: impl Into<String>,
        app_name: impl Into<String>,
        app_id: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            text: text.into(),
            app_name: app_name.into(),
            app_id: app_id.into(),
            captured_at: Utc::now(),
            bounds: None,
            method: "Manual paste".into(),
            limitations: vec![],
            source_title: None,
            source_url: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QualityStatus {
    Checked,
    Concerns(Vec<String>),
    Uncertain,
    NotChecked(String),
    ChartValidated,
}
impl QualityStatus {
    pub const CHECKS_OFF: &'static str = "Jev disabled";
    pub const OFFLINE: &'static str = "local-only mode";
    pub fn label(&self) -> String {
        match self {
            Self::Checked => "Checked against your selection".into(),
            Self::Concerns(_) => "Check found concerns — review the source".into(),
            Self::Uncertain => "Check inconclusive — review the source".into(),
            Self::NotChecked(reason) => format!("Semantic checks not performed · {reason}"),
            Self::ChartValidated => "Chart structure and source evidence checked".into(),
        }
    }
    pub fn visible_label(&self) -> Option<String> {
        match self {
            Self::NotChecked(reason) if reason == Self::CHECKS_OFF || reason == Self::OFFLINE => {
                None
            }
            _ => Some(self.label()),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub question: String,
    pub answer: String,
    pub quality: QualityStatus,
    pub label: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebReference {
    pub url: String,
    pub title: String,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub timestamp: Option<f64>,
}
impl WebReference {
    pub fn validate(&self) -> Result<()> {
        if Self::public_url(&self.url).is_none()
            || self.title.len() > 1000
            || self.text.trim().is_empty()
            || self.text.len() > limits::REFERENCE
            || self
                .timestamp
                .is_some_and(|t| !t.is_finite() || !(0.0..=31_536_000.0).contains(&t))
            || ![
                "article",
                "video captions",
                "video transcript",
                "recorded audio",
            ]
            .contains(&self.kind.as_str())
        {
            return Err(PickleError::message("Invalid or oversized reference."));
        }
        Ok(())
    }
    /// Syntactic URL gate only. Fetchers must additionally validate and pin DNS addresses.
    pub fn public_url(raw: &str) -> Option<url::Url> {
        if raw.len() > 4096 {
            return None;
        }
        let mut url = url::Url::parse(raw).ok()?;
        let host = url.host_str()?.trim_end_matches('.').to_lowercase();
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || !host.contains('.')
            || !host.chars().any(char::is_alphabetic)
            || host.ends_with(".local")
            || host.ends_with(".localhost")
            || host.ends_with(".internal")
            || host.contains(':')
            || url.port().is_some_and(|p| p != 80 && p != 443)
            || matches!(
                url.host(),
                Some(url::Host::Ipv4(_)) | Some(url::Host::Ipv6(_))
            )
        {
            return None;
        }
        url.set_fragment(None);
        Some(url)
    }
    pub fn source(&self) -> String {
        format!(
            "Additional {} reference (untrusted source text):\nTitle: {}\nURL: {}{}\n{}",
            self.kind,
            self.title,
            self.url,
            self.timestamp
                .map(|time| format!("\nPlayback time: {} seconds", time as u64))
                .unwrap_or_default(),
            self.text
        )
    }
    /// Rank paragraphs by selection overlap, then keep their original reading order.
    pub fn excerpt(text: &str, selection: &str, budget: usize) -> String {
        use std::collections::BTreeSet;
        let paragraphs: Vec<_> = text
            .lines()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        if paragraphs.is_empty() {
            return String::new();
        }
        let words: BTreeSet<_> = selection
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() > 3)
            .map(str::to_lowercase)
            .collect();
        if words.is_empty() {
            return bounded_utf8(&paragraphs.join("\n"), budget).to_owned();
        }
        let mut ranked: Vec<_> = paragraphs
            .iter()
            .enumerate()
            .map(|(index, paragraph)| {
                let lower = paragraph.to_lowercase();
                (
                    index,
                    words
                        .iter()
                        .filter(|word| lower.contains(word.as_str()))
                        .count(),
                )
            })
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut chosen = BTreeSet::new();
        let mut bytes = 0usize;
        for (index, _) in &ranked {
            for candidate in [
                Some(*index),
                index.checked_sub(1),
                index.checked_add(1).filter(|i| *i < paragraphs.len()),
            ]
            .into_iter()
            .flatten()
            {
                let count = paragraphs[candidate].len() + usize::from(!chosen.is_empty());
                if !chosen.contains(&candidate) && bytes + count <= budget {
                    chosen.insert(candidate);
                    bytes += count;
                }
            }
        }
        if chosen.is_empty() {
            return bounded_utf8(paragraphs[ranked[0].0], budget).to_owned();
        }
        chosen
            .into_iter()
            .map(|index| paragraphs[index])
            .collect::<Vec<_>>()
            .join("\n")
    }
}
/// Truncate supplementary context on a UTF-8 boundary. Never use for selected/manual source.
pub fn bounded_utf8(text: &str, budget: usize) -> &str {
    let mut end = budget.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInput {
    pub snapshot: SelectionSnapshot,
    pub action: ReadingAction,
    pub context: String,
    pub level: ReadingLevel,
    pub limited: bool,
    pub chart_choice: Option<ChartKind>,
    pub question: String,
    pub previous_result: String,
    pub conversation: Vec<ConversationTurn>,
    pub page_context: String,
    pub visual_context: String,
    pub reference: Option<WebReference>,
}
impl RequestInput {
    pub fn new(snapshot: SelectionSnapshot, action: ReadingAction) -> Self {
        Self {
            snapshot,
            action,
            context: String::new(),
            level: ReadingLevel::Automatic,
            limited: false,
            chart_choice: None,
            question: String::new(),
            previous_result: String::new(),
            conversation: vec![],
            page_context: String::new(),
            visual_context: String::new(),
            reference: None,
        }
    }
    pub fn source(&self) -> String {
        if self.context.is_empty() {
            self.snapshot.text.clone()
        } else {
            format!(
                "{}\n\nUser-supplied context:\n{}",
                self.snapshot.text, self.context
            )
        }
    }
    pub fn reading_source(&self) -> String {
        let mut source = self.source();
        if !self.page_context.is_empty() {
            source.push_str("\n\nVisible page text (automatic OCR; may contain reading errors):\n");
            source.push_str(&self.page_context);
        }
        if let Some(reference) = &self.reference {
            source.push_str("\n\n");
            source.push_str(&reference.source());
        }
        source
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(reference) = &self.reference {
            reference.validate()?;
        }
        if self.snapshot.text.trim().is_empty()
            && self.reference.is_none()
            && self.page_context.is_empty()
        {
            return Err(PickleError::message("Select or paste some text first."));
        }
        if self.snapshot.text.len() > limits::SELECTION
            || self.context.len() > limits::CONTEXT
            || self.question.len() > limits::QUESTION
        {
            return Err(PickleError::message(
                "This selection, context, or question is too large. Use a shorter passage.",
            ));
        }
        if self.page_context.len() > limits::PAGE_CONTEXT
            || self.visual_context.len() > limits::VISUAL_CONTEXT
        {
            return Err(PickleError::message(
                "Page context is too large. Remove it and try again.",
            ));
        }
        if self.previous_result.len() > limits::OUTPUT
            || self.conversation.len() > limits::TURNS
            || self
                .conversation
                .iter()
                .map(|t| t.question.len() + t.answer.len())
                .sum::<usize>()
                > limits::CONVERSATION
        {
            return Err(PickleError::message(
                "This conversation is full. Clear follow-ups to continue with the same selection.",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, alias = "prompt_tokens")]
    pub input_tokens: Option<u64>,
    #[serde(default, alias = "completion_tokens")]
    pub output_tokens: Option<u64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalGenerationTiming {
    pub load_seconds: f64,
    pub generation_seconds: f64,
    pub first_token_seconds: Option<f64>,
    pub total_seconds: f64,
    pub output_tokens: u64,
}
#[derive(Debug, Clone)]
pub struct Generation {
    pub text: String,
    pub model: String,
    pub usage: Option<Usage>,
    pub timing: Option<LocalGenerationTiming>,
}
impl Generation {
    pub fn new(text: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            model: model.into(),
            usage: None,
            timing: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct GenerationPrompt {
    pub system: String,
    pub user: String,
    pub structured: bool,
    pub schema: Option<Value>,
    pub local_input: Option<RequestInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingResult {
    pub id: Uuid,
    pub action: ReadingAction,
    pub text: String,
    pub chart: Option<ValidatedChart>,
    pub quality: QualityStatus,
    pub model: String,
    pub evaluator_model: Option<String>,
    pub elapsed: f64,
    pub repairs: usize,
    pub notes: Vec<String>,
    pub timing: Option<LocalGenerationTiming>,
}
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    Draft(String),
    Progress(String),
}
#[async_trait]
pub trait GenerativeProvider: Send + Sync {
    fn is_remote(&self) -> bool;
    fn permits_cloud_checks(&self) -> bool {
        true
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: UnboundedSender<PipelineEvent>,
        cancel: CancellationToken,
    ) -> Result<Generation>;
}
