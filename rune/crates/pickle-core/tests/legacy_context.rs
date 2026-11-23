//! Original PageContext/WebReference predicates through the shared pipeline.
//! All context is fixed synthetic text; no fetching, model, or native capture.
use async_trait::async_trait;
use pickle_core::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

struct Writer {
    response: &'static str,
    prompts: Mutex<Vec<GenerationPrompt>>,
}
impl Writer {
    fn new(response: &'static str) -> Arc<Self> {
        Arc::new(Self {
            response,
            prompts: Mutex::new(vec![]),
        })
    }
}
#[async_trait]
impl GenerativeProvider for Writer {
    fn is_remote(&self) -> bool {
        false
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        _: UnboundedSender<PipelineEvent>,
        _: CancellationToken,
    ) -> Result<Generation> {
        self.prompts.lock().unwrap().push(prompt);
        Ok(Generation::new(self.response, "fixture"))
    }
}
async fn run(writer: Arc<Writer>, input: RequestInput) -> Result<PipelineOutcome> {
    let (events, _) = unbounded_channel();
    Pipeline::new(writer, None, false)
        .run(input, events, CancellationToken::new())
        .await
}
fn reference(text: impl Into<String>) -> WebReference {
    WebReference {
        url: "https://example.com".into(),
        title: "Title".into(),
        text: text.into(),
        kind: "article".into(),
        timestamp: None,
    }
}

#[tokio::test]
async fn screen_numbers_cannot_create_a_chart_or_reach_the_writer() {
    // Exact testScreenNumbersCannotCreateAChart input: neither the OCR nor
    // visual-summary dollars may rescue a numberless selected passage.
    let writer = Writer::new("unused");
    let mut input = RequestInput::new(
        SelectionSnapshot::new("A passage with no numbers.", "Test", "test"),
        ReadingAction::Chart,
    );
    input.chart_choice = Some(ChartKind::Bar);
    input.page_context = "$10 and $20".into();
    input.visual_context = "$30 and $40".into();
    let error = run(writer.clone(), input.clone())
        .await
        .expect_err("Supplementary screen numbers must not produce a chart/result");
    assert!(
        error
            .to_string()
            .contains("at least two explicit quantities")
    );
    assert!(writer.prompts.lock().unwrap().is_empty());
    assert_eq!(input.snapshot.text, "A passage with no numbers.");
}

#[tokio::test]
async fn reference_reaches_prose_but_same_writer_cannot_use_its_numbers_for_a_chart() {
    // Exact testReferencesReachProseButNotChartEvidence sequence and source.
    let writer = Writer::new("A faithful explanation.");
    let mut input = RequestInput::new(
        SelectionSnapshot::new("Revenue increased.", "Browser", "browser"),
        ReadingAction::Simplify,
    );
    input.reference = Some(reference("Revenue was $20 and $40."));
    let PipelineOutcome::Result(result) = run(writer.clone(), input.clone()).await.unwrap() else {
        panic!("Expected the explanation with the attached reference")
    };
    assert_eq!(result.text, "A faithful explanation.");
    assert!(result.chart.is_none());
    {
        let prompts = writer.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 1);
        let body: serde_json::Value = serde_json::from_str(&prompts[0].user).unwrap();
        assert_eq!(
            body["source"],
            "Revenue increased.\n\nAdditional article reference (untrusted source text):\nTitle: Title\nURL: https://example.com\nRevenue was $20 and $40."
        );
        assert!(!prompts[0].structured);
        assert_eq!(
            prompts[0].local_input.as_ref().unwrap().snapshot.text,
            "Revenue increased."
        );
    }
    input.action = ReadingAction::Chart;
    input.chart_choice = Some(ChartKind::Bar);
    let error = run(writer.clone(), input.clone())
        .await
        .expect_err("Reference numbers must not become selected chart evidence");
    assert!(
        error
            .to_string()
            .contains("at least two explicit quantities")
    );
    assert_eq!(writer.prompts.lock().unwrap().len(), 1);
    assert_eq!(input.snapshot.text, "Revenue increased.");
    assert_eq!(input.reference.unwrap().text, "Revenue was $20 and $40.");
}

#[tokio::test]
async fn original_ocr_label_and_visual_summary_isolation_survive_real_pipeline_prompting() {
    let writer = Writer::new("An explanation.");
    let mut input = RequestInput::new(
        SelectionSnapshot::new("This is the passage.", "Test", "test"),
        ReadingAction::Simplify,
    );
    input.page_context = "A visible heading".into();
    input.visual_context = "A possible visual relationship".into();
    assert!(matches!(
        run(writer.clone(), input).await.unwrap(),
        PipelineOutcome::Result(_)
    ));
    let prompts = writer.prompts.lock().unwrap();
    assert_eq!(prompts.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&prompts[0].user).unwrap();
    assert_eq!(
        body["source"],
        "This is the passage.\n\nVisible page text (automatic OCR; may contain reading errors):\nA visible heading"
    );
    assert_eq!(
        body["visual_context_summary"],
        "A possible visual relationship"
    );
    assert!(!body["source"].as_str().unwrap().contains("possible visual"));
    assert!(prompts[0].system.contains("never source evidence"));
    assert_eq!(
        prompts[0].local_input.as_ref().unwrap().snapshot.text,
        "This is the passage."
    );
}

#[test]
fn original_unicode_ocr_prefix_and_explicit_page_summary_byte_limits() {
    assert_eq!(limits::PAGE_CONTEXT, 6000);
    assert_eq!(limits::VISUAL_CONTEXT, 3000);
    let text = "🌍 café\n".repeat(2000);
    let bounded = bounded_utf8(&text, limits::PAGE_CONTEXT);
    assert_eq!(bounded.len(), 6000);
    assert!(text.starts_with(bounded));
    let mut input = RequestInput::new(
        SelectionSnapshot::new("Selected passage", "Test", "test"),
        ReadingAction::Simplify,
    );
    input.page_context = "a".repeat(6001);
    assert!(input.validate().is_err());
    input.page_context = "a".repeat(6000);
    assert!(input.validate().is_ok());
    input.page_context.clear();
    input.visual_context = "a".repeat(3001);
    assert!(input.validate().is_err());
    input.visual_context = "a".repeat(3000);
    assert!(input.validate().is_ok());
}

#[test]
fn original_empty_and_over_budget_reference_bodies_are_rejected() {
    assert_eq!(limits::REFERENCE, 12_000);
    assert!(reference("Page text").validate().is_ok());
    for body in [String::new(), "x".repeat(12_001)] {
        let value = reference(body);
        assert!(value.validate().is_err());
        let mut input = RequestInput::new(
            SelectionSnapshot::new("Selected passage", "Test", "test"),
            ReadingAction::Simplify,
        );
        input.reference = Some(value);
        assert!(input.validate().is_err());
    }
    assert!(reference("x".repeat(12_000)).validate().is_ok());
}
