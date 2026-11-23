//! Explicit, synthetic-only developer benchmark; never reads app configuration.
use pickle_core::{
    ChartKind, Generation, GenerationPrompt, GenerativeProvider, PickleError, Pipeline,
    PipelineEvent, PipelineOutcome, ReadingAction, RequestInput, SelectionSnapshot, bounded_utf8,
};
use pickle_providers::{LocalAIConfiguration, OllamaProvider};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::ExitCode,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const CASE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_RECORD_BYTES: usize = 131_072;
const MAX_CHART_CAPTURE_BYTES: usize = 16_384;
const USAGE: &str = "Synthetic Local pipeline benchmark (no network without --run).\nUsage: cargo run -p pickle-providers --example local-benchmark -- --run [--address URL] [--model NAME]\nDefaults: http://127.0.0.1:11434 and llama3.2:1b. No credentials, model downloads, or app settings.\nJSONL reports pipeline success separately from factual fidelity, which always requires manual review.";
const SHUTTLE: &str = "During a two-week trial, the library shuttle arrived within five minutes of its schedule on 18 of 20 trips. The trial used one route and ran only in dry weather. The results suggest the service may be dependable on that route, but they do not establish how it will perform in rain or on other routes.";

enum Invocation {
    Help,
    Run(LocalAIConfiguration),
}
fn arguments(args: &[String]) -> Result<Invocation, &'static str> {
    if args.is_empty() || args == ["--help"] {
        return Ok(Invocation::Help);
    }
    let mut config = LocalAIConfiguration::default();
    let (mut run, mut address, mut model) = (false, false, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--run" if !run => run = true,
            "--address" if !address => {
                config.address = args
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or("--address requires a server URL.")?
                    .clone();
                address = true;
            }
            "--model" if !model => {
                config.model = args
                    .next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or("--model requires an installed model name.")?
                    .clone();
                model = true;
            }
            _ => return Err("Unknown, repeated, or mixed options. Use --help."),
        }
    }
    if !run {
        return Err("Explicit --run is required before contacting a Local server.");
    }
    config
        .validate(false)
        .map_err(|_| "Invalid trusted server address or downloaded model name.")?;
    Ok(Invocation::Run(config))
}

struct Fixture {
    id: &'static str,
    origin: &'static str,
    input: RequestInput,
}
fn input(source: &str, action: ReadingAction) -> RequestInput {
    RequestInput::new(
        SelectionSnapshot::new(source, "Fixed benchmark", "fixture"),
        action,
    )
}
fn fixtures() -> Vec<Fixture> {
    let library = "The community library will extend its weekday opening hours from 6 PM to 8 PM for a three-month trial. Weekend hours will not change. The trial is funded by a temporary grant, and permanent hours will depend on attendance and future funding.";
    let mut context = input(library, ReadingAction::Simplify);
    context.context = "The proposal follows a survey of residents who work during the day. Staff schedules will be adjusted; no additional permanent positions have been promised.".into();
    context.page_context =
        "The council will review the results at the end of the trial. The grant is temporary."
            .into();
    let mut follow_up = input(library, ReadingAction::FollowUp);
    follow_up.question = "Are the longer hours permanent?".into();
    follow_up.previous_result =
        "The library is testing longer weekday hours for three months.".into();
    let mut bar = input(
        "Revenue was $10 million in 2024 and $15 million in 2025.",
        ReadingAction::Chart,
    );
    bar.chart_choice = Some(ChartKind::Bar);
    let mut flow = input(
        "The request is received, validated, and saved.",
        ReadingAction::Chart,
    );
    flow.chart_choice = Some(ChartKind::Flow);
    let mut scope = input(SHUTTLE, ReadingAction::FollowUp);
    scope.question = "What uncertainty remains about rain and other routes?".into();
    // Fixed prior text, not the preceding model's answer: each case is isolated.
    scope.previous_result = "The trial observed 18 of 20 trips arriving within five minutes of schedule, on one route in dry weather. Rain and other routes were not tested.".into();
    vec![
        Fixture {
            id: "cold_simplify",
            origin: "swift_fixed_corpus",
            input: input(
                "The treatment may reduce symptoms in some patients, but the evidence remains limited.",
                ReadingAction::Simplify,
            ),
        },
        Fixture {
            id: "warm_context",
            origin: "swift_fixed_corpus",
            input: context,
        },
        Fixture {
            id: "follow_up",
            origin: "swift_fixed_corpus",
            input: follow_up,
        },
        Fixture {
            id: "bar_chart",
            origin: "swift_fixed_corpus",
            input: bar,
        },
        Fixture {
            id: "flow_chart",
            origin: "swift_fixed_corpus",
            input: flow,
        },
        Fixture {
            id: "shuttle_expand",
            origin: "recorded_controlled_synthetic_passage",
            input: input(SHUTTLE, ReadingAction::Expand),
        },
        Fixture {
            id: "shuttle_source_scope",
            origin: "recorded_controlled_synthetic_passage",
            input: scope,
        },
    ]
}

enum Failure {
    Provider(PickleError),
    Timeout,
}

struct ChartCandidate {
    text: String,
    truncated: bool,
}
struct CaptureFinalChart {
    inner: Arc<dyn GenerativeProvider>,
    captured: Mutex<Option<ChartCandidate>>,
}
impl CaptureFinalChart {
    fn new(inner: Arc<dyn GenerativeProvider>) -> Self {
        Self {
            inner,
            captured: Mutex::new(None),
        }
    }
    fn reset(&self) {
        self.captured
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }
    fn take(&self) -> Option<ChartCandidate> {
        self.captured
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }
}
#[async_trait::async_trait]
impl GenerativeProvider for CaptureFinalChart {
    fn is_remote(&self) -> bool {
        self.inner.is_remote()
    }
    fn permits_cloud_checks(&self) -> bool {
        self.inner.permits_cloud_checks()
    }
    async fn generate(
        &self,
        prompt: GenerationPrompt,
        events: tokio::sync::mpsc::UnboundedSender<PipelineEvent>,
        cancel: CancellationToken,
    ) -> pickle_core::Result<Generation> {
        let structured = prompt.structured;
        let generation = self.inner.generate(prompt, events, cancel).await?;
        if structured {
            *self.captured.lock().unwrap_or_else(|p| p.into_inner()) = Some(ChartCandidate {
                text: bounded_utf8(&generation.text, MAX_CHART_CAPTURE_BYTES).into(),
                truncated: generation.text.len() > MAX_CHART_CAPTURE_BYTES,
            });
        }
        Ok(generation)
    }
}

fn attach_rejected_chart(record: &mut Value, candidate: Option<ChartCandidate>) {
    if record["action"] == json!(ReadingAction::Chart)
        && record["error_code"] == "chart_rejected"
        && record["pipeline_succeeded"] == false
        && record["chart_validated"] == false
        && let Some(candidate) = candidate
    {
        record["raw_chart_candidate"] = json!({
            "accepted":false, "text":candidate.text, "truncated":candidate.truncated,
        });
    }
}
async fn execute(
    pipeline: &Pipeline,
    input: RequestInput,
    timeout: Duration,
    cancel: CancellationToken,
) -> Result<PipelineOutcome, Failure> {
    let (events, receiver) = tokio::sync::mpsc::unbounded_channel();
    // Streaming still exercises the production path and timing, without
    // retaining every intermediate draft in an unbounded benchmark queue.
    drop(receiver);
    match tokio::time::timeout(timeout, pipeline.run(input, events, cancel.clone())).await {
        Ok(outcome) => outcome.map_err(Failure::Provider),
        Err(_) => {
            cancel.cancel();
            Err(Failure::Timeout)
        }
    }
}

fn safe_error(failure: Failure) -> (&'static str, &'static str) {
    match failure {
        Failure::Timeout => ("case_timeout", "The bounded benchmark case timed out."),
        Failure::Provider(PickleError::Cancelled) => ("cancelled", "The request was cancelled."),
        Failure::Provider(PickleError::Message(message)) => {
            // Never serialize arbitrary server/library errors, which may carry
            // URLs, filesystem paths, or unexpected response content.
            if message.contains("chart")
                || message.contains("diagram")
                || message.contains("quantities")
            {
                (
                    "chart_rejected",
                    "Chart structure or source evidence validation failed.",
                )
            } else if message.contains("model") || message.contains("Cloud aliases") {
                (
                    "model_rejected",
                    "The installed Local model or its capabilities could not be used.",
                )
            } else if message.contains("server")
                || message.contains("provider")
                || message.contains("Provider")
            {
                (
                    "provider_failed",
                    "The Local provider request could not complete.",
                )
            } else {
                (
                    "pipeline_failed",
                    "The Local pipeline failed; raw error details are omitted.",
                )
            }
        }
    }
}
fn report(fixture: &Fixture, outcome: Result<PipelineOutcome, Failure>, seconds: f64) -> Value {
    let mut record = json!({
        "schema":1, "fixture":fixture.id, "fixture_origin":fixture.origin,
        "action":fixture.input.action, "source":fixture.input.source(),
        "page_context":fixture.input.page_context, "question":fixture.input.question,
        "prior_explanation":fixture.input.previous_result,
        "pipeline_succeeded":false, "chart_validated":false,
        "factual_fidelity":"manual_review_required",
        "seconds":seconds, "same_device":false, "inference_location":"unverified",
    });
    match outcome {
        Ok(PipelineOutcome::Result(result)) => {
            record["pipeline_succeeded"] = json!(true);
            record["answer"] = json!(result.text);
            record["model"] = json!(bounded_utf8(&result.model, 200));
            record["quality"] = json!(result.quality);
            record["timing"] = json!(result.timing);
            record["chart_validated"] = json!(result.chart.is_some());
            record["chart"] = json!(result.chart);
        }
        Ok(_) => {
            record["error_code"] = json!("non_result_outcome");
            record["error"] = json!("The pipeline requested more context or a chart choice.");
        }
        Err(failure) => {
            let (code, message) = safe_error(failure);
            record["error_code"] = json!(code);
            record["error"] = json!(message);
        }
    }
    record
}
fn json_line(record: &Value) -> Result<Vec<u8>, &'static str> {
    let mut bytes = serde_json::to_vec(record).map_err(|_| "Could not encode benchmark record.")?;
    if bytes.len() >= MAX_RECORD_BYTES {
        return Err("Benchmark record exceeded its output budget.");
    }
    bytes.push(b'\n');
    Ok(bytes)
}
async fn run(config: LocalAIConfiguration) -> ExitCode {
    let provider = Arc::new(CaptureFinalChart::new(Arc::new(OllamaProvider::new(
        config,
    ))));
    let pipeline = Pipeline::new(provider.clone(), None, false);
    let mut failed = false;
    let stdout = std::io::stdout();
    for fixture in fixtures() {
        provider.reset();
        let began = Instant::now();
        let outcome = execute(
            &pipeline,
            fixture.input.clone(),
            CASE_TIMEOUT,
            CancellationToken::new(),
        )
        .await;
        let mut record = report(&fixture, outcome, began.elapsed().as_secs_f64());
        attach_rejected_chart(&mut record, provider.take());
        failed |= record["pipeline_succeeded"] != true;
        let bytes = match json_line(&record) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        let mut writer = stdout.lock();
        if writer
            .write_all(&bytes)
            .and_then(|_| writer.flush())
            .is_err()
        {
            eprintln!("Could not write benchmark JSONL.");
            return ExitCode::FAILURE;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let config = match arguments(&args) {
        Ok(Invocation::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Invocation::Run(config)) => config,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("Could not start benchmark runtime.");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FixedProvider(Option<&'static str>);
    #[async_trait::async_trait]
    impl pickle_core::GenerativeProvider for FixedProvider {
        fn is_remote(&self) -> bool {
            false
        }
        async fn generate(
            &self,
            _: pickle_core::GenerationPrompt,
            _: tokio::sync::mpsc::UnboundedSender<pickle_core::PipelineEvent>,
            _: CancellationToken,
        ) -> pickle_core::Result<pickle_core::Generation> {
            match self.0 {
                Some(answer) => Ok(pickle_core::Generation::new(answer, "Fixed test provider")),
                None => std::future::pending().await,
            }
        }
    }
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
    }

    #[test]
    fn networking_requires_unambiguous_explicit_run_and_valid_config() {
        assert!(matches!(arguments(&[]), Ok(Invocation::Help)));
        assert!(matches!(
            arguments(&args(&["--help"])),
            Ok(Invocation::Help)
        ));
        for values in [
            vec!["--model", "llama3.2:1b"],
            vec!["--run", "--run"],
            vec!["--run", "--help"],
            vec!["--run", "--token", "unused"],
            vec!["--run", "--address"],
            vec!["--run", "--model", "cloud-model"],
            vec![
                "--run",
                "--address",
                "https://user:password@example.invalid",
            ],
            vec!["--run", "--address", "http://192.0.2.1"],
        ] {
            assert!(arguments(&args(&values)).is_err());
        }
        let Invocation::Run(config) = arguments(&args(&["--run"])).unwrap() else {
            panic!()
        };
        assert_eq!(config.address, "http://127.0.0.1:11434");
        assert!(!config.same_device && config.token.is_empty());
    }

    #[test]
    fn corpus_is_fixed_valid_and_preserves_the_recorded_shuttle_scope() {
        let corpus = fixtures();
        assert_eq!(corpus.len(), 7);
        for fixture in &corpus {
            fixture.input.validate().unwrap();
        }
        let shuttle = &corpus[5];
        assert_eq!(shuttle.input.action, ReadingAction::Expand);
        assert_eq!(shuttle.input.snapshot.text, SHUTTLE);
        assert!(shuttle.input.previous_result.is_empty());
        assert_eq!(
            corpus[6].input.question,
            "What uncertainty remains about rain and other routes?"
        );
        assert_eq!(corpus[3].input.chart_choice, Some(ChartKind::Bar));
        assert_eq!(corpus[4].input.chart_choice, Some(ChartKind::Flow));
    }

    #[test]
    fn arbitrary_failure_content_never_enters_jsonl() {
        let fixture = fixtures().remove(0);
        let record = report(
            &fixture,
            Err(Failure::Provider(PickleError::message(
                "https://private.invalid/secret /private/key token=never",
            ))),
            0.0,
        );
        let bytes = json_line(&record).unwrap();
        let line = String::from_utf8(bytes).unwrap();
        assert!(
            !line.contains("private.invalid")
                && !line.contains("token=")
                && !line.contains("/private/key")
        );
        assert_eq!(record["pipeline_succeeded"], false);
        assert_eq!(record["factual_fidelity"], "manual_review_required");
        assert!(serde_json::from_str::<Value>(line.trim_end()).is_ok());
        assert!(json_line(&json!({"answer":"x".repeat(MAX_RECORD_BYTES)})).is_err());
    }

    #[tokio::test]
    async fn a_returned_answer_never_receives_an_automatic_factual_pass() {
        let fixture = fixtures().remove(5);
        let unsupported = "The larger research project tested 18 trips.";
        let pipeline = Pipeline::new(Arc::new(FixedProvider(Some(unsupported))), None, false);
        let outcome = execute(
            &pipeline,
            fixture.input.clone(),
            CASE_TIMEOUT,
            CancellationToken::new(),
        )
        .await;
        let record = report(&fixture, outcome, 0.0);
        assert_eq!(record["pipeline_succeeded"], true);
        assert_eq!(record["answer"], unsupported);
        assert_eq!(record["source"], SHUTTLE);
        assert_eq!(record["factual_fidelity"], "manual_review_required");
        assert_eq!(record["inference_location"], "unverified");
        assert!(json_line(&record).unwrap().len() < MAX_RECORD_BYTES);
    }

    #[tokio::test]
    async fn failed_chart_record_keeps_exact_unaccepted_candidate_without_bypassing_validation() {
        let fixture = fixtures().remove(4);
        let invalid = r#"{"type":"flow_diagram","nodes":[],"edges":[]}"#;
        let provider = Arc::new(CaptureFinalChart::new(Arc::new(FixedProvider(Some(
            invalid,
        )))));
        let pipeline = Pipeline::new(provider.clone(), None, false);
        provider.reset();
        let outcome = execute(
            &pipeline,
            fixture.input.clone(),
            CASE_TIMEOUT,
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(outcome, Err(Failure::Provider(_))));
        let mut record = report(&fixture, outcome, 0.0);
        attach_rejected_chart(&mut record, provider.take());
        assert_eq!(record["error_code"], "chart_rejected");
        assert_eq!(record["pipeline_succeeded"], false);
        assert_eq!(record["chart_validated"], false);
        assert_eq!(record["factual_fidelity"], "manual_review_required");
        assert!(record.get("answer").is_none());
        assert_eq!(record["raw_chart_candidate"]["text"], invalid);
        assert_eq!(record["raw_chart_candidate"]["accepted"], false);
        assert_eq!(record["raw_chart_candidate"]["truncated"], false);
        assert_eq!(record["source"], fixture.input.source());
        assert!(json_line(&record).unwrap().len() < MAX_RECORD_BYTES);

        // The next nonstructured case cannot inherit the previous chart text.
        provider.reset();
        let prose = fixtures().remove(0);
        let outcome = execute(
            &pipeline,
            prose.input.clone(),
            CASE_TIMEOUT,
            CancellationToken::new(),
        )
        .await;
        let mut record = report(&prose, outcome, 0.0);
        assert!(provider.take().is_none());
        attach_rejected_chart(
            &mut record,
            Some(ChartCandidate {
                text: "must not appear".into(),
                truncated: false,
            }),
        );
        assert!(record.get("raw_chart_candidate").is_none());
    }

    #[tokio::test]
    async fn capture_preserves_flags_prompt_events_cancellation_and_the_complete_generation() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct InspectingProvider {
            calls: AtomicUsize,
            cancel: Mutex<Option<CancellationToken>>,
        }
        #[async_trait::async_trait]
        impl GenerativeProvider for InspectingProvider {
            fn is_remote(&self) -> bool {
                true
            }
            fn permits_cloud_checks(&self) -> bool {
                false
            }
            async fn generate(
                &self,
                prompt: GenerationPrompt,
                events: tokio::sync::mpsc::UnboundedSender<PipelineEvent>,
                cancel: CancellationToken,
            ) -> pickle_core::Result<Generation> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(prompt.system, "Exact system");
                assert_eq!(prompt.user, "Exact user");
                assert_eq!(prompt.schema, Some(json!({"type":"object"})));
                assert_eq!(prompt.local_input.unwrap().snapshot.text, SHUTTLE);
                *self.cancel.lock().unwrap() = Some(cancel);
                events
                    .send(PipelineEvent::Draft(
                        "A draft that must not be captured".into(),
                    ))
                    .unwrap();
                Ok(Generation {
                    text: "🥒".repeat(5_000),
                    model: "Exact model".into(),
                    usage: Some(pickle_core::Usage {
                        input_tokens: Some(23),
                        output_tokens: Some(42),
                    }),
                    timing: Some(pickle_core::LocalGenerationTiming {
                        load_seconds: 0.1,
                        generation_seconds: 0.2,
                        first_token_seconds: Some(0.03),
                        total_seconds: 0.3,
                        output_tokens: 42,
                    }),
                })
            }
        }
        let inner = Arc::new(InspectingProvider {
            calls: AtomicUsize::new(0),
            cancel: Mutex::new(None),
        });
        let provider = CaptureFinalChart::new(inner.clone());
        assert!(provider.is_remote());
        assert!(!provider.permits_cloud_checks());
        let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let result = provider
            .generate(
                GenerationPrompt {
                    system: "Exact system".into(),
                    user: "Exact user".into(),
                    structured: true,
                    schema: Some(json!({"type":"object"})),
                    local_input: Some(input(SHUTTLE, ReadingAction::Expand)),
                },
                events,
                cancel.clone(),
            )
            .await
            .unwrap();
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
        assert!(
            matches!(receiver.try_recv().unwrap(),PipelineEvent::Draft(text) if text=="A draft that must not be captured")
        );
        assert_eq!(result.text, "🥒".repeat(5_000));
        assert_eq!(result.model, "Exact model");
        assert_eq!(result.usage.unwrap().output_tokens, Some(42));
        assert_eq!(result.timing.unwrap().total_seconds, 0.3);
        cancel.cancel();
        assert!(
            inner
                .cancel
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled()
        );
        let candidate = provider.take().unwrap();
        assert!(candidate.truncated);
        assert_eq!(candidate.text.len(), MAX_CHART_CAPTURE_BYTES);
        assert!(candidate.text.chars().all(|ch| ch == '🥒'));
        assert!(provider.take().is_none());
    }

    #[tokio::test]
    async fn outer_deadline_cancels_the_real_pipeline_future_without_network() {
        let pipeline = Pipeline::new(Arc::new(FixedProvider(None)), None, false);
        let cancel = CancellationToken::new();
        let outcome = execute(
            &pipeline,
            fixtures().remove(0).input,
            Duration::from_millis(1),
            cancel.clone(),
        )
        .await;
        assert!(matches!(outcome, Err(Failure::Timeout)));
        assert!(cancel.is_cancelled());
    }
}
