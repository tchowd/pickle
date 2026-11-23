use crate::{PickleError, Result, limits};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChartKind {
    #[serde(rename = "bar_chart")]
    Bar,
    #[serde(rename = "flow_diagram")]
    Flow,
}
impl ChartKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar_chart",
            Self::Flow => "flow_diagram",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NumericEvidence {
    pub id: String,
    pub value: f64,
    pub unit: String,
    pub quote: String,
    pub excerpt: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    pub evidence: String,
    pub condition: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ValidatedChart {
    Bar(Vec<NumericEvidence>),
    Flow {
        nodes: Vec<FlowNode>,
        edges: Vec<FlowEdge>,
    },
}
impl ValidatedChart {
    pub fn kind(&self) -> ChartKind {
        match self {
            Self::Bar(_) => ChartKind::Bar,
            Self::Flow { .. } => ChartKind::Flow,
        }
    }
    pub fn alternative(&self) -> String {
        match self {
            Self::Bar(values) => format!(
                "Quantities in your selection\n{}",
                values
                    .iter()
                    .map(|v| format!("{}: {} {}", v.excerpt, v.value, v.unit))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Self::Flow { nodes, edges } => {
                let labels: BTreeMap<_, _> = nodes
                    .iter()
                    .map(|n| (n.id.as_str(), n.label.as_str()))
                    .collect();
                format!(
                    "Relationships in your selection\n{}",
                    edges
                        .iter()
                        .map(|e| format!(
                            "{} → {}{}\nSource: {}",
                            labels.get(e.from.as_str()).unwrap_or(&""),
                            labels.get(e.to.as_str()).unwrap_or(&""),
                            if e.condition.is_empty() {
                                String::new()
                            } else {
                                format!(" [{}]", e.condition)
                            },
                            e.evidence
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        }
    }
}

pub fn validate_prose(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > limits::OUTPUT {
        Err(PickleError::message(
            "The generated result was empty or too long. Try a shorter passage.",
        ))
    } else {
        Ok(())
    }
}
pub fn without_preamble(text: &str) -> String {
    static ANNOUNCEMENT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^(?:(?:sure|certainly|of course|okay|ok|absolutely)[,.!]?\s+)?(?:here(?:'s|’s|\s+is|\s+are)|below\s+is|the\s+following\s+is)\b.*\b(?:explanation|version|summary|simplification|breakdown|rewrite|overview|answer|response|example)s?\b").unwrap()
    });
    if let Some((first, rest)) = text.trim_start().split_once('\n') {
        let line = first.trim_matches(|c| "*_#> \t".contains(c));
        if !rest.trim().is_empty()
            && line.chars().count() <= 200
            && line.ends_with(':')
            && ANNOUNCEMENT.is_match(line)
        {
            return rest.trim_start().into();
        }
    }
    text.into()
}

pub fn numeric_evidence(source: &str) -> Vec<NumericEvidence> {
    static NUMBERS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)([$€£])?(-?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?)(?:[ \t]+(thousand|million|billion))?[ \t]*(%|percent\b|USD\b|EUR\b|GBP\b|kg\b|km\b|ms\b|seconds\b|minutes\b|hours\b|users\b|requests\b|items\b)?").unwrap()
    });
    static EXAMPLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)hypothetical|illustrative|for example|suppose").unwrap());
    NUMBERS
        .captures_iter(source)
        .filter_map(|c| {
            let m = c.get(0)?;
            let previous = source[..m.start()].chars().next_back();
            if previous.is_some_and(|ch| is_word_character(ch) || ".,".contains(ch)) {
                return None;
            }
            let token = m.as_str().trim_end();
            let token_end = m.start() + token.len();
            let after = &source[token_end..];
            if after.chars().next().is_some_and(is_word_character) {
                return None;
            }
            let mut following = after.chars();
            if following.next().is_some_and(|ch| ch == '.' || ch == ',')
                && following.next().is_some_and(|ch| ch.is_ascii_digit())
            {
                return None;
            }
            let prefix = c.get(1).map_or("", |m| m.as_str());
            let suffix = c.get(4).map_or("", |m| m.as_str()).to_ascii_lowercase();
            if prefix.is_empty() && suffix.is_empty() {
                return None;
            }
            let currency = match prefix {
                "$" => Some("USD"),
                "€" => Some("EUR"),
                "£" => Some("GBP"),
                _ => None,
            };
            if !prefix.is_empty()
                && !suffix.is_empty()
                && currency.map(|s| s.to_ascii_lowercase()).as_deref() != Some(suffix.as_str())
            {
                return None;
            }
            let scale = match c.get(3).map(|m| m.as_str().to_ascii_lowercase()).as_deref() {
                Some("thousand") => 1e3,
                Some("million") => 1e6,
                Some("billion") => 1e9,
                _ => 1.0,
            };
            let value = c.get(2)?.as_str().replace(',', "").parse::<f64>().ok()? * scale;
            if !value.is_finite() || value.abs() > 1e15 {
                return None;
            }
            let unit = currency
                .map(str::to_owned)
                .unwrap_or_else(|| match suffix.as_str() {
                    "percent" => "%".into(),
                    "usd" => "USD".into(),
                    "eur" => "EUR".into(),
                    "gbp" => "GBP".into(),
                    _ => suffix,
                });
            let start = source[..m.start()]
                .char_indices()
                .rev()
                .nth(44)
                .map_or(0, |(i, _)| i);
            let end = source[token_end..]
                .char_indices()
                .nth(45)
                .map_or(source.len(), |(i, _)| token_end + i);
            let excerpt = source[start..end].trim().to_owned();
            if EXAMPLE.is_match(&excerpt) {
                return None;
            }
            Some(NumericEvidence {
                id: format!("n{}", source[..m.start()].encode_utf16().count()),
                value,
                unit,
                quote: m.as_str().trim().into(),
                excerpt,
            })
        })
        .collect()
}

// Match the Unicode `\w` boundary used by the original Foundation parser.
// Combining marks, connector punctuation and join controls are parts of words
// too: splitting there can turn a technical identifier into chart evidence.
fn is_word_character(ch: char) -> bool {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\w\z").unwrap());
    let mut bytes = [0; 4];
    WORD.is_match(ch.encode_utf8(&mut bytes))
}

fn ordered_step_sequence() -> &'static Regex {
    static SEQUENCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:is|are)\s+([a-z]+ed(?:,\s*[a-z]+ed)*(?:,?\s+and\s+[a-z]+ed))\b")
            .unwrap()
    });
    &SEQUENCE
}

pub fn explicit_ordered_steps(source: &str) -> Vec<String> {
    static AND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\band\b").unwrap());
    let Some(c) = ordered_step_sequence().captures(source) else {
        return vec![];
    };
    AND.replace_all(c.get(1).unwrap().as_str(), ",")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A deliberately bounded whole-source operation-list form under the existing
/// ordered-step contract, not a general language or causal-relation classifier.
/// Unknown participles, clauses and ambiguous prefixes retain the general path.
fn sole_operation_list(source: &str) -> Option<Vec<String>> {
    let source = source.trim();
    if source
        .chars()
        .any(|ch| ch.is_control() && ch != '\t' || ch.is_whitespace() && !matches!(ch, ' ' | '\t'))
    {
        return None;
    }
    let capture = ordered_step_sequence().captures(source)?;
    let sequence = capture.get(0)?;
    if !matches!(source[sequence.end()..].trim(), "" | ".") {
        return None;
    }
    let subject: Vec<_> = source[..sequence.start()]
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let article = |word: &str| matches!(word, "the" | "a" | "an");
    if !(subject.len() == 1 && !article(&subject[0])
        || (2..=4).contains(&subject.len()) && article(&subject[0]))
        || subject.iter().any(|word| {
            !word.bytes().all(|ch| ch.is_ascii_alphabetic())
                || word.ends_with("ed")
                || word.ends_with("ly")
                || matches!(
                    word.as_str(),
                    "if" | "unless"
                        | "except"
                        | "when"
                        | "whenever"
                        | "while"
                        | "after"
                        | "before"
                        | "once"
                        | "until"
                        | "provided"
                        | "assuming"
                        | "only"
                        | "not"
                        | "never"
                        | "no"
                        | "neither"
                        | "nor"
                        | "either"
                        | "or"
                        | "and"
                        | "but"
                        | "because"
                        | "since"
                        | "although"
                        | "though"
                        | "as"
                        | "that"
                        | "which"
                        | "who"
                        | "whose"
                        | "where"
                        | "whether"
                        | "may"
                        | "might"
                        | "can"
                        | "could"
                        | "must"
                        | "should"
                        | "would"
                        | "will"
                        | "shall"
                        | "has"
                        | "have"
                        | "had"
                        | "is"
                        | "are"
                        | "was"
                        | "were"
                        | "be"
                        | "been"
                        | "being"
                        | "to"
                        | "by"
                        | "for"
                        | "with"
                        | "without"
                        | "under"
                        | "during"
                        | "in"
                        | "on"
                        | "at"
                        | "of"
                        | "from"
                        | "it"
                        | "they"
                        | "this"
                        | "these"
                        | "those"
                        | "he"
                        | "she"
                        | "we"
                        | "you"
                        | "i"
                        | "cycle"
                        | "loop"
                        | "cyclic"
                        | "iterative"
                        | "circular"
                        | "repeating"
                        | "recurring"
                )
        })
    {
        return None;
    }
    let steps = explicit_ordered_steps(source);
    let normalized: BTreeSet<_> = steps.iter().map(|step| step.to_ascii_lowercase()).collect();
    if !(2..=10).contains(&steps.len())
        || normalized.len() != steps.len()
        || subject.iter().any(|word| normalized.contains(word))
        || normalized.iter().any(|step| {
            !matches!(
                step.as_str(),
                "received"
                    | "validated"
                    | "saved"
                    | "created"
                    | "checked"
                    | "processed"
                    | "stored"
                    | "queued"
                    | "reviewed"
                    | "approved"
                    | "rejected"
                    | "recorded"
                    | "updated"
                    | "deleted"
                    | "archived"
                    | "published"
                    | "completed"
                    | "forwarded"
                    | "delivered"
                    | "closed"
            )
        })
    {
        return None;
    }
    Some(steps)
}

pub fn validate_chart(text: &str, kind: ChartKind, source: &str) -> Result<ValidatedChart> {
    validate_prose(text)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Bar {
        #[serde(rename = "evidenceID")]
        evidence_id: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Bars {
        #[serde(rename = "type")]
        kind: ChartKind,
        bars: Vec<Bar>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Flow {
        #[serde(rename = "type")]
        kind: ChartKind,
        nodes: Vec<FlowNode>,
        edges: Vec<FlowEdge>,
    }
    let invalid = || {
        PickleError::message(
            "The model returned an invalid chart schema. Try again or use Explain more.",
        )
    };
    match kind {
        ChartKind::Bar => {
            let spec: Bars = serde_json::from_str(text).map_err(|_| invalid())?;
            if spec.kind != kind || !(2..=12).contains(&spec.bars.len()) {
                return Err(invalid());
            }
            let evidence: BTreeMap<_, _> = numeric_evidence(source)
                .into_iter()
                .map(|e| (e.id.clone(), e))
                .collect();
            let mut seen = BTreeSet::new();
            let mut bars = vec![];
            for bar in spec.bars {
                if !seen.insert(bar.evidence_id.clone()) {
                    return Err(PickleError::message("Chart quantities must be distinct."));
                }
                bars.push(
                    evidence
                        .get(&bar.evidence_id)
                        .ok_or_else(|| {
                            PickleError::message(
                                "A chart quantity has no explicit source evidence.",
                            )
                        })?
                        .clone(),
                );
            }
            if bars.iter().any(|b| b.unit != bars[0].unit) {
                return Err(PickleError::message(
                    "Chart quantities use incompatible units.",
                ));
            }
            Ok(ValidatedChart::Bar(bars))
        }
        ChartKind::Flow => {
            let spec: Flow = serde_json::from_str(text).map_err(|_| invalid())?;
            if spec.kind != kind
                || !(2..=10).contains(&spec.nodes.len())
                || !(1..=16).contains(&spec.edges.len())
            {
                return Err(invalid());
            }
            let mut labels = BTreeMap::new();
            for node in &spec.nodes {
                if node.id.is_empty()
                    || node.id.chars().count() > 40
                    || node.label.is_empty()
                    || node.label.chars().count() > 180
                    || !source.contains(&node.label)
                    || labels
                        .insert(node.id.as_str(), node.label.as_str())
                        .is_some()
                {
                    return Err(PickleError::message(
                        "The flow diagram has invalid or unsupported nodes.",
                    ));
                }
            }
            static CONDITIONAL: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(r"(?i)\b(if|unless|except|only when|provided that)\b").unwrap()
            });
            let mut seen = BTreeSet::new();
            let mut connected = BTreeSet::new();
            for edge in &spec.edges {
                let from = labels.get(edge.from.as_str()).ok_or_else(invalid)?;
                let to = labels.get(edge.to.as_str()).ok_or_else(invalid)?;
                if edge.from == edge.to
                    || !seen.insert((&edge.from, &edge.to))
                    || edge.evidence.is_empty()
                    || edge.evidence.chars().count() > 1200
                    || !source.contains(&edge.evidence)
                    || !edge.evidence.contains(from)
                    || !edge.evidence.contains(to)
                    || edge.condition.chars().count() > 180
                    || (!edge.condition.is_empty() && !edge.evidence.contains(&edge.condition))
                {
                    return Err(PickleError::message(
                        "A flow connection lacks valid source evidence.",
                    ));
                }
                if CONDITIONAL.is_match(&edge.evidence) && edge.condition.is_empty() {
                    return Err(PickleError::message(
                        "The diagram omitted a source condition.",
                    ));
                }
                connected.insert(edge.from.as_str());
                connected.insert(edge.to.as_str());
            }
            if connected != labels.keys().copied().collect() {
                return Err(PickleError::message("The diagram has disconnected nodes."));
            }
            let steps = explicit_ordered_steps(source);
            if steps
                .iter()
                .any(|step| !labels.values().any(|label| *label == step))
                || steps.windows(2).any(|pair| {
                    !spec.edges.iter().any(|e| {
                        labels.get(e.from.as_str()).copied() == Some(pair[0].as_str())
                            && labels.get(e.to.as_str()).copied() == Some(pair[1].as_str())
                    })
                })
            {
                return Err(PickleError::message(
                    "The diagram omitted an explicitly stated process step or connection.",
                ));
            }
            if let Some(steps) = sole_operation_list(source) {
                let expected_labels: BTreeSet<_> = steps.iter().map(String::as_str).collect();
                let actual_labels: BTreeSet<_> = labels.values().copied().collect();
                // Compare only to relations already required by the existing
                // step contract. Never synthesize, replace or repair a graph.
                let expected_pairs: BTreeSet<_> = steps
                    .windows(2)
                    .map(|pair| (pair[0].as_str(), pair[1].as_str()))
                    .collect();
                let actual_pairs: BTreeSet<_> = spec
                    .edges
                    .iter()
                    .map(|edge| (labels[edge.from.as_str()], labels[edge.to.as_str()]))
                    .collect();
                if spec.nodes.len() != steps.len()
                    || actual_labels != expected_labels
                    || spec.edges.len() != steps.len() - 1
                    || actual_pairs != expected_pairs
                    || spec.edges.iter().any(|edge| !edge.condition.is_empty())
                {
                    return Err(PickleError::message(
                        "The flow diagram added connections or conditions absent from the sole operation list.",
                    ));
                }
            }
            Ok(ValidatedChart::Flow {
                nodes: spec.nodes,
                edges: spec.edges,
            })
        }
    }
}

fn flow_step_id(index: usize) -> String {
    format!("s{index}")
}

pub fn chart_schema(kind: ChartKind, source: &str) -> Value {
    let object = |properties: Value| {
        let keys: Vec<_> = properties.as_object().unwrap().keys().cloned().collect();
        json!({"type":"object","properties":properties,"required":keys,"additionalProperties":false})
    };
    match kind {
        ChartKind::Bar => object(
            json!({"type":{"type":"string","enum":["bar_chart"]},"bars":{"type":"array","minItems":2,"maxItems":12,"items":object(json!({"evidenceID":{"type":"string","enum":numeric_evidence(source).iter().map(|e|e.id.clone()).collect::<Vec<_>>()}}))}}),
        ),
        ChartKind::Flow => {
            let steps = explicit_ordered_steps(source);
            let sole = sole_operation_list(source);
            let label = if steps.is_empty() {
                json!({"type":"string","maxLength":180})
            } else {
                json!({"type":"string","enum":steps})
            };
            let quotes = flow_evidence(source);
            let condition = if sole.is_some() {
                json!({"type":"string","enum":[""]})
            } else {
                json!({"type":"string","maxLength":180})
            };
            let min_edges = sole.as_ref().map_or(1, |steps| steps.len() - 1);
            let max_edges = sole.as_ref().map_or(16, |steps| steps.len() - 1);
            let id = if steps.is_empty() {
                json!({"type":"string","maxLength":40})
            } else {
                json!({"type":"string","enum":(0..steps.len()).map(flow_step_id).collect::<Vec<_>>()})
            };
            object(
                json!({"type":{"type":"string","enum":["flow_diagram"]},"nodes":{"type":"array","minItems":steps.len().max(2),"maxItems":if steps.is_empty(){10}else{steps.len()},"items":object(json!({"id":id,"label":label}))},"edges":{"type":"array","minItems":min_edges,"maxItems":max_edges,"items":object(json!({"from":id,"to":id,"evidence":{"type":"string","enum":quotes},"condition":condition}))}}),
            )
        }
    }
}
pub fn flow_evidence(source: &str) -> Vec<String> {
    if source.chars().count() <= 1200 {
        return vec![source.into()];
    }
    source
        // Sentence boundaries keep decimal/version punctuation within its
        // literal source quote, while also recognizing Unicode terminators.
        .split_sentence_bounds()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 1200)
        .map(str::to_owned)
        .collect()
}
pub fn chart_instructions(kind: ChartKind, source: &str) -> String {
    match kind {
        ChartKind::Bar => format!(
            "Return only JSON {{\"type\":\"bar_chart\",\"bars\":[{{\"evidenceID\":\"ID\"}}]}}. Select 2–12 comparable candidates of the same unit measuring the same thing. Do not add values or labels; app code supplies those. Candidates: {}",
            serde_json::to_string(&numeric_evidence(source)).unwrap()
        ),
        ChartKind::Flow => {
            let mut instructions = String::from(
                "Return only JSON with type flow_diagram, nodes [{id,label}], and edges [{from,to,evidence,condition}]. Use 2–10 nodes and 1–16 supported directed edges. Labels must be exact source substrings. Include every explicit process step and connect the complete stated sequence. Each edge evidence must be an exact allowed source quote containing both connected labels. Preserve all conditions; condition is an exact source condition or empty string. No inferred relationships.",
            );
            let steps = explicit_ordered_steps(source);
            if !steps.is_empty() {
                let names: Vec<_> = steps
                    .iter()
                    .enumerate()
                    .map(|(index, label)| json!({"id":flow_step_id(index),"label":label}))
                    .collect();
                instructions.push_str(&format!(" Source step node IDs (names only): {}. Use each listed ID once with its corresponding exact label. Edge from/to must use these IDs, never labels or source sentences. Infer no additional relationships; existing evidence and condition rules still apply.",serde_json::to_string(&names).unwrap()));
            }
            if let Some(steps) = sole_operation_list(source) {
                instructions.push_str(&format!(" For this whole-source operation-list format, use exactly {} edges between consecutive listed steps and empty condition strings. Do not add back edges or other connections.",steps.len()-1));
            }
            instructions
        }
    }
}
