//! Real TLS/HTTP fixtures, not an inference server or replacement transport.
//! Certificates/keys exist only in RAM; only this client trusts the fixture CA.
use async_trait::async_trait;
use pickle_core::{
    DecisionClient, DecisionResponse, DecisionState, GenerativeProvider, PickleError, Pipeline,
    PipelineEvent, PipelineOutcome, Questions, ReadingAction, RequestInput, SelectionSnapshot,
};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use reqwest::{
    Client,
    dns::{Name, Resolve, Resolving},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, timeout, timeout_at},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{self, pki_types::PrivatePkcs8KeyDer},
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    LocalAIConfiguration, OllamaProvider,
    transport::{self, RequestContext},
};

const HOST: &str = "ross.example";
const ADDRESS: &str = "https://ross.example";
const TOKEN: &str = "fixture"; // Public original Swift fixture token, never a user credential.
const MODEL: &str = "llama3.2:1b";
const SOURCE: &str = "The treatment may help.";
const DEADLINE: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 16;

struct RejectDns;
impl Resolve for RejectDns {
    fn resolve(&self, _: Name) -> Resolving {
        // The sole accepted name uses the client's explicit loopback override.
        // Every fallback name fails without calling system DNS or any network.
        Box::pin(async {
            Err(Box::new(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture DNS is closed",
            )) as Box<dyn std::error::Error + Send + Sync>)
        })
    }
}

enum WaitAt {
    None,
    BeforeHeaders,
    AfterHeaders,
}
struct Reply {
    status: u16,
    mime: &'static str,
    extra: &'static str,
    body: String,
    wait: WaitAt,
}
impl Reply {
    fn json(value: Value) -> Self {
        Self {
            status: 200,
            mime: "application/json",
            extra: "",
            body: value.to_string(),
            wait: WaitAt::None,
        }
    }
    fn original_stream() -> Self {
        Self {
            status: 200, mime: "application/x-ndjson", extra: "", wait: WaitAt::None,
            body: concat!(
                "{\"message\":{\"content\":\"A simple \"},\"done\":false}\n",
                "{\"message\":{\"content\":\"answer.\"},\"done\":true,\"done_reason\":\"stop\",\"load_duration\":2000000,\"eval_duration\":30000000,\"eval_count\":4}\n",
            ).into(),
        }
    }
}

#[derive(Debug)]
struct Captured {
    request_line: String,
    headers: BTreeMap<String, String>,
    body: Value,
    sni_matches: bool,
}
#[derive(Default)]
struct Report {
    requests: Vec<Captured>,
    tcp_connections: usize,
    tls_rejections: usize,
}
#[derive(Debug, PartialEq, Eq)]
enum Observation {
    Http,
    TlsRejected,
}
struct TlsFixture {
    address: SocketAddr,
    ca: Vec<u8>,
    stop: CancellationToken,
    task: Option<JoinHandle<Result<Report, &'static str>>>,
    observations: mpsc::Receiver<Observation>,
}
impl TlsFixture {
    async fn start(san: &str, replies: Vec<Reply>) -> Self {
        // No key/cert files, OS trust store, environment, crypto-provider global
        // installation, external process, DNS query, or non-loopback listener.
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = Issuer::new(ca_params, ca_key);
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec![san.into()])
            .unwrap()
            .signed_by(&leaf_key, &issuer)
            .unwrap();
        let key = PrivatePkcs8KeyDer::from(leaf_key.serialize_der());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![leaf.der().clone()], key.into())
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let stop = CancellationToken::new();
        let (observed, observations) = mpsc::channel(MAX_CONNECTIONS);
        let task = tokio::spawn(serve(
            listener,
            TlsAcceptor::from(Arc::new(config)),
            replies.into(),
            stop.clone(),
            observed,
        ));
        Self {
            address,
            ca: ca.der().to_vec(),
            stop,
            task: Some(task),
            observations,
        }
    }
    fn client(&self, trust_ca: bool) -> Client {
        let mut builder = transport::secure_client_builder(Duration::from_secs(90))
            .tls_built_in_root_certs(false)
            .dns_resolver(Arc::new(RejectDns))
            .resolve(HOST, self.address);
        if trust_ca {
            builder =
                builder.add_root_certificate(reqwest::Certificate::from_der(&self.ca).unwrap());
        }
        builder.build().unwrap()
    }
    async fn observed(&mut self, expected: Observation) {
        assert_eq!(
            timeout(DEADLINE, self.observations.recv()).await.unwrap(),
            Some(expected)
        );
    }
    async fn finish(mut self) -> Report {
        self.stop.cancel();
        let mut task = self.task.take().unwrap();
        match timeout(DEADLINE, &mut task).await {
            Ok(result) => result.unwrap().unwrap(),
            Err(_) => {
                task.abort();
                let _ = timeout(DEADLINE, task).await;
                panic!("Owned TLS fixture did not stop boundedly");
            }
        }
    }
}
impl Drop for TlsFixture {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn read_request(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
) -> Result<Captured, &'static str> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 16_384 {
            return Err("fixture header bound");
        }
        bytes.push(stream.read_u8().await.map_err(|_| "fixture header read")?);
    }
    let text = String::from_utf8(bytes).map_err(|_| "fixture header encoding")?;
    let mut lines = text.lines();
    let request_line = lines.next().ok_or("fixture request line")?.to_owned();
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("fixture header format")?;
        if headers
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err("fixture duplicate header");
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err("fixture unexpected transfer encoding");
    }
    let length: usize = headers
        .get("content-length")
        .map(String::as_str)
        .unwrap_or("0")
        .parse()
        .map_err(|_| "fixture body length")?;
    if length > 65_536 {
        return Err("fixture body bound");
    }
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|_| "fixture body read")?;
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).map_err(|_| "fixture body JSON")?
    };
    Ok(Captured {
        request_line,
        headers,
        body,
        sni_matches: stream.get_ref().1.server_name() == Some(HOST),
    })
}

async fn serve(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    mut replies: VecDeque<Reply>,
    stop: CancellationToken,
    observed: mpsc::Sender<Observation>,
) -> Result<Report, &'static str> {
    let mut report = Report::default();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (socket, peer) = tokio::select! {
            _ = stop.cancelled() => return Ok(report),
            accepted = timeout_at(deadline, listener.accept()) => accepted.map_err(|_| "fixture accept deadline")?.map_err(|_| "fixture accept")?,
        };
        if !peer.ip().is_loopback() || report.tcp_connections >= MAX_CONNECTIONS {
            return Err("fixture connection bound");
        }
        report.tcp_connections += 1;
        let handshake = tokio::select! {
            _ = stop.cancelled() => return Ok(report),
            handshake = timeout(DEADLINE, acceptor.accept(socket)) => handshake.map_err(|_| "fixture handshake deadline")?,
        };
        let mut stream = match handshake {
            Ok(stream) => stream,
            Err(_) => {
                report.tls_rejections += 1;
                observed
                    .try_send(Observation::TlsRejected)
                    .map_err(|_| "fixture observation bound")?;
                continue;
            }
        };
        let request = tokio::select! {
            _ = stop.cancelled() => return Ok(report),
            request = timeout(DEADLINE, read_request(&mut stream)) => request.map_err(|_| "fixture request deadline")??,
        };
        report.requests.push(request);
        observed
            .try_send(Observation::Http)
            .map_err(|_| "fixture observation bound")?;
        let reply = replies
            .pop_front()
            .ok_or("unexpected fixture HTTP request")?;
        if matches!(reply.wait, WaitAt::BeforeHeaders) {
            stop.cancelled().await;
            return Ok(report);
        }
        let headers = format!(
            "HTTP/1.1 {} Fixture\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n",
            reply.status,
            reply.mime,
            reply.body.len(),
            reply.extra
        );
        timeout(DEADLINE, stream.write_all(headers.as_bytes()))
            .await
            .map_err(|_| "fixture header write deadline")?
            .map_err(|_| "fixture header write")?;
        timeout(DEADLINE, stream.flush())
            .await
            .map_err(|_| "fixture flush deadline")?
            .map_err(|_| "fixture flush")?;
        if matches!(reply.wait, WaitAt::AfterHeaders) {
            stop.cancelled().await;
            return Ok(report);
        }
        timeout(DEADLINE, stream.write_all(reply.body.as_bytes()))
            .await
            .map_err(|_| "fixture body write deadline")?
            .map_err(|_| "fixture body write")?;
        let _ = timeout(Duration::from_secs(1), stream.shutdown()).await;
    }
}

struct CloudChecker {
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl DecisionClient for CloudChecker {
    async fn evaluate(
        &self,
        _: DecisionState,
        _: Questions,
        _: CancellationToken,
    ) -> Result<DecisionResponse, PickleError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PickleError::message(
            "The HTTPS Local fixture must never call cloud checks",
        ))
    }
}
fn provider(client: Client) -> OllamaProvider {
    OllamaProvider::new(LocalAIConfiguration {
        address: ADDRESS.into(),
        token: TOKEN.into(),
        model: MODEL.into(),
        same_device: false,
    })
    .with_test_client(client)
}
fn input() -> RequestInput {
    RequestInput::new(
        SelectionSnapshot::new(SOURCE, "Test", "test"),
        ReadingAction::Simplify,
    )
}
fn context(cancel: &CancellationToken, timeout: Duration, sse: bool) -> RequestContext<'_> {
    RequestContext {
        token: TOKEN,
        timeout,
        local: true,
        sse,
        cancel,
    }
}

#[tokio::test]
async fn legacy_https_local_pipeline_preserves_routing_streaming_and_no_cloud_checks() {
    let fixture = TlsFixture::start(
        HOST,
        vec![
            Reply::json(json!({"details":{"format":"gguf"},"capabilities":["completion"]})),
            Reply::original_stream(),
        ],
    )
    .await;
    let writer = provider(fixture.client(true));
    assert!(
        writer.is_remote(),
        "A trusted HTTPS recipient is not same-device inference"
    );
    assert!(!writer.permits_cloud_checks());
    let calls = Arc::new(AtomicUsize::new(0));
    let checker = Arc::new(CloudChecker {
        calls: calls.clone(),
    });
    let (events, mut received) = mpsc::unbounded_channel();
    let outcome = timeout(
        DEADLINE,
        Pipeline::new(Arc::new(writer), Some(checker), false).run(
            input(),
            events,
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let PipelineOutcome::Result(answer) = outcome else {
        panic!("Expected the exact original Local result");
    };
    assert_eq!(answer.text, "A simple answer.");
    assert_eq!(answer.model, MODEL);
    assert_eq!(answer.repairs, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(answer.evaluator_model.is_none());
    let timing = answer.timing.unwrap();
    assert_eq!(timing.load_seconds, 0.002);
    assert_eq!(timing.generation_seconds, 0.03);
    assert_eq!(timing.output_tokens, 4);
    assert!(timing.first_token_seconds.is_some());
    let mut drafts = Vec::new();
    while let Ok(event) = received.try_recv() {
        if let PipelineEvent::Draft(text) = event {
            drafts.push(text);
        }
    }
    assert_eq!(drafts, ["A simple ", "A simple answer."]);
    let report = fixture.finish().await;
    assert_eq!(report.tcp_connections, 2);
    assert_eq!(report.tls_rejections, 0);
    assert_eq!(report.requests.len(), 2);
    for (request, path) in report.requests.iter().zip(["/api/show", "/api/chat"]) {
        assert!(request.sni_matches);
        assert_eq!(request.request_line, format!("POST {path} HTTP/1.1"));
        assert_eq!(request.headers.get("host").unwrap(), HOST);
        assert_eq!(
            request.headers.get("authorization").unwrap(),
            "Bearer fixture"
        );
        assert_eq!(
            request.headers.get("content-type").unwrap(),
            "application/json"
        );
    }
    assert_eq!(report.requests[0].body, json!({"model":MODEL}));
    let body = &report.requests[1].body;
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["stream"], true);
    assert_eq!(body["keep_alive"], "2m");
    assert_eq!(
        body["options"],
        json!({"num_ctx":8192,"num_predict":1024,"temperature":0.2})
    );
    assert!(body.get("format").is_none());
    let source: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(source["source"], SOURCE);
}

async fn rejected_tls(san: &str, trust_ca: bool) {
    let mut fixture = TlsFixture::start(san, vec![]).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::new(
        Arc::new(provider(fixture.client(trust_ca))),
        Some(Arc::new(CloudChecker {
            calls: calls.clone(),
        })),
        false,
    );
    let (events, mut received) = mpsc::unbounded_channel();
    let error = timeout(
        DEADLINE,
        pipeline.run(input(), events, CancellationToken::new()),
    )
    .await
    .unwrap()
    .expect_err("Real TLS certificate validation must reject this recipient");
    assert!(error.to_string().contains("Can’t reach the Local server"));
    assert!(!error.to_string().contains(TOKEN));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    while let Ok(event) = received.try_recv() {
        assert!(!matches!(event, PipelineEvent::Draft(_)));
    }
    fixture.observed(Observation::TlsRejected).await;
    let report = fixture.finish().await;
    assert_eq!(report.tcp_connections, 1);
    assert_eq!(report.tls_rejections, 1);
    assert!(
        report.requests.is_empty(),
        "No HTTP/Bearer/source may cross failed TLS validation"
    );
}
#[tokio::test]
async fn legacy_https_local_untrusted_ca_fails_before_http() {
    rejected_tls(HOST, false).await;
}
#[tokio::test]
async fn legacy_https_local_wrong_hostname_fails_before_http() {
    rejected_tls("wrong.example", true).await;
}

#[tokio::test]
async fn https_fixture_fallback_dns_is_closed() {
    let fixture = TlsFixture::start(HOST, vec![]).await;
    assert!(
        timeout(
            DEADLINE,
            fixture.client(true).get("https://unmapped.invalid/").send()
        )
        .await
        .unwrap()
        .is_err()
    );
    let report = fixture.finish().await;
    assert_eq!(report.tcp_connections, 0);
    assert!(report.requests.is_empty());
}

#[tokio::test]
async fn https_common_sender_preserves_status_mime_and_redirect_refusals() {
    for (status, sse, reason) in [
        (401, false, "Local server access denied"),
        (200, true, "did not return a text stream"),
        (302, false, "HTTP 302"),
    ] {
        let mut reply = Reply::json(json!({"private":"must-not-appear-in-errors"}));
        reply.status = status;
        if status == 302 {
            reply.extra = "Location: https://unmapped.invalid/private\r\n";
        }
        let fixture = TlsFixture::start(HOST, vec![reply]).await;
        let cancel = CancellationToken::new();
        let error = transport::open_with_test_client(
            fixture.client(true),
            Url::parse(ADDRESS).unwrap(),
            None,
            context(&cancel, DEADLINE, sse),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains(reason));
        assert!(!error.to_string().contains("must-not-appear"));
        let report = fixture.finish().await;
        assert_eq!(report.requests.len(), 1);
        assert_eq!(report.tcp_connections, 1);
        if sse {
            assert_eq!(
                report.requests[0].headers.get("accept").unwrap(),
                "text/event-stream"
            );
        }
    }
}

#[tokio::test]
async fn https_common_sender_keeps_body_deadline_with_an_injected_client() {
    let mut reply = Reply::json(json!({"message":"body is deliberately withheld"}));
    reply.wait = WaitAt::AfterHeaders;
    let fixture = TlsFixture::start(HOST, vec![reply]).await;
    let cancel = CancellationToken::new();
    let response = transport::open_with_test_client(
        fixture.client(true),
        Url::parse(ADDRESS).unwrap(),
        None,
        context(&cancel, Duration::from_secs(2), false),
    )
    .await
    .unwrap();
    let error = timeout(DEADLINE, transport::json_body(response, &cancel))
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("interrupted"));
    assert_eq!(fixture.finish().await.requests.len(), 1);
}

#[tokio::test]
async fn https_common_sender_cancels_pending_headers_and_stops_owned_fixture() {
    let mut reply = Reply::json(json!({"unused":"never sent"}));
    reply.wait = WaitAt::BeforeHeaders;
    let mut fixture = TlsFixture::start(HOST, vec![reply]).await;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let client = fixture.client(true);
    let request = transport::open_with_test_client(
        client,
        Url::parse(ADDRESS).unwrap(),
        None,
        context(&cancel, DEADLINE, false),
    );
    let after_observed = async {
        fixture.observed(Observation::Http).await;
        trigger.cancel();
    };
    let (result, ()) = timeout(DEADLINE, async { tokio::join!(request, after_observed) })
        .await
        .unwrap();
    assert!(matches!(result, Err(PickleError::Cancelled)));
    assert_eq!(fixture.finish().await.requests.len(), 1);
}
