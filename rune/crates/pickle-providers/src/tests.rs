use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use pickle_core::{
    DecisionClient, DecisionQuestion, GenerationPrompt, GenerativeProvider, Pipeline,
    PipelineEvent, PipelineOutcome, ReadingAction, RequestInput, SelectionSnapshot, Usage,
    WebReference,
};
use serde_json::{Value, json};
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    CloudflareProvider, JevClient, LocalAIConfiguration, OllamaProvider, SampleProvider,
    local_budget::{LOCAL_PROMPT_BYTES, fit},
    streams::{Lines, Ndjson, Sse},
};

const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
fn prompt() -> GenerationPrompt {
    GenerationPrompt {
        system: "Explain this source.".into(),
        user: "A study may help.".into(),
        structured: false,
        schema: None,
        local_input: None,
    }
}

struct Reply {
    status: u16,
    mime: &'static str,
    body: String,
    extra: String,
    delay: Duration,
    disconnect: bool,
}
impl Reply {
    fn json(body: Value) -> Self {
        Self {
            status: 200,
            mime: "application/json",
            body: body.to_string(),
            extra: String::new(),
            delay: Duration::ZERO,
            disconnect: false,
        }
    }
    fn stream(body: &str) -> Self {
        Self {
            status: 200,
            mime: "text/event-stream",
            body: body.into(),
            extra: String::new(),
            delay: Duration::ZERO,
            disconnect: false,
        }
    }
    fn disconnected() -> Self {
        Self {
            disconnect: true,
            ..Self::json(Value::Null)
        }
    }
}
#[derive(Debug)]
struct Captured {
    path: String,
    headers: String,
    body: Value,
}

fn captured_header<'a>(request: &'a Captured, name: &str) -> Option<&'a str> {
    let mut values = request.headers.lines().skip(1).filter_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    });
    let value = values.next();
    assert!(values.next().is_none(), "Duplicate fixture header: {name}");
    value
}

fn assert_native_json_post(request: &Captured, path: &str) {
    assert_eq!(
        request.headers.lines().next(),
        Some(format!("POST {path} HTTP/1.1").as_str())
    );
    assert_eq!(request.path, path);
    // This is the public fixed token from the original Swift fixtures, sent
    // exclusively to the owned loopback HTTP listener.
    assert_eq!(
        captured_header(request, "Authorization"),
        Some("Bearer test-token")
    );
    assert_eq!(
        captured_header(request, "Content-Type"),
        Some("application/json")
    );
}

/// A real local HTTP fixture. It receives actual reqwest requests but never contacts a provider.
fn server(replies: Vec<Reply>) -> (Url, mpsc::Receiver<Captured>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for reply in replies {
            let start = Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(10) =>
                    {
                        thread::sleep(Duration::from_millis(2))
                    }
                    _ => return,
                }
            };
            // BSD sockets may inherit O_NONBLOCK from the listening socket.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut data = Vec::new();
            let headers_end;
            loop {
                let mut byte = [0u8];
                if socket.read_exact(&mut byte).is_err() {
                    return;
                }
                data.push(byte[0]);
                if data.ends_with(b"\r\n\r\n") {
                    headers_end = data.len();
                    break;
                }
                if data.len() > 32_000 {
                    return;
                }
            }
            let headers = String::from_utf8(data[..headers_end].to_vec()).unwrap();
            let count: usize = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, value)| value.trim().parse().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0; count];
            if socket.read_exact(&mut body).is_err() {
                return;
            }
            let path = headers
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_owned();
            let _ = tx.send(Captured {
                path,
                headers,
                body: serde_json::from_slice(&body).unwrap_or(Value::Null),
            });
            if reply.disconnect {
                // Drop this owned socket before any response bytes, but retain
                // the listener for the next reply/sentinel. This is a genuine
                // transport failure, not a stopped listener or an HTTP status.
                continue;
            }
            thread::sleep(reply.delay);
            let response = format!(
                "HTTP/1.1 {} Fixture\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                reply.status,
                reply.mime,
                reply.body.len(),
                reply.extra,
                reply.body
            );
            let _ = socket.write_all(response.as_bytes());
        }
    });
    (Url::parse(&format!("http://{address}/")).unwrap(), rx)
}

fn stop_listening_fixture(base: &Url, captured: &mpsc::Receiver<Captured>) {
    // A live listener, then an explicit non-provider request, proves both that
    // the observation window was real and that its worker terminates boundedly.
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], base.port().unwrap()));
    let mut socket =
        std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .write_all(
            b"GET /owned-fixture-stop HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        )
        .unwrap();
    let mut response = Vec::new();
    socket.take(4096).read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 Fixture\r\n"));
    let sentinel = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(sentinel.path, "/owned-fixture-stop");
    assert_eq!(
        sentinel.headers.lines().next(),
        Some("GET /owned-fixture-stop HTTP/1.1")
    );
    assert!(matches!(
        captured.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn local_address_rejects_unsafe_or_ambiguous_destinations() {
    for address in [
        "http://example.com",
        "https://ollama.com",
        "https://a.ollama.com",
        "https://user:secret@example.com",
        "https://example.com/path",
        "https://example.com/?x=1",
        "https://example.com/#x",
        "file:///tmp/model",
    ] {
        assert!(
            LocalAIConfiguration::new(address).validate(false).is_err(),
            "{address}"
        );
    }
    let mut config = LocalAIConfiguration::new("http://127.0.0.1:11436");
    assert!(config.validate(false).is_ok());
    assert!(config.validate(true).is_err());
    config.same_device = true;
    assert!(config.validate(true).is_ok());
    config.address = "https://server.example".into();
    assert!(config.validate(false).is_err());
    config.same_device = false;
    config.model = "some-cloud-model".into();
    assert!(config.validate(false).is_err());
}

#[tokio::test]
async fn local_offline_pipeline_rejects_an_unasserted_loopback_tunnel_before_any_http() {
    let (base, captured) = server(vec![Reply::json(json!({"fixture":"finished"}))]);
    let writer = OllamaProvider::new(LocalAIConfiguration {
        address: base.to_string(),
        same_device: false,
        ..LocalAIConfiguration::default()
    });
    let checker = Arc::new(CloudCheckCounter::default());
    let input = RequestInput::new(
        SelectionSnapshot::new("The treatment may help.", "Test", "test"),
        ReadingAction::Simplify,
    );
    let (events, mut received) = unbounded_channel();
    let error = Pipeline::new(Arc::new(writer), Some(checker.clone()), true)
        .run(input, events, CancellationToken::new())
        .await
        .expect_err("Loopback alone must not assert same-device inference");
    assert!(error.to_string().contains("Offline mode blocks inference"));
    assert_eq!(checker.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(received.try_recv().is_err());
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    // The still-live recipient must receive only this explicit stop request,
    // not metadata or a chat attempt hidden behind a closed listener.
    stop_listening_fixture(&base, &captured);
}

#[test]
fn line_framing_handles_unicode_boundaries_and_rejects_bad_text() {
    let mut lines = Lines::default();
    let mut seen = vec![];
    let text = "café\r\n次\n".as_bytes();
    for byte in text {
        lines
            .push(&[*byte], |line| {
                seen.push(line.to_owned());
                Ok(())
            })
            .unwrap();
    }
    lines.finish(|_| Ok(())).unwrap();
    assert_eq!(seen, ["café", "次"]);
    assert!(Lines::default().push(&[255, b'\n'], |_| Ok(())).is_err());
    assert!(
        Lines::default()
            .push(&vec![b'a'; 65_537], |_| Ok(()))
            .is_err()
    );
    assert!(
        Lines::default()
            .push(&vec![b'\n'; 256_001], |_| Ok(()))
            .is_err()
    );
}

#[test]
fn sse_supports_both_contracts_and_requires_a_terminator() {
    let mut stream = Sse::default();
    assert!(stream.line(": heartbeat").unwrap().is_none());
    stream.line("data: {\"response\":\"Some \"}").unwrap();
    assert_eq!(stream.line("").unwrap().unwrap(), "Some ");
    stream.line("data: {\"choices\":[{\"delta\":{\"content\":\"benefit\"}}],\"model\":\"resolved\",\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":2}}").unwrap();
    stream.line("").unwrap();
    stream.line("data: [DONE]").unwrap();
    let result = stream.finish("fallback").unwrap();
    assert_eq!(result.text, "Some benefit");
    assert_eq!(result.model, "resolved");
    assert_eq!(result.usage.unwrap().input_tokens, Some(8));
    // Original OpenAI-only case, without an earlier native-response delta.
    let mut openai = Sse::default();
    openai
        .line(r#"data: {"choices":[{"delta":{"content":"An answer."}}]}"#)
        .unwrap();
    openai.line("").unwrap();
    openai.line("data: [DONE]").unwrap();
    assert_eq!(openai.finish("test").unwrap().text, "An answer.");
    let mut incomplete = Sse::default();
    incomplete
        .line("data: {\"response\":\"Incomplete\"}")
        .unwrap();
    assert!(incomplete.finish("model").is_err());
}

#[test]
fn sse_errors_malformed_oversized_and_length_limited_events_fail() {
    for value in [
        json!({"error":"private source echoed here"}),
        json!({"response":"x".repeat(12_001)}),
        json!({"choices":[{"finish_reason":"length"}]}),
        json!({"response":42}),
        json!({"choices":[{"delta":{"content":42}}]}),
    ] {
        let mut stream = Sse::default();
        stream.line(&format!("data: {value}")).unwrap();
        let error = stream.line("").unwrap_err().to_string();
        assert!(!error.contains("private source"));
    }
    let mut malformed = Sse::default();
    malformed.line("data: nope").unwrap();
    assert!(malformed.line("").is_err());
}

#[test]
fn ndjson_requires_completion_and_rejects_length_and_trailing_data() {
    let mut stream = Ndjson::default();
    stream
        .line(&json!({"done":false,"message":{"content":"Maybe"}}).to_string())
        .unwrap();
    assert!(stream.finish("local").is_err());
    for value in [
        json!({"done":true,"done_reason":"length","message":{"content":"Partial"}}),
        json!({"error":"private"}),
        json!({"message":{"content":"No marker"}}),
        json!({"done":true,"message":{"content":42}}),
    ] {
        assert!(Ndjson::default().line(&value.to_string()).is_err());
    }
    let mut done = Ndjson::default();
    done.line(&json!({"done":true,"message":{"content":"Answer"}}).to_string())
        .unwrap();
    assert!(done.line("{}").is_err());
}

#[test]
fn local_budget_preserves_source_and_only_excerpts_supplemental_context() {
    let mut input = RequestInput::new(
        SelectionSnapshot::new("Keep this source intact.", "Fixture", "fixture"),
        ReadingAction::Simplify,
    );
    input.context = "User added source context.".into();
    input.question = "Keep this question?".into();
    input.page_context = "supplemental 日本語 ".repeat(2000);
    input.previous_result = "history \"\\\n".repeat(2000);
    let expected_source =
        "Keep this source intact.\n\nUser-supplied context:\nUser added source context.";
    assert_eq!(input.source(), expected_source);
    let mut original = prompt();
    original.local_input = Some(input);
    let result = fit(original).unwrap();
    let fields: Value = serde_json::from_str(&result.user).unwrap();
    assert_eq!(fields["source"], expected_source);
    assert_eq!(fields["follow_up_question"], "Keep this question?");
    assert!(result.system.len() + result.user.len() <= LOCAL_PROMPT_BYTES);
    assert!(
        fields["page_OCR_may_contain_errors"]
            .as_str()
            .unwrap()
            .len()
            <= 1400
    );
    let mut oversized = prompt();
    oversized.user = "x".repeat(LOCAL_PROMPT_BYTES);
    assert!(fit(oversized).is_err());
    let input = RequestInput::new(
        SelectionSnapshot::new("x".repeat(7000), "Fixture", "fixture"),
        ReadingAction::Simplify,
    );
    let mut oversized = prompt();
    oversized.local_input = Some(input);
    assert!(fit(oversized).is_err());
}

fn timed_reference(text: String) -> WebReference {
    WebReference {
        url: "https://example.com/video?source=owned-fixture".into(),
        title: "Café shuttle video".into(),
        kind: "video captions".into(),
        text,
        timestamp: Some(86.25),
    }
}

#[test]
fn local_reference_omits_atomic_metadata_when_only_primary_source_fits() {
    let mut input = RequestInput::new(
        SelectionSnapshot::new("Preserve Café ", "Fixture", "fixture"),
        ReadingAction::Simplify,
    );
    input.context = "Preserve manually supplied 日本語 context.".into();
    let baseline = fit(pickle_core::make_prompt(&input, None, None).unwrap()).unwrap();
    let spare = LOCAL_PROMPT_BYTES - baseline.system.len() - baseline.user.len();
    input.snapshot.text.push_str(&"é".repeat((spare - 120) / 2));
    input.reference = Some(timed_reference("[1:26] Supplementary caption.".into()));
    let expected_source = input.source();
    let original = pickle_core::make_prompt(&input, None, None).unwrap();
    let fitted = fit(original).unwrap();
    let fields: Value = serde_json::from_str(&fitted.user).unwrap();
    assert_eq!(fields["source"], expected_source);
    assert!(fields.get("reference_excerpt").is_none());
    assert!(fitted.system.len() + fitted.user.len() <= LOCAL_PROMPT_BYTES);

    // References cannot make an oversized primary passage fit by cutting it.
    input.snapshot.text.push_str(&"é".repeat(100));
    assert!(fit(pickle_core::make_prompt(&input, None, None).unwrap()).is_err());
}

#[tokio::test]
async fn cloudflare_stream_uses_real_http_and_delivers_cumulative_drafts() {
    let (base, captured) = server(vec![Reply::stream(
        "data: {\"response\":\"May \"}\n\ndata: {\"response\":\"help\",\"model\":\"actual\"}\n\ndata: [DONE]\n\n",
    )]);
    let provider =
        CloudflareProvider::new(ACCOUNT, "fixture-key", "@cf/meta/small").with_test_base(base);
    let (tx, mut rx) = unbounded_channel();
    let result = provider
        .generate(prompt(), tx, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.text, "May help");
    assert_eq!(result.model, "actual");
    let first = rx.try_recv().unwrap();
    assert!(matches!(first, PipelineEvent::Draft(value) if value == "May "));
    let request = captured.recv().unwrap();
    assert!(request.path.ends_with("/ai/run/@cf/meta/small"));
    assert!(
        request
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer fixture-key")
    );
    assert_eq!(request.body["stream"], true);
    assert_eq!(request.body["messages"][0]["role"], "system");
}

#[tokio::test]
async fn legacy_cloudflare_sse_pipeline_preserves_exact_unicode_drafts_and_stream_contract() {
    // Original StreamingTests.testStreamingProviderAndPipeline event payloads,
    // framed as SSE over actual owned HTTP, including keepalive and terminal.
    const MODEL: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";
    let (base, captured) = server(vec![Reply::stream(concat!(
        ": keepalive\n\n",
        "data: {\"response\":\"Hello \"}\n\n",
        "data: {\"response\":\"world 🌍\",\"usage\":{\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    ))]);
    let account = "a".repeat(32);
    let writer = CloudflareProvider::new(&account, "test-token", MODEL).with_test_base(base);
    let input = RequestInput::new(
        SelectionSnapshot::new("Hello world", "Test", "test"),
        ReadingAction::Simplify,
    );
    let (events, mut received) = unbounded_channel();
    let outcome = Pipeline::new(Arc::new(writer), None, false)
        .run(input, events, CancellationToken::new())
        .await
        .unwrap();
    let PipelineOutcome::Result(result) = outcome else {
        panic!("Expected the original SSE prose result")
    };
    assert_eq!(result.text, "Hello world 🌍");
    assert_eq!(result.model, MODEL);
    assert!(result.chart.is_none());
    assert_eq!(result.repairs, 0);
    let mut drafts = vec![];
    while let Ok(event) = received.try_recv() {
        if let PipelineEvent::Draft(text) = event {
            drafts.push(text);
        }
    }
    assert_eq!(drafts, ["Hello ", "Hello world 🌍"]);
    let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_native_json_post(&request, &format!("/accounts/{account}/ai/run/{MODEL}"));
    assert_eq!(
        captured_header(&request, "Accept"),
        Some("text/event-stream")
    );
    assert_eq!(request.body["stream"], true);
    assert!(request.body.get("response_format").is_none());
    let user: Value =
        serde_json::from_str(request.body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(user["source"], "Hello world");
    assert!(matches!(
        captured.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[tokio::test]
async fn legacy_cloudflare_sse_pipeline_refuses_incomplete_malformed_and_empty_done() {
    // Original testIncompleteAndInvalidStreamsFail, now across actual HTTP and
    // the real Pipeline: a valid partial event is not a completed answer; a
    // malformed event and an empty-but-terminated stream cannot become results.
    const MODEL: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";
    for (body, expected_drafts, reason) in [
        (
            "data: {\"response\":\"Partial\"}\n\n",
            vec!["Partial"],
            "before the answer finished",
        ),
        ("data: not-json\n\n", vec![], "invalid event"),
        ("data: [DONE]\n\n", vec![], "empty or oversized"),
    ] {
        let (base, captured) = server(vec![Reply::stream(body)]);
        let account = "a".repeat(32);
        let writer = CloudflareProvider::new(&account, "test-token", MODEL).with_test_base(base);
        let input = RequestInput::new(
            SelectionSnapshot::new("Hello world", "Test", "test"),
            ReadingAction::Simplify,
        );
        let (events, mut received) = unbounded_channel();
        let error = Pipeline::new(Arc::new(writer), None, false)
            .run(input, events, CancellationToken::new())
            .await
            .expect_err("A partial, malformed, or empty stream must not complete a reading");
        assert!(
            error.to_string().contains(reason),
            "Expected decoder refusal: {reason}"
        );
        let mut drafts = vec![];
        while let Ok(event) = received.try_recv() {
            if let PipelineEvent::Draft(text) = event {
                drafts.push(text);
            }
        }
        assert_eq!(drafts, expected_drafts);
        let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_native_json_post(&request, &format!("/accounts/{account}/ai/run/{MODEL}"));
        assert_eq!(
            captured_header(&request, "Accept"),
            Some("text/event-stream")
        );
        assert_eq!(request.body["stream"], true);
        assert!(matches!(
            captured.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}

#[tokio::test]
async fn chart_requests_use_schema_capable_cloud_model_and_buffer_objects() {
    let (base, captured) = server(vec![Reply::json(
        json!({"success":true,"result":{"response":{"bars":[]},"model":"json-resolved"}}),
    )]);
    let mut input = prompt();
    input.structured = true;
    input.schema = Some(json!({"type":"object"}));
    let provider =
        CloudflareProvider::new(ACCOUNT, "fixture-key", "@cf/meta/small").with_test_base(base);
    let (tx, _) = unbounded_channel();
    let result = provider
        .generate(input, tx, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result.text).unwrap(),
        json!({"bars":[]})
    );
    let request = captured.recv().unwrap();
    assert!(request.path.ends_with(CloudflareProvider::DIAGRAM_MODEL));
    assert_eq!(request.body["stream"], false);
    assert_eq!(request.body["response_format"]["type"], "json_schema");
}

#[tokio::test]
async fn legacy_structured_cloudflare_object_crosses_http_and_pipeline_with_source_bars() {
    // Exact source, selected small prose model, and response-object shape from
    // TransportTests.testStructuredCloudflareResponseObjectAndSchema. Only the
    // HTTP origin is replaced with an owned loopback listener; real prompt
    // construction, Cloudflare decoding and deterministic chart validation run.
    const SOURCE: &str = "$10 and $15";
    const SMALL: &str = "@cf/meta/llama-3.2-1b-instruct";
    const DIAGRAM: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";
    let evidence = pickle_core::numeric_evidence(SOURCE);
    assert_eq!(evidence.len(), 2);
    assert_eq!(evidence[0].id, "n0");
    assert_eq!(evidence[1].id, "n8");
    let (base, captured) = server(vec![Reply::json(json!({
        "success":true,
        "result":{"response":{"type":"bar_chart","bars":[
            {"evidenceID":evidence[0].id},
            {"evidenceID":evidence[1].id}
        ]}}
    }))]);
    let account = "a".repeat(32);
    // Leave the ordinary streaming preference enabled: structured charts must
    // still use the buffered object contract rather than expose JSON drafts.
    let writer = CloudflareProvider::new(&account, "test-token", SMALL).with_test_base(base);
    let pipeline = Pipeline::new(Arc::new(writer), None, false);
    let mut input = RequestInput::new(
        SelectionSnapshot::new(SOURCE, "Test", "test"),
        ReadingAction::Chart,
    );
    input.chart_choice = Some(pickle_core::ChartKind::Bar);
    let (events, mut received) = unbounded_channel();
    let outcome = pipeline
        .run(input, events, CancellationToken::new())
        .await
        .unwrap();
    let PipelineOutcome::Result(result) = outcome else {
        panic!("The original response object must produce a validated bar chart")
    };
    let Some(pickle_core::ValidatedChart::Bar(bars)) = &result.chart else {
        panic!("Expected source-backed bars, not raw JSON or a refusal")
    };
    assert_eq!(
        bars.iter().map(|bar| bar.value).collect::<Vec<_>>(),
        [10.0, 15.0]
    );
    assert_eq!(
        bars.iter().map(|bar| bar.unit.as_str()).collect::<Vec<_>>(),
        ["USD", "USD"]
    );
    assert_eq!(
        bars.iter()
            .map(|bar| bar.quote.as_str())
            .collect::<Vec<_>>(),
        ["$10", "$15"]
    );
    assert_eq!(
        result.text,
        "Quantities in your selection\n$10 and $15: 10 USD\n$10 and $15: 15 USD"
    );
    assert_eq!(result.action, ReadingAction::Chart);
    assert_eq!(result.quality, pickle_core::QualityStatus::ChartValidated);
    assert_eq!(result.repairs, 0);
    assert_eq!(CloudflareProvider::DIAGRAM_MODEL, DIAGRAM);
    assert_eq!(result.model, DIAGRAM);
    while let Ok(event) = received.try_recv() {
        assert!(
            !matches!(event, PipelineEvent::Draft(_)),
            "Chart JSON must not stream into the prose draft"
        );
    }

    let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_native_json_post(&request, &format!("/accounts/{account}/ai/run/{DIAGRAM}"));
    assert!(!request.path.ends_with(SMALL));
    assert_eq!(request.body["stream"], false);
    assert_eq!(request.body["max_tokens"], 1800);
    assert_eq!(request.body["response_format"]["type"], "json_schema");
    let schema = &request.body["response_format"]["json_schema"];
    assert!(
        schema.is_object(),
        "The actual chart schema must reach HTTP"
    );
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["type"]["enum"], json!(["bar_chart"]));
    assert_eq!(schema["properties"]["bars"]["minItems"], 2);
    assert_eq!(
        schema["properties"]["bars"]["items"]["properties"]["evidenceID"]["enum"],
        json!(["n0", "n8"])
    );
    assert_eq!(request.body["messages"][0]["role"], "system");
    assert_eq!(request.body["messages"][1]["role"], "user");
    let user: Value =
        serde_json::from_str(request.body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(user["source"], SOURCE);
    assert!(matches!(
        captured.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[tokio::test]
async fn invalid_cloud_accounts_send_no_writer_or_jev_requests() {
    // Keep an actual listening fixture alive: an implementation that attempted
    // either request would be observed, rather than merely failing DNS/connect.
    let (base, captured) = server(vec![Reply::json(json!({"fixture":"finished"}))]);
    let provider = CloudflareProvider::new(
        "../elsewhere",
        "public-fake-credential",
        CloudflareProvider::DEFAULT_MODEL,
    )
    .with_test_base(base.clone());
    let (tx, _) = unbounded_channel();
    assert!(
        provider
            .generate(prompt(), tx, CancellationToken::new())
            .await
            .is_err()
    );
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let client =
        JevClient::new("../elsewhere", "public-fake-credential").with_test_base(base.clone());
    assert!(
        client
            .evaluate(
                BTreeMap::from([("source".into(), "Fixed source".into())]),
                BTreeMap::from([(
                    "q".into(),
                    DecisionQuestion::noul("Does source mention the sky?")
                )]),
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    // Finish the fixture deterministically. The only received HTTP request must
    // be this explicit sentinel; no detached ten-second listener is left over.
    stop_listening_fixture(&base, &captured);
}

#[tokio::test]
async fn legacy_provider_initialization_sends_no_writer_or_jev_http_requests() {
    // Original TransportTests.testProviderInitializationDoesNotSendAnything,
    // with an actual listening loopback recipient instead of a mock counter.
    // The base override follows new(): this observes configured idle lifetime.
    // The default constructors are separately inspected field-only literals;
    // this listener cannot observe a hypothetical send to the default origin.
    let (base, captured) = server(vec![Reply::json(json!({"fixture":"finished"}))]);
    let writer = CloudflareProvider::new(
        ACCOUNT,
        "public-fake-credential",
        CloudflareProvider::DEFAULT_MODEL,
    )
    .with_test_base(base.clone())
    .with_streaming(false);
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let client = JevClient::new(ACCOUNT, "public-fake-credential").with_test_base(base.clone());
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    // No generate/evaluate call: construction, configuration and release alone
    // must neither send data nor turn settings into a billable operation.
    drop((writer, client));
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    stop_listening_fixture(&base, &captured);
}

// Exact fixtures from Tests/PickleCoreTests/ValidationTests.swift. These assert
// decoded values, not just that the compatibility branches happen to run.
#[test]
fn legacy_jev_cloudflare_envelope_and_direct_model_payload() {
    const PAYLOAD: &str = r#"{"model":"jev-1.13.0","answers":{"q":{"type":"noul","noul":0.9}},"usage":{"input_tokens":10,"output_tokens":1}}"#;
    let direct = JevClient::decode(serde_json::from_str(PAYLOAD).unwrap()).unwrap();
    let envelope = JevClient::decode(
        serde_json::from_str(&format!(r#"{{"success":true,"result":{PAYLOAD}}}"#)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&direct).unwrap(),
        serde_json::to_value(&envelope).unwrap()
    );
    for result in [&direct, &envelope] {
        assert_eq!(result.model, "jev-1.13.0");
        assert_eq!(result.answers.len(), 1);
        assert_eq!(result.answers["q"].kind, "noul");
        assert_eq!(result.answers["q"].noul, Some(0.9));
        let usage = result.usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(1));
    }
    assert!(
        JevClient::decode(serde_json::from_str(r#"{"success":false,"result":null}"#).unwrap())
            .is_err()
    );
}

#[test]
fn legacy_jev_completed_gateway_job() {
    const BODY: &str = r#"{"success":true,"result":{"state":"Completed","result":{"model":"jev-1.13.0","answers":{"sample":{"type":"noul","noul":1}},"usage":{"input_tokens":283,"output_tokens":20}},"gatewayMetadata":{"keySource":"Unified"}}}"#;
    let result = JevClient::decode(serde_json::from_str(BODY).unwrap()).unwrap();
    result
        .validate(&BTreeMap::from([(
            "sample".into(),
            DecisionQuestion::noul("Does source mention the sky?"),
        )]))
        .unwrap();
    assert_eq!(result.model, "jev-1.13.0");
    assert_eq!(result.answers.len(), 1);
    assert_eq!(result.answers["sample"].kind, "noul");
    assert_eq!(result.answers["sample"].noul, Some(1.0));
    let usage = result.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(283));
    assert_eq!(usage.output_tokens, Some(20));
}

#[test]
fn legacy_jev_incomplete_gateway_job_is_not_an_answer() {
    for body in [
        r#"{"success":true,"result":{"state":"Running","result":null}}"#,
        r#"{"success":true,"result":{"state":"Completed","result":null}}"#,
        r#"{"success":false,"result":{"state":"Completed","result":{"model":"jev","answers":{}}}}"#,
    ] {
        assert!(
            JevClient::decode(serde_json::from_str(body).unwrap()).is_err(),
            "{body}"
        );
    }
}

#[test]
fn legacy_cloudflare_usage_field_aliases() {
    // The first spelling is the exact original Swift fixture; the second
    // confirms that accepting aliases preserves the ordinary wire spelling.
    for body in [
        r#"{"prompt_tokens":67,"completion_tokens":25}"#,
        r#"{"input_tokens":67,"output_tokens":25}"#,
    ] {
        let usage: Usage = serde_json::from_str(body).unwrap();
        assert_eq!(usage.input_tokens, Some(67));
        assert_eq!(usage.output_tokens, Some(25));
    }
}

#[tokio::test]
async fn jev_completed_envelope_validates_answers_and_native_contract() {
    let (base, captured) = server(vec![Reply::json(
        json!({"success":true,"result":{"state":"Completed","result":{"model":"jev-test","answers":{"sample":{"type":"noul","noul":0.1}}}}}),
    )]);
    let client = JevClient::new(ACCOUNT, "fixture-key").with_test_base(base);
    let questions = BTreeMap::from([(
        "sample".into(),
        DecisionQuestion::noul("Is source unsupported?"),
    )]);
    let response = client
        .evaluate(
            BTreeMap::from([("source".into(), "Fixed source".into())]),
            questions,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(response.model, "jev-test");
    let request = captured.recv().unwrap();
    assert!(request.path.ends_with("/ai/run"));
    assert_eq!(request.body["model"], "typesafe/jev");
    assert_eq!(request.body["input"]["questions"]["sample"]["type"], "noul");
    assert!(JevClient::decode(json!({"success":true,"result":{"state":"Running"}})).is_err());
}

#[tokio::test]
async fn legacy_jev_native_http_contract_preserves_method_authorization_and_input() {
    // Exact response from testJevUsesCloudflareNativeEvaluationContract.
    const RESPONSE: &str = r#"{"result":{"model":"jev-1.13.0","answers":{"flag":{"type":"noul","noul":0.9}},"usage":{"input_tokens":10,"output_tokens":1}},"success":true}"#;
    let (base, captured) = server(vec![Reply::json(serde_json::from_str(RESPONSE).unwrap())]);
    let account = "a".repeat(32);
    let client = JevClient::new(&account, "test-token").with_test_base(base);
    let state = BTreeMap::from([("source".into(), "Sample".into())]);
    let questions = BTreeMap::from([("flag".into(), DecisionQuestion::noul("Question?"))]);
    let result = client
        .evaluate(state.clone(), questions.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.model, "jev-1.13.0");
    assert_eq!(result.answers["flag"].noul, Some(0.9));
    let usage = result.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(1));
    let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_native_json_post(&request, &format!("/accounts/{account}/ai/run"));
    assert_eq!(request.body["model"], "typesafe/jev");
    assert_eq!(request.body["input"]["state"], json!({"source":"Sample"}));
    assert_eq!(request.body["input"]["questions"]["flag"]["type"], "noul");
    assert_eq!(
        request.body,
        json!({"model":"typesafe/jev","input":{"state":state,"questions":questions}})
    );
    assert!(request.body.get("messages").is_none());
    assert!(request.body["input"].get("messages").is_none());
    assert!(matches!(
        captured.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[tokio::test]
async fn legacy_buffered_llama_http_preserves_reported_model_and_usage() {
    // Exact response from testLlamaNativeEnvelopeAndReportedModelUsage. Run
    // both Generation (where usage lives) and the actual Pipeline with it.
    const RESPONSE: &str = r#"{"success":true,"result":{"response":"Sample response","model":"resolved-model","usage":{"prompt_tokens":67,"completion_tokens":25}}}"#;
    const DEFAULT: &str = "@cf/meta/llama-3.3-70b-instruct-fp8-fast";
    assert_eq!(CloudflareProvider::DEFAULT_MODEL, DEFAULT);
    let (base, captured) = server(vec![
        Reply::json(serde_json::from_str(RESPONSE).unwrap()),
        Reply::json(serde_json::from_str(RESPONSE).unwrap()),
    ]);
    let account = "a".repeat(32);
    let provider =
        CloudflareProvider::new(&account, "test-token", CloudflareProvider::DEFAULT_MODEL)
            .with_streaming(false)
            .with_test_base(base);
    let input = RequestInput::new(
        SelectionSnapshot::new("Sample", "Test", "test"),
        ReadingAction::Simplify,
    );
    let expected_prompt = pickle_core::make_prompt(&input, None, None).unwrap();
    let (tx, mut drafts) = unbounded_channel();
    let generated = provider
        .generate(expected_prompt.clone(), tx, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(generated.text, "Sample response");
    assert_eq!(generated.model, "resolved-model");
    let usage = generated.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(67));
    assert_eq!(usage.output_tokens, Some(25));
    assert!(drafts.try_recv().is_err());
    let pipeline = Pipeline::new(Arc::new(provider), None, false);
    let (tx, _) = unbounded_channel();
    let outcome = pipeline
        .run(input, tx, CancellationToken::new())
        .await
        .unwrap();
    let PipelineOutcome::Result(result) = outcome else {
        panic!("Expected the original buffered prose result")
    };
    assert_eq!(result.text, "Sample response");
    assert_eq!(result.model, "resolved-model");
    assert!(result.chart.is_none());
    for _ in 0..2 {
        let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_native_json_post(&request, &format!("/accounts/{account}/ai/run/{DEFAULT}"));
        assert_eq!(request.body["stream"], false);
        assert_eq!(request.body["max_tokens"].as_u64(), Some(1800));
        assert!(request.body.get("response_format").is_none());
        assert_eq!(
            request.body["messages"],
            json!([
                {"role":"system","content":expected_prompt.system},
                {"role":"user","content":expected_prompt.user},
            ])
        );
    }
    assert!(matches!(
        captured.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[tokio::test]
async fn legacy_small_writing_model_buffered_prose_http_preserves_selected_endpoint() {
    // Exact model, response, and source from testSmallWritingModelIsPreservedForProse.
    const MODEL: &str = "@cf/meta/llama-3.2-1b-instruct";
    const RESPONSE: &str = r#"{"success":true,"result":{"response":"A short explanation."}}"#;
    let (base, captured) = server(vec![Reply::json(serde_json::from_str(RESPONSE).unwrap())]);
    let account = "a".repeat(32);
    let provider = CloudflareProvider::new(&account, "test-token", MODEL)
        .with_streaming(false)
        .with_test_base(base);
    let pipeline = Pipeline::new(Arc::new(provider), None, false);
    let input = RequestInput::new(
        SelectionSnapshot::new("Rainwater is collected in a tank.", "Test", "test"),
        ReadingAction::Simplify,
    );
    let (tx, _) = unbounded_channel();
    let outcome = pipeline
        .run(input, tx, CancellationToken::new())
        .await
        .unwrap();
    let PipelineOutcome::Result(result) = outcome else {
        panic!("Expected the original small-model prose result")
    };
    assert_eq!(result.text, "A short explanation.");
    assert_eq!(result.model, MODEL);
    assert!(result.chart.is_none());
    let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_native_json_post(&request, &format!("/accounts/{account}/ai/run/{MODEL}"));
    assert_eq!(request.body["stream"], false);
    assert_eq!(request.body["max_tokens"].as_u64(), Some(1800));
    assert!(request.body.get("response_format").is_none());
    assert!(matches!(
        captured.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[tokio::test]
async fn local_generation_verifies_metadata_before_streaming() {
    let mut stream = Reply::stream(
        "{\"message\":{\"content\":\"Might help\"},\"done\":false}\n{\"done\":true,\"eval_count\":2,\"prompt_eval_count\":8,\"load_duration\":1000000}\n",
    );
    stream.mime = "application/x-ndjson";
    let (base, captured) = server(vec![
        Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
        stream,
    ]);
    let provider = OllamaProvider::new(LocalAIConfiguration {
        address: base.to_string(),
        model: "llama3.2:1b".into(),
        token: "local-key".into(),
        same_device: true,
    });
    let (tx, _) = unbounded_channel();
    let result = provider
        .generate(prompt(), tx, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.text, "Might help");
    assert_eq!(result.usage.unwrap().input_tokens, Some(8));
    assert!(result.timing.unwrap().load_seconds > 0.0);
    assert_eq!(captured.recv().unwrap().path, "/api/show");
    let request = captured.recv().unwrap();
    assert_eq!(request.path, "/api/chat");
    assert_eq!(request.body["options"]["num_ctx"], 8192);
    assert_eq!(request.body["options"]["num_predict"], 1024);
    assert_eq!(request.body["keep_alive"], "2m");
}

#[tokio::test]
async fn malformed_nested_stream_envelopes_never_complete_partial_http_answers() {
    let input = || {
        RequestInput::new(
            SelectionSnapshot::new("Treatment may help some people.", "Fixture", "fixture"),
            ReadingAction::Simplify,
        )
    };
    for local in [true, false] {
        let malformed = if local {
            vec![
                json!({"done":true,"message":false}),
                json!({"done":true,"message":["malformed-private-marker"]}),
                json!({"done":true,"model":{ "private":"malformed-private-marker"}}),
                json!({"done":true,"done_reason":false}),
                json!({"done":true,"eval_count":"malformed-private-marker"}),
                json!({"done":true,"load_duration":"malformed-private-marker"}),
            ]
        } else {
            vec![
                json!({"choices":[false]}),
                json!({"choices":[{"delta":false}]}),
                json!({"choices":[{"delta":["malformed-private-marker"]}]}),
                json!({"choices":[{},false]}),
                json!({"choices":[{"finish_reason":false}]}),
                json!({"usage":[8,2]}),
            ]
        };
        for terminal in malformed {
            let mut response = if local {
                Reply::stream(&format!(
                    "{}\n{terminal}\n",
                    json!({"done":false,"message":{"content":"Treatment may"}})
                ))
            } else {
                Reply::stream(&format!(
                    "data: {{\"response\":\"Treatment may\"}}\n\ndata: {terminal}\n\ndata: [DONE]\n\n"
                ))
            };
            let replies = if local {
                response.mime = "application/x-ndjson";
                vec![
                    Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
                    response,
                ]
            } else {
                vec![response]
            };
            let (base, captured) = server(replies);
            let provider: Arc<dyn GenerativeProvider> = if local {
                Arc::new(OllamaProvider::new(LocalAIConfiguration {
                    address: base.to_string(),
                    model: "fixture-local-model".into(),
                    token: String::new(),
                    same_device: true,
                }))
            } else {
                Arc::new(
                    CloudflareProvider::new(ACCOUNT, "fixture-key", "@cf/meta/fixture")
                        .with_test_base(base),
                )
            };
            let (events, mut received) = unbounded_channel();
            let outcome = Pipeline::new(provider, None, local)
                .run(input(), events, CancellationToken::new())
                .await;
            let error = match outcome {
                Err(error) => error,
                Ok(_) => panic!("Malformed terminal completed a partial answer: {terminal}"),
            };
            assert!(!error.to_string().contains("malformed-private-marker"));
            let mut partial_was_received = false;
            while let Ok(event) = received.try_recv() {
                if let PipelineEvent::Draft(text) = event {
                    assert_eq!(text, "Treatment may");
                    partial_was_received = true;
                }
            }
            assert!(
                partial_was_received,
                "The fixture must reach stream decoding."
            );
            if local {
                assert_eq!(captured.recv().unwrap().path, "/api/show");
                assert_eq!(captured.recv().unwrap().path, "/api/chat");
            } else {
                assert!(captured.recv().unwrap().path.ends_with("/@cf/meta/fixture"));
            }
        }
    }
}

#[tokio::test]
async fn optional_stream_fields_and_non_content_events_preserve_http_completion() {
    for local in [true, false] {
        let mut response = if local {
            let chunks = [
                json!({"done":false,"message":null,"model":null,"done_reason":null,"load_duration":null,"eval_count":null}),
                json!({"done":false,"message":{"content":null,"role":"assistant"},"future_metadata":{"anything":[1,true]}}),
                json!({"done":false,"message":{}}),
                json!({"done":false,"message":{"content":"Café may help."}}),
                json!({"done":true,"message":{"content":null},"done_reason":"stop","model":"resolved-local","eval_count":2,"prompt_eval_count":8,"load_duration":1_000_000,"eval_duration":2_000_000}),
            ];
            Reply::stream(&format!(
                "{}\n",
                chunks
                    .iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
        } else {
            Reply::stream(concat!(
                ": heartbeat\n\n",
                "data: {\"response\":null,\"choices\":null,\"model\":null,\"usage\":null}\n\n",
                "data: {\"choices\":[{\"delta\":null,\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":null,\"role\":\"assistant\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"Café may help.\"}}],\"future_metadata\":{\"anything\":[1,true]}}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":2},\"model\":\"resolved-cloud\"}\n\n",
                "data: [DONE]\n\n"
            ))
        };
        let replies = if local {
            response.mime = "application/x-ndjson";
            vec![
                Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
                response,
            ]
        } else {
            vec![response]
        };
        let (base, _) = server(replies);
        let provider: Arc<dyn GenerativeProvider> = if local {
            Arc::new(OllamaProvider::new(LocalAIConfiguration {
                address: base.to_string(),
                model: "fixture-local-model".into(),
                token: String::new(),
                same_device: true,
            }))
        } else {
            Arc::new(
                CloudflareProvider::new(ACCOUNT, "fixture-key", "@cf/meta/fixture")
                    .with_test_base(base),
            )
        };
        let (events, _) = unbounded_channel();
        let result = provider
            .generate(prompt(), events, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.text, "Café may help.");
        assert_eq!(
            result.model,
            if local {
                "resolved-local"
            } else {
                "resolved-cloud"
            }
        );
        let usage = result.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(8));
        assert_eq!(usage.output_tokens, Some(2));
        if local {
            let timing = result.timing.unwrap();
            assert_eq!(timing.output_tokens, 2);
            assert_eq!(timing.load_seconds, 0.001);
            assert_eq!(timing.generation_seconds, 0.002);
        }
    }
}

#[tokio::test]
async fn local_http_preserves_timed_reference_provenance_and_bounds_unicode_excerpts() {
    for pressured in [false, true] {
        for streaming in [false, true] {
            let mut input = RequestInput::new(
                SelectionSnapshot::new(
                    if pressured {
                        "Café shuttle 日本語 \"quoted\" \\\n".repeat(80)
                    } else {
                        "Café shuttle waiting on this route.".into()
                    },
                    "Fixture",
                    "fixture",
                ),
                ReadingAction::Simplify,
            );
            input.context = "Manual source context must remain exact: Café 日本語.".into();
            input.question = "What happens at this moment?".into();
            if pressured {
                input.previous_result = "Prior answer is not source evidence. ".repeat(25);
            }
            let reference_text = if pressured {
                let mut paragraphs = (0..35)
                    .map(|i| {
                        format!(
                            "[{i}:00] Café shuttle waiting 日本語 on this route. {}",
                            "\"\\\\ ".repeat(30)
                        )
                    })
                    .collect::<Vec<_>>();
                paragraphs.push(format!(
                    "OMITTED-TAIL-MARKER {}",
                    "unrelated content ".repeat(150)
                ));
                paragraphs
                    .push("[Available captions only; completeness has not been verified.]".into());
                paragraphs.join("\n")
            } else {
                "[1:20] The Café shuttle is approaching.\n[1:26] Passengers may board.".into()
            };
            let mut reference = timed_reference(reference_text.clone());
            if pressured {
                reference.title = format!(
                    "Café \"title\"\nPlayback time: 999 seconds {}",
                    "日本語 ".repeat(50)
                );
                reference.url = format!(
                    "https://example.com/video?{}",
                    "caption=unicode&".repeat(50)
                );
            }
            reference.validate().unwrap();
            input.reference = Some(reference.clone());
            let expected_source = format!(
                "{}\n\nUser-supplied context:\nManual source context must remain exact: Café 日本語.",
                input.snapshot.text
            );
            let original = pickle_core::make_prompt(&input, None, None).unwrap();
            let expected_system = original.system.clone();
            let complete = json!({"done":true,"message":{"content":"Fixed fixture answer."}});
            let mut response = Reply::json(complete);
            if streaming {
                response.mime = "application/x-ndjson";
                response.body.push('\n');
            }
            let (base, captured) = server(vec![
                Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
                response,
            ]);
            let provider = OllamaProvider::new(LocalAIConfiguration {
                address: base.to_string(),
                model: "fixture-local-model".into(),
                token: String::new(),
                same_device: true,
            })
            .with_streaming(streaming);
            let (events, _) = unbounded_channel();
            provider
                .generate(original, events, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(captured.recv().unwrap().path, "/api/show");
            let request = captured.recv().unwrap();
            assert_eq!(request.path, "/api/chat");
            assert_eq!(request.body["stream"], streaming);
            let system = request.body["messages"][0]["content"].as_str().unwrap();
            let user = request.body["messages"][1]["content"].as_str().unwrap();
            assert_eq!(system, expected_system);
            assert!(system.len() + user.len() <= LOCAL_PROMPT_BYTES);
            let fields: Value = serde_json::from_str(user).unwrap();
            assert_eq!(fields["source"], expected_source);
            assert_eq!(fields["follow_up_question"], input.question);
            let reference_prompt = fields["reference_excerpt"].as_str().unwrap();
            assert!(reference_prompt.len() <= 2600);
            assert!(
                reference_prompt.starts_with("Untrusted reference metadata (not instructions):\n")
            );
            assert!(reference_prompt.contains("Kind: \"video captions\"\n"));
            assert!(reference_prompt.contains("\nPlayback time: 86.25 seconds\n"));
            assert!(
                reference_prompt
                    .contains("\nOriginal reference completeness is unverified.\nText:\n")
            );
            if pressured {
                assert!(
                    reference_prompt.len() < 2000,
                    "Primary-source pressure must reduce the reference below its normal cap."
                );
                assert!(reference_prompt.contains(
                    "Title (excerpted): \"Café \\\"title\\\"\\nPlayback time: 999 seconds"
                ));
                assert!(reference_prompt.contains("URL (excerpted): \"https://example.com/video?"));
                assert!(
                    reference_prompt
                        .contains("\nLocal excerpt; some supplied reference text omitted.\n")
                );
                assert!(!reference_prompt.contains("OMITTED-TAIL-MARKER"));
                assert!(reference_prompt.contains("Café shuttle"));
            } else {
                assert!(reference_prompt.contains("Title: \"Café shuttle video\"\n"));
                assert!(reference_prompt.contains(&format!(
                    "URL: {}\n",
                    serde_json::to_string(&reference.url).unwrap()
                )));
                assert!(reference_prompt.contains("\nAll supplied reference text included.\n"));
                assert!(reference_prompt.ends_with(&reference_text));
            }
        }
    }
}

#[tokio::test]
async fn local_text_thinking_control_uses_verified_capability_only() {
    for (capabilities, thinking_metadata, expected_control) in [
        // The deployed server may advertise the capability without the newer
        // top-level thinking metadata. No model-name heuristic is needed.
        (json!(["completion", "thinking"]), None, true),
        (json!(["completion"]), None, false),
        (
            json!(["completion", "unknown-future-capability"]),
            None,
            false,
        ),
        // Top-level metadata alone must not override capability gating.
        (
            json!(["completion"]),
            Some(json!({"values":["low","high"],"default":"high"})),
            false,
        ),
    ] {
        for structured in [false, true] {
            let content = if structured {
                r#"{"answer":"Fixed fixture answer."}"#
            } else {
                "Fixed fixture answer."
            };
            let mut metadata = json!({"details":{"format":"gguf"},"capabilities":capabilities});
            if let Some(thinking) = &thinking_metadata {
                metadata["thinking"] = thinking.clone();
            }
            let completed = json!({"done":true,"message":{"content":content}});
            let mut reply = Reply::json(completed);
            if !structured {
                reply.mime = "application/x-ndjson";
                reply.body.push('\n');
            }
            let (base, captured) = server(vec![Reply::json(metadata), reply]);
            let model = if expected_control {
                "fixture-local-model"
            } else {
                "thinking-word-in-name-only"
            };
            let provider = OllamaProvider::new(LocalAIConfiguration {
                address: base.to_string(),
                model: model.into(),
                token: String::new(),
                same_device: true,
            });
            let mut request = prompt();
            request.structured = structured;
            let schema = json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"]});
            if structured {
                request.schema = Some(schema.clone());
            }
            let expected_system = request.system.clone();
            let expected_user = request.user.clone();
            let (sender, _) = unbounded_channel();
            let result = provider
                .generate(request, sender, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(result.text, content);
            let show = captured.recv().unwrap();
            assert_eq!(show.path, "/api/show");
            assert_eq!(show.body, json!({"model":model}));
            let chat = captured.recv().unwrap();
            assert_eq!(chat.path, "/api/chat");
            if expected_control {
                assert_eq!(chat.body.get("think"), Some(&json!(false)));
            } else {
                assert!(
                    chat.body.get("think").is_none(),
                    "Non-thinking/unknown capabilities must keep the prior request contract"
                );
            }
            assert_eq!(chat.body["model"], model);
            assert_eq!(
                chat.body["messages"],
                json!([{"role":"system","content":expected_system},{"role":"user","content":expected_user}])
            );
            assert_eq!(chat.body["stream"], !structured);
            assert_eq!(chat.body["keep_alive"], "2m");
            assert_eq!(
                chat.body["options"],
                json!({"num_ctx":8192,"num_predict":1024,"temperature":0.2})
            );
            if structured {
                assert_eq!(chat.body["format"], schema);
            } else {
                assert!(chat.body.get("format").is_none());
            }
            assert!(
                captured.try_recv().is_err(),
                "Capability gating must reuse the existing show response"
            );
        }
    }
}

#[tokio::test]
async fn local_chat_timing_includes_delayed_headers_and_only_streams_report_first_token() {
    let delay = Duration::from_millis(160);
    // Cover ordinary streaming, explicitly buffered prose and forced-buffered
    // structured output through the real HTTP serializer/response reader.
    for (streaming, structured) in [(true, false), (false, false), (true, true)] {
        let content = if structured {
            r#"{"answer":"Fixed fixture answer."}"#
        } else {
            "Fixed fixture answer."
        };
        let complete = json!({"done":true,"model":"llama3.2:1b","message":{"content":content},"eval_count":2,"prompt_eval_count":5,"load_duration":2_000_000,"eval_duration":80_000_000});
        let actual_stream = streaming && !structured;
        let mut reply = if actual_stream {
            let delta = json!({"done":false,"message":{"content":content}});
            let mut final_chunk = complete.clone();
            final_chunk["message"]["content"] = json!("");
            let mut reply = Reply::stream(&format!("{delta}\n{final_chunk}\n"));
            reply.mime = "application/x-ndjson";
            reply
        } else {
            Reply::json(complete)
        };
        // The fixture sleeps before writing ANY headers. The previous timer
        // started after open().await and incorrectly measured near-zero here.
        reply.delay = delay;
        let (base, captured) = server(vec![
            Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
            reply,
        ]);
        let provider = OllamaProvider::new(LocalAIConfiguration {
            address: base.to_string(),
            model: "llama3.2:1b".into(),
            token: String::new(),
            same_device: true,
        })
        .with_streaming(streaming);
        let mut request = prompt();
        request.structured = structured;
        let (sender, _) = unbounded_channel();
        let started = Instant::now();
        let result = provider
            .generate(request, sender, CancellationToken::new())
            .await
            .unwrap();
        let client_elapsed = started.elapsed().as_secs_f64();
        assert_eq!(result.text, content);
        let timing = result.timing.unwrap();
        let minimum = delay.as_secs_f64() * 0.8;
        assert!(
            timing.total_seconds >= minimum,
            "Chat timing excluded the pre-header delay: {timing:?}"
        );
        assert!(timing.total_seconds < 10.0 && timing.total_seconds <= client_elapsed + 0.1);
        if actual_stream {
            let first = timing
                .first_token_seconds
                .expect("Streaming content is observable");
            assert!(
                first >= minimum && first <= timing.total_seconds,
                "Streaming first-token timing excluded pre-header delay: {timing:?}"
            );
        } else {
            assert_eq!(
                timing.first_token_seconds, None,
                "Buffered JSON cannot establish first-token latency"
            );
        }
        assert_eq!(timing.load_seconds, 0.002);
        assert_eq!(timing.generation_seconds, 0.08);
        assert_eq!(timing.output_tokens, 2);
        assert_eq!(captured.recv().unwrap().path, "/api/show");
        let chat = captured.recv().unwrap();
        assert_eq!(chat.path, "/api/chat");
        assert_eq!(chat.body["stream"], actual_stream);
    }
}

#[tokio::test]
async fn cloud_backed_ollama_metadata_prevents_chat() {
    let (base, captured) = server(vec![
        Reply::json(
            json!({"details":{"format":"gguf"},"capabilities":["completion"],"remote_host":"https://ollama.com"}),
        ),
        Reply::json(json!({"fixture":"finished"})),
    ]);
    let provider = OllamaProvider::new(LocalAIConfiguration::new(base.to_string()));
    let (tx, _) = unbounded_channel();
    assert!(
        provider
            .generate(prompt(), tx, CancellationToken::new())
            .await
            .is_err()
    );
    let metadata = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(metadata.path, "/api/show");
    assert_eq!(metadata.body, json!({"model":"llama3.2:1b"}));
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    stop_listening_fixture(&base, &captured);
}

#[tokio::test]
async fn local_chart_uses_buffered_schema_and_never_cloud_checks() {
    let (base, captured) = server(vec![
        Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
        Reply::json(json!({"done":true,"message":{"content":"{\"bars\":[]}"}})),
    ]);
    let provider = OllamaProvider::new(LocalAIConfiguration::new(base.to_string()));
    assert!(!provider.permits_cloud_checks());
    let mut input = prompt();
    input.structured = true;
    input.schema = Some(json!({"type":"object"}));
    let (tx, _) = unbounded_channel();
    provider
        .generate(input, tx, CancellationToken::new())
        .await
        .unwrap();
    captured.recv().unwrap();
    let request = captured.recv().unwrap();
    assert_eq!(request.body["stream"], false);
    assert_eq!(request.body["format"], json!({"type":"object"}));
}

#[derive(Default)]
struct CloudCheckCounter(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl DecisionClient for CloudCheckCounter {
    async fn evaluate(
        &self,
        _: pickle_core::DecisionState,
        _: pickle_core::Questions,
        _: CancellationToken,
    ) -> pickle_core::Result<pickle_core::DecisionResponse> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(pickle_core::PickleError::message(
            "Unexpected fixture cloud check",
        ))
    }
}

#[tokio::test]
async fn legacy_local_chart_http_preserves_local_schema_and_pipeline_validation() {
    // Exact source/response/model from testLocalChartsUseSchemaAndKeepValidation.
    // Only its HTTPS origin is substituted by an owned loopback recipient.
    const SOURCE: &str = "Revenue was $10 million and $15 million.";
    let (base, captured) = server(vec![
        Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
        Reply::json(json!({"message":{"content":"{\"bars\":[]}"},"done":true})),
    ]);
    let writer = OllamaProvider::new(LocalAIConfiguration::new(base.to_string()));
    let checker = Arc::new(CloudCheckCounter::default());
    let mut input = RequestInput::new(
        SelectionSnapshot::new(SOURCE, "Test", "test"),
        ReadingAction::Chart,
    );
    input.chart_choice = Some(pickle_core::ChartKind::Bar);
    let (events, mut received) = unbounded_channel();
    let error = Pipeline::new(Arc::new(writer), Some(checker.clone()), false)
        .run(input, events, CancellationToken::new())
        .await
        .expect_err("A decoded empty-bars candidate must not become a chart/result");
    assert!(error.to_string().contains("invalid chart schema"));
    assert_eq!(checker.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    while let Ok(event) = received.try_recv() {
        assert!(
            !matches!(event, PipelineEvent::Draft(_)),
            "Structured JSON must stay buffered"
        );
    }
    let metadata = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(metadata.path, "/api/show");
    assert_eq!(metadata.body, json!({"model":"llama3.2:1b"}));
    let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(request.path, "/api/chat");
    assert_eq!(request.body["model"], "llama3.2:1b");
    assert_eq!(request.body["stream"], false);
    assert_eq!(request.body["format"]["type"], "object");
    assert_eq!(request.body["format"]["properties"]["bars"]["minItems"], 2);
    assert_eq!(
        request.body["format"]["properties"]["bars"]["items"]["properties"]["evidenceID"]["enum"],
        json!(["n12", "n28"])
    );
    let user: Value =
        serde_json::from_str(request.body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(user["source"], SOURCE);
    assert!(matches!(
        captured.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[tokio::test]
async fn legacy_local_metadata_transport_failure_never_sends_chat_to_a_live_listener() {
    let (base, captured) = server(vec![
        Reply::disconnected(),
        Reply::json(json!({"fixture":"finished"})),
    ]);
    let writer = OllamaProvider::new(LocalAIConfiguration::new(base.to_string()));
    let (events, mut received) = unbounded_channel();
    let error = writer
        .generate(prompt(), events, CancellationToken::new())
        .await
        .expect_err("A metadata socket failure must stop before chat");
    assert!(error.to_string().contains("Can’t reach the Local server"));
    let metadata = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(metadata.path, "/api/show");
    assert_eq!(
        metadata.headers.lines().next(),
        Some("POST /api/show HTTP/1.1")
    );
    assert_eq!(metadata.body, json!({"model":"llama3.2:1b"}));
    assert!(received.try_recv().is_err());
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    // The next request MUST be this sentinel while the listener is live, not
    // a /api/chat attempt hidden by connection refusal after worker teardown.
    stop_listening_fixture(&base, &captured);
}

#[tokio::test]
async fn legacy_local_ndjson_http_refuses_original_invalid_and_unterminated_chunks() {
    // Exact lines from OllamaTests.testOllamaNDJSONCompletionAndErrors.
    for (body, drafts_expected, reason) in [
        ("data: [DONE]\n", vec![], "invalid answer"),
        ("{\n", vec![], "invalid answer"),
        (
            "{\"error\":\"secret source text\"}\n",
            vec![],
            "could not complete",
        ),
        (
            "{\"message\":{\"content\":\"Partial\"},\"done\":true,\"done_reason\":\"length\"}\n",
            vec![],
            "reached its length limit",
        ),
        (
            "{\"message\":{\"content\":\"Partial\"},\"done\":false}\n",
            vec!["Partial"],
            "before the answer finished",
        ),
    ] {
        let mut response = Reply::stream(body);
        response.mime = "application/x-ndjson";
        let (base, captured) = server(vec![
            Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
            response,
        ]);
        let writer = OllamaProvider::new(LocalAIConfiguration::new(base.to_string()));
        let input = RequestInput::new(
            SelectionSnapshot::new("The treatment may help.", "Test", "test"),
            ReadingAction::Simplify,
        );
        let (events, mut received) = unbounded_channel();
        let error = Pipeline::new(Arc::new(writer), None, false)
            .run(input, events, CancellationToken::new())
            .await
            .expect_err("An invalid or unterminated Local stream must not become a result");
        assert!(
            error.to_string().contains(reason),
            "Expected stream decoder refusal: {reason}"
        );
        assert!(!error.to_string().contains("secret source text"));
        let mut drafts = vec![];
        while let Ok(event) = received.try_recv() {
            if let PipelineEvent::Draft(text) = event {
                drafts.push(text);
            }
        }
        assert_eq!(drafts, drafts_expected);
        let metadata = captured.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(metadata.path, "/api/show");
        let chat = captured.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(chat.path, "/api/chat");
        assert_eq!(chat.body["model"], "llama3.2:1b");
        assert_eq!(chat.body["stream"], true);
        assert!(chat.body.get("format").is_none());
        assert!(matches!(
            captured.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}

#[tokio::test]
async fn provider_errors_are_redacted_and_schema_errors_are_specific() {
    let mut reply =
        Reply::json(json!({"errors":[{"code":5025,"message":"private selection and secret"}]}));
    reply.status = 403;
    let (base, _) = server(vec![reply]);
    let provider = CloudflareProvider::new(ACCOUNT, "fixture-key", "@cf/meta/small")
        .with_test_base(base)
        .with_streaming(false);
    let (tx, _) = unbounded_channel();
    let text = provider
        .generate(prompt(), tx, CancellationToken::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(text.contains("structured output"));
    assert!(!text.contains("private selection"));
    assert!(!text.contains("fixture-key"));
}

#[tokio::test]
async fn redirects_and_oversized_responses_are_rejected() {
    let mut redirect = Reply::json(json!({}));
    redirect.status = 302;
    redirect.extra = "Location: http://127.0.0.1:9/leak\r\n".into();
    for reply in [
        redirect,
        Reply::json(json!({"response":"x".repeat(256_001)})),
    ] {
        let (base, _) = server(vec![reply]);
        let provider = CloudflareProvider::new(ACCOUNT, "fixture-key", "@cf/meta/small")
            .with_test_base(base)
            .with_streaming(false);
        let (tx, _) = unbounded_channel();
        assert!(
            provider
                .generate(prompt(), tx, CancellationToken::new())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn cancellation_interrupts_pending_http_without_waiting_for_reply() {
    let mut reply = Reply::json(json!({"success":true,"result":{"response":"Late"}}));
    reply.delay = Duration::from_millis(500);
    let (base, captured) = server(vec![reply]);
    let provider = CloudflareProvider::new(ACCOUNT, "fixture-key", "@cf/meta/small")
        .with_test_base(base)
        .with_streaming(false);
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let (tx, _) = unbounded_channel();
    let task = tokio::spawn(async move { provider.generate(prompt(), tx, task_cancel).await });
    tokio::task::spawn_blocking(move || captured.recv_timeout(Duration::from_secs(2)).unwrap())
        .await
        .unwrap();
    let start = Instant::now();
    cancel.cancel();
    let error = task.await.unwrap().unwrap_err();
    assert!(matches!(error, pickle_core::PickleError::Cancelled));
    assert!(start.elapsed() < Duration::from_millis(200));
}

#[tokio::test]
async fn sample_is_explicitly_labeled_and_network_free() {
    let (tx, _) = unbounded_channel();
    let result = SampleProvider
        .generate(prompt(), tx, CancellationToken::new())
        .await
        .unwrap();
    assert!(result.model.contains("fixed response"));
    assert!(result.text.contains("fixed demonstration"));
    assert!(!SampleProvider.is_remote());
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (tx, _) = unbounded_channel();
    assert!(SampleProvider.generate(prompt(), tx, cancel).await.is_err());
}

#[tokio::test]
async fn visual_analysis_sends_one_bounded_image_only_when_explicitly_invoked() {
    let (base, captured) = server(vec![
        Reply::json(json!({"success":true,"result":{"response":"Two labeled bars are visible."}})),
        Reply::json(json!({"fixture":"finished"})),
    ]);
    let client =
        crate::VisualContextClient::new(ACCOUNT, "fixture-key").with_test_base(base.clone());
    assert!(
        client
            .summarize(&[], "source", CancellationToken::new())
            .await
            .is_err()
    );
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(
        client
            .summarize(&vec![0; 750_001], "source", CancellationToken::new())
            .await
            .is_err()
    );
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let answer = client
        .summarize(&[1, 2, 3], "source", CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(answer, "Two labeled bars are visible.");
    let request = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(request.path.ends_with(crate::VisualContextClient::MODEL));
    assert_eq!(request.body["stream"], false);
    let content = request.body["messages"][1]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
    assert_eq!(
        content
            .iter()
            .filter(|part| part["type"] == "image_url")
            .count(),
        1
    );
    assert_eq!(
        content[1]["image_url"]["url"],
        "data:image/jpeg;base64,AQID"
    );
    // Match the original PageContextTests post-success oversize refusal while
    // the second HTTP reply is STILL listening. Connection refusal cannot hide
    // an accidental extra provider request; the only next request is sentinel.
    assert!(
        client
            .summarize(&vec![0; 750_001], "source", CancellationToken::new())
            .await
            .is_err()
    );
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    stop_listening_fixture(&base, &captured);
}

#[tokio::test]
async fn local_vision_requires_a_downloaded_vision_model() {
    let (base, captured) = server(vec![
        Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
        Reply::json(json!({"fixture":"finished"})),
    ]);
    let provider = OllamaProvider::new(LocalAIConfiguration::new(base.to_string()));
    let error = provider
        .summarize(&[1, 2, 3], "source", CancellationToken::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("vision model"), "{error}");
    let metadata = captured.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(metadata.path, "/api/show");
    assert_eq!(metadata.body, json!({"model":"llama3.2:1b"}));
    assert!(matches!(
        captured.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    stop_listening_fixture(&base, &captured);
}

#[test]
fn public_page_policy_rejects_special_addresses_and_nonstandard_urls() {
    use std::net::IpAddr;
    for address in [
        "0.0.0.0",
        "10.1.2.3",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.168.1.1",
        "100.64.0.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "::ffff:127.0.0.1",
        "fe80::1",
        "fc00::1",
        "2001:db8::1",
        "2002:7f00:1::1",
    ] {
        assert!(
            !crate::article::public_ip(address.parse::<IpAddr>().unwrap()),
            "{address}"
        );
    }
    for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(crate::article::public_ip(address.parse().unwrap()));
    }
    for address in [
        "http://127.1",
        "http://2130706433",
        "http://localhost",
        "https://private.local",
        "https://a.internal",
        "https://example.com:8080",
        "https://user:secret@example.com",
        "file:///tmp/page",
    ] {
        assert!(crate::public_article_url(address).is_err(), "{address}");
    }
    assert_eq!(
        crate::public_article_url("https://example.com/article#section")
            .unwrap()
            .as_str(),
        "https://example.com/article"
    );
}

#[test]
fn inert_article_parser_removes_scripts_forms_navigation_and_hidden_content() {
    let html = "<html><head><title>Fixture &amp; article</title><script>fetch('https://leak.example')</script></head><body><nav>Navigation</nav><article><h1>A reading study</h1><p>The study <b>may</b> help some readers.</p><p hidden>Secret hidden text</p><div contenteditable>Editable private text</div><script>private script text</script><style>private style text</style><form><input value='private input'><p>private form text</p></form><ul><li><p>One supported detail.</p></li></ul><footer>Footer text</footer></article></body></html>";
    let result = crate::extract_article_html(html, "https://example.com/article", "study").unwrap();
    assert_eq!(result.title, "Fixture & article");
    assert!(result.text.contains("may help"));
    assert_eq!(result.text.matches("One supported detail.").count(), 1);
    for excluded in ["Navigation", "Secret", "Editable", "private", "Footer"] {
        assert!(!result.text.contains(excluded), "{excluded}");
    }
}

#[test]
fn article_excerpts_are_bounded_and_prioritize_selected_context() {
    let html = format!(
        "<title>Long page</title><article>{}<p>The selected quantum passage is relevant.</p></article>",
        (0..200)
            .map(|i| format!("<p>Unrelated paragraph {i} {}</p>", "a".repeat(100)))
            .collect::<String>()
    );
    let result = crate::extract_article_html(
        &html,
        "https://example.com/article",
        "selected quantum passage",
    )
    .unwrap();
    assert!(result.text.len() <= 12_000);
    assert!(result.text.contains("selected quantum passage"));
}

#[tokio::test]
async fn offline_article_fetch_is_rejected_before_dns_or_network() {
    let error =
        crate::fetch_public_reference("https://example.com", "", true, CancellationToken::new())
            .await
            .unwrap_err()
            .to_string();
    assert!(error.contains("Offline mode"));
}

#[tokio::test]
#[ignore = "explicit public HTTPS integration check; never calls an AI provider"]
async fn live_public_article_smoke() {
    let reference =
        crate::fetch_public_reference("https://example.com/", "", false, CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(reference.kind, "article");
    assert!(!reference.title.is_empty());
    assert!(!reference.text.trim().is_empty());
    assert!(reference.text.len() <= 12_000);
}
