# Pickle providers and browser context

This crate implements the shared network-facing services for the Rune application. Constructors do not send requests. Tests use local HTTP fixtures unless explicitly marked as live.

## Generation and evaluation

- `CloudflareProvider::new(account_id, token, model)` implements the core `GenerativeProvider` trait. `with_streaming(false)` requests a buffered prose response. Structured generation always uses the schema-capable diagram model.
- `OllamaProvider::new(LocalAIConfiguration)` implements Local generation, read-only `installed_models(cancel)`, `verify_model(vision, cancel)`, and explicit `summarize(jpeg, selection, cancel)`.
- `JevClient::new(account_id, token)` implements the core `DecisionClient` trait and validates typed responses. Direct, REST-wrapped, and Completed-job envelopes are supported.
- `VisualContextClient::new(account_id, token).summarize(jpeg, selection, cancel)` performs explicit Cloudflare image analysis. Ordinary generation never calls it.
- `SampleProvider` returns a labeled fixed answer without networking. Its `PASSAGE` and `ANSWER` constants define the demonstration. It is not a local AI writer.

The application must enforce its consent, exclusion, pause, and provider-choice policy before invoking these services. Strict offline is also enforced through the core pipeline's remote-provider gate, `LocalAIConfiguration::validate(true)`, and public-article fetching's offline argument. A loopback SSH tunnel is not same-device inference; set `same_device` accurately.

Transport uses explicit deadlines, bounded bodies and streaming events, no redirects, no response cache or cookie jar, no automatic retry, and redacted provider errors. Cancellation interrupts pending HTTP consumption. Local model metadata is checked before inference; cloud-backed aliases are rejected.

## Public article context

```rust,ignore
let reference = fetch_public_reference(
    "https://example.com/article",
    &selected_text,
    strict_offline,
    cancel,
).await?;
```

The fetcher validates public URL syntax, rejects special/private resolved addresses, pins the validated resolution into the HTTP client, retains the hostname for TLS verification, disables proxies and redirects, and bounds the download to 2 MB and five seconds overall.

`extract_article_html(html, url, selection)` is the pure extraction function. Its parser does not execute JavaScript, load images/subresources, navigate, or share browser cookies. It prefers article/main content, excludes scripts, forms, navigation and explicitly hidden/editable content, and produces a bounded selection-relevant excerpt.

This portable extractor is deliberately described as a conservative heuristic. It does not implement Mozilla Readability and needs real-site compatibility testing. The extension retains its existing Readability extraction path for loaded pages.

## Browser bridge

Keep a `BrowserBridge` handle alive while context is enabled:

```rust,ignore
let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
let bridge = BrowserBridge::start(
    support_directory.join("BrowserBridge"),
    sender,
    application_cancel,
).await?;

while let Some(request) = receiver.recv().await {
    // Perform current pause, context, exclusion, and session-policy checks on the UI thread.
    // Payload has already passed authentication, freshness, size, identity, URL, and replay checks.
    let decision = accept_reference_after_policy_checks(&request.payload);
    let _ = request.reply.send(decision);
}
```

`BrowserRequest` contains the payload and a one-shot acknowledgement channel. Acknowledging receipt before checking application policy would bypass that policy; check it first. The bridge does not automatically generate an answer or capture a screen.

`BrowserPayload::is_excluded(exclusions)` recognizes browser identity aliases and Windows executable-path basenames. Extension captures do not establish a source PID/window handle, so never treat them as permission to capture an arbitrary foreground window.

The listener binds only to `127.0.0.1`, uses an unpredictable per-instance token, allows four simultaneous connections, applies a five-second connection deadline, and retains bounded replay IDs. Unix configuration directories/files use 0700/0600 permissions. Windows configuration inherits the user's application-data directory ACLs. Content is not written to the connection file.

## Native messaging executable and setup

Build and package the `pickle-browser-host` executable with the application. It replaces the Python runtime dependency. The extension still targets `com.pickle.reader` and uses the existing length-prefixed JSON protocol.

Explicit setup calls:

```rust,ignore
let manifest = install_browser_host(
    BrowserKind::Chrome,
    &extension_id,
    &packaged_host_executable,
    &application_bundle_or_executable,
    &support_directory,
)?;
```

The installer creates a browser-specific binary copy and adjacent origin-bound registration, then registers the native messaging manifest in the selected browser's per-user configuration. Windows registration uses HKCU; it does not require administrator access. Arc setup is currently unsupported on Windows.

The host checks the calling extension origin, overwrites any page-supplied browser identity using its installed registration, reads the loopback connection metadata, and returns one framed response. If necessary, it attempts to open the installed application. Nothing except framed JSON is written to stdout, and no captured content is logged.

Browser integration is opt-in. Installing the same native-host name replaces that browser's existing Pickle host registration; development and legacy installations must not be connected indiscriminately. The host binary copies should be refreshed through the selected installer/update workflow, and browser registration cleanup must be part of the production uninstall design.

`uninstall_browser_host(browser, extension_id, support_directory)` is the explicit disconnect action. It verifies the selected browser, exact extension origin, app bridge directory, native-host path, and manifest ownership before removing known files. A different registration, symbolic link, or concurrent registration change is refused. Windows cleanup touches only the current user's exact default host registry value; no key trees or other users' configuration are removed. Unrelated files are preserved, and only an empty per-browser leaf directory is removed. Run installation/disconnection on a blocking worker rather than the UI thread.

An installer can call `uninstall_hosts_for_application(application, support_directory)` before removing its application executable/bundle. This canonicalizes the current application path, preflights known browser registrations, and disconnects only hosts recorded for that exact application. A different installed version at another path is skipped even if it shares the same application-data root or executable name. The function returns the number disconnected and never recursively removes data directories.

## Verification

From the repository root:

```sh
cargo +stable test --manifest-path rune/Cargo.toml -p pickle-providers
cargo +stable test --manifest-path rune/Cargo.toml -p pickle-providers live_public_article_smoke -- --ignored
```

The second command makes one public HTTPS request to example.com and performs no AI inference. It is excluded from the normal deterministic test run.

Tests cover real local HTTP contracts, SSE/NDJSON framing and completion, Unicode chunk boundaries, bounds, cancellation, schema routing, local metadata, Jev envelopes, article extraction and address policy, authenticated relay, policy rejection, replay protection, owner-only configuration, and the actual native-host executable's stdin/stdout protocol.

Windows native execution and browser installation still require testing on Windows. Cross-checking this crate from a Mac also requires Windows C SDK headers for the TLS dependency's native build; the Rust target alone is insufficient.
