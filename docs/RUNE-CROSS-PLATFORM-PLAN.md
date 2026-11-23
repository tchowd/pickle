# Pickle: Rune rebuild and cross-platform shipping plan

Design baseline: November 22, 2026. Development status reviewed November 23, 2026.

Status: **Development rebuild implemented and under native verification.** The user chose development-only delivery on November 23; production signing/notarization, update hosting and publication are deferred. The [implementation status](RUNE-IMPLEMENTATION-STATUS.md) and [native acceptance record](RUNE-NATIVE-ACCEPTANCE.md) distinguish current source, verified artifacts, failed attempts and remaining gates. The public shipping requirements below remain the contract for a later release; they are not all satisfied by the development build.

## 1. Objective and recommendation

Rebuild Pickle as an installable desktop reading assistant for **both macOS and Windows**, using Rune for the interface and Rust for shared application behavior. Preserve the defining interaction: select text in another application, invoke Pickle, choose a reading action, and read the response in a floating panel without changing the original document.

Cross-platform distribution is a primary requirement. A completed macOS rewrite followed by an unspecified future Windows port does not meet the objective. Both operating systems must participate in the first end-to-end prototype, continuous integration, compatibility testing, and release preparation.

Recommended approach:

1. Build a shared Rust application and Rune interface.
2. Put operating-system integrations behind explicit platform interfaces.
3. Initially reuse small, proven Swift services on macOS through an in-process bridge.
4. Implement Windows integrations with Windows APIs through Rust bindings.
5. Keep the current Swift app available during migration and use its behavior and fixtures as a reference.
6. Prove cross-app capture and floating-window behavior on both systems before porting the complete interface.
7. Ship ordinary signed applications, installers, and an update path for each platform.

Rune is a viable candidate, but the decision to use it for the production rebuild remains subject to the native-integration gate in Phase 1. Cross-platform rendering alone does not prove that Pickle's complete interaction works reliably on both platforms.

### 1.1 Benefits and costs of the rebuild

| Benefit | Corresponding cost or limitation |
|---|---|
| Share the reader UI, reading pipeline, provider routing, settings, and storage across macOS and Windows. | Selection capture, permissions, focus, tray/menu behavior, credentials, OCR, and source-app audio still need platform implementations and device testing. |
| Deliver most product changes to both platforms from one application codebase. | Rewriting the existing Swift app introduces migration and regression work before that maintenance benefit is realized. |
| Use Rust types, ownership, and cancellation contracts for shared policy and background work. | Maintaining the product requires Rust plus enough Swift/AppKit and Windows API expertise to diagnose native failures. Memory safety alone does not establish correct privacy policy or native behavior. |
| Automate screenshots, element-tree inspection, and scripted interactions through Rune's headless tooling. | Headless checks cannot establish desktop focus, selection access, real screen-reader behavior, permissions, or installer reliability. |
| Keep one coherent Pickle visual design on both platforms. | A custom-rendered interface requires deliberate accessibility, keyboard, text, scaling, and platform-behavior acceptance work. |
| Build a shared native application without adopting a browser-based UI stack. | Actual startup, memory, idle CPU/GPU, binary size, and reading latency must be measured; a framework choice is not evidence of a performance improvement. |

The largest framework-specific risk is Rune's prerelease status. Pinning a revision makes builds repeatable but leaves Pickle responsible for deciding when to upgrade and for maintaining bounded workarounds if upstream changes or defects affect the product. A pinned dependency does not remove that maintenance obligation.

The recommended business tradeoff is a parallel rebuild while the Swift app remains usable. This invests more work up front in exchange for a shared cross-platform product. It should not be represented as the quickest route to the next macOS release. Continue toward public distribution only after the two-platform feasibility and release gates below have recorded evidence.

## 2. Evidence, assumptions, and unresolved facts

### 2.1 What has been inspected and verified

The investigation reviewed Rune at commit:

```text
67d73a5f18390686c8e9b8b3a32b3414ebd66db0
```

On the development Mac:

- Rune's Hello example compiled and passed its test.
- The Hello example rendered a PNG without opening an interactive application window.
- The launcher example passed `cargo check`.
- A separate application fetched Rune as a pinned Git dependency, compiled, and produced a PNG and element-tree JSON.
- That external application explicitly supplied Rune's required Cargo patches.
- The installed Rust 1.99.0 toolchain worked with `+stable`; Rune's checked-in toolchain file requests 1.99.1.

These checks validate basic development and headless rendering on that Mac. They do not validate Windows execution, installers, native selection capture, permissions, assistive technology, or production readiness.

The existing Pickle audit also ran the current deterministic suite: 75 Swift checks passed, along with browser extraction, native-host, and Python gateway checks. Record this as a migration baseline, not as evidence that the new implementation has passed those checks.

### 2.2 Rune capabilities confirmed in the inspected source

- Rust view/state APIs, layout, text rendering, animation, and GPU rendering.
- Retained application state while windows are hidden or closed.
- macOS accessory mode and nonactivating launcher-panel support.
- Multiple windows, placement requests, window handles, and background-to-UI updates.
- Global shortcut registration for conventional key combinations.
- Windows and Linux tray support.
- Selectable Markdown through `rune-kit`.
- Accessibility integration through AccessKit when enabled.
- Headless rendering, element-tree export, scripted input, and runtime tests.

### 2.3 Known gaps and limitations

- Rune's inspected `Tray` implementation does not support macOS. The Mac build needs an `NSStatusItem` implementation.
- Rune's shortcut parser requires a non-modifier key. Pickle's Control + Option press-and-release gesture needs a native recognizer.
- Exposing Pickle's own UI through AccessKit is separate from reading selected text in another application. The latter needs platform capture code.
- Rune does not supply Pickle's complete selection, permission, OCR, credential, browser-reference, or app-audio workflows.
- Window and overlay behavior varies by operating system. Some documentation describes limitations that differ between newer window APIs and older overlay APIs; inspect the pinned implementation and test the chosen path.
- Rune is prerelease. A pinned revision and lockfile are required, and upgrades need regression testing.
- Windows behavior has not been exercised in this investigation.
- Actual macOS 14 compatibility of the complete Rune application remains unverified, even though the existing Swift package targets macOS 14.

### 2.4 Proposed support baseline

Use the following as an initial engineering target, subject to device testing and an explicit release decision:

| Target | Initial proposal | Validation required |
|---|---|---|
| macOS | macOS 14+ on Apple Silicon | Build with an explicit deployment target; run on the oldest supported OS and current OS |
| Windows | Supported Windows 11 releases on x64 | Test on clean installations, standard-user accounts, and representative GPUs |
| macOS Intel | Separate support decision | Build, dependency, rendering, performance, and hardware validation |
| Windows ARM64 | Separate support decision | Native toolchain, dependency, installer, and hardware validation |

Do not advertise Intel, Windows ARM64, Windows 10, or any additional OS version merely because a cross-compilation succeeds. Publish the exact tested support matrix with the release.

## 3. Product contract and scope

### 3.1 Required core experience on both platforms

- Start as a background/menu-bar or system-tray application.
- Capture a supported application's selected text before Pickle changes focus.
- Show a compact preview and Simplify, Expand, and Chart actions.
- Expand into a movable, resizable reader with streaming prose and follow-ups.
- Offer manual paste when selection capture is unavailable.
- Preserve source identity throughout retries and follow-up questions.
- Support configured Local and Online providers without silent fallback.
- Expose cancellation, errors, explicit retry, and meaningful status.
- Provide accessible keyboard operation and readable text.
- Save answers explicitly and optionally record completed-answer history.
- Install, launch, update, and uninstall through documented platform workflows.

### 3.2 Complete migration inventory

| Capability | Shared implementation | Platform work | Migration expectation |
|---|---|---|---|
| Source snapshots and request budgets | Rust core | Capture metadata conversion | Preserve existing behavior |
| Simplify, Expand, follow-ups | Rust pipeline | None beyond networking/credentials | Required on both systems |
| Answer adjustments and word explanations | Rust commands and Rune views | Selection integration inside the reader | Required on both systems |
| Cloudflare generation and Jev | Rust providers and policy | Credential access | Preserve route and validation contracts |
| Ollama generation | Rust provider and prompt budgets | Credential access; optional connection controls | No cloud fallback |
| Streaming | Rust SSE/NDJSON parsers | UI wake and scheduling integration | Preserve bounds and completion semantics |
| Bar and flow charts | Rust validators and Rune rendering | Accessibility presentation | Preserve source-evidence constraints |
| Shortcut invocation | Shared action semantics | Native shortcut/gesture capture | Platform-appropriate defaults |
| Floating reader | Shared Rune content | Focus, placement, activation, display handling | Same user workflow on both systems |
| Pause and app exclusions | Rust policy | Native application identity | Apply before capture and requests |
| Bookmarks and knowledge base | Rust persistence and Rune views | OS data directory and permissions | Preserve privacy and corruption behavior |
| Screenshot context and OCR | Shared context model and budgets | Native capture and OCR adapter | Track parity explicitly |
| Visual analysis | Shared provider contract | Image capture and capability checks | Explicit image-sharing action |
| Web references | Shared fetch policy and reference model | Portable extraction or isolated native adapter | Preserve SSRF and content boundaries |
| Browser extension and video captions | Existing extension plus shared protocol | Host registration and app discovery | Chrome/Edge first; publish browser matrix |
| Source-app audio transcription | Shared recording state and reference model | Capture and transcription implementation | Track platform and hardware availability |
| Appearance preferences | Shared configuration and Rune theme | System theme/accessibility preferences | Preserve user control |
| Installation and updates | Shared version/channel policy | Separate signing and packaging | Required for shipping |

The target is the complete app. Context, video, and audio can be implemented after the core prototype, but they must not disappear from the plan. Before a release candidate, explicitly decide which features ship on each system. Any temporary difference must be visible in the product and release notes. Full parity cannot be declared while a listed capability is missing.

### 3.3 Non-goals for the initial rebuild

- Training or hosting a new foundation model.
- Automatically downloading large local models without user intent.
- Adding cloud account synchronization or uploading the knowledge base.
- Turning saved-answer history into retrieval-augmented generation.
- Editing the source application's document.
- Guaranteeing capture from every desktop application or protected field.
- Replacing source-evidence validation with unrestricted generated diagrams.
- Supporting Linux or web deployment as a launch requirement.

## 4. Architecture

### 4.1 Workspace layout

Develop alongside the existing app in a `rune/` workspace until cutover:

```text
pickle/
  Sources/                       Existing Swift app and core
  Tests/                         Existing behavior reference
  browser-extension/             Existing browser extension
  docs/
  rune/
    Cargo.toml
    Cargo.lock
    rust-toolchain.toml
    crates/
      pickle-app/
        src/
          main.rs
          app/
          views/
          theme/
      pickle-core/
        src/
          models/
          pipeline/
          policy/
          validation/
          persistence/
      pickle-providers/
        src/
          cloudflare/
          jev/
          ollama/
          transport/
      pickle-platform/
        src/
          lib.rs
          capabilities.rs
          macos/
          windows/
          fake/
    native/
      macos/
        include/
        Sources/
    tests/
      fixtures/
      parity/
      ui/
      migration/
    packaging/
      macos/
      windows/
    scripts/
```

Names and file boundaries can evolve. The architectural requirement is that pipeline rules and provider code remain independent of Rune views and OS APIs.

### 4.2 Responsibility boundaries

`pickle-core` owns immutable source snapshots, commands, pipeline states, policy, deterministic chart validation, persistence schemas, and result models. It must be testable without a GPU, native window, accessibility permission, or live provider.

`pickle-providers` owns HTTP transport, provider request construction, SSE/NDJSON decoding, metadata validation, output budgets, and typed provider errors. It must not invoke OS UI, make provider choices on behalf of the user, or write history.

`pickle-app` owns Rune views, window coordination, user commands, presentation state, and connecting services. User-visible state changes arrive on Rune's UI thread. Network, OCR, file I/O, and other slow work run outside the frame loop.

`pickle-platform` owns safe interfaces for selection capture, native application identity, permissions, shortcuts, tray/menu-bar integration, credentials, display information, capture, transcription, and shell integration. Compile only the selected operating system's implementation into that target.

### 4.3 Platform interface contracts

Define interfaces by capability rather than by a single large OS utility object. Candidate services:

- `SelectionCapture`: resolve source identity, reject protected/excluded content, obtain selected text and optional bounds.
- `InvocationService`: register/unregister shortcuts and report invocation events.
- `DesktopShell`: tray/menu-bar actions, reopen, quit, display changes, launch-at-login if included.
- `PermissionService`: report supported permissions, request them through the OS, and explain recovery.
- `CredentialVault`: save, load, and remove endpoint-scoped secrets with structured failures.
- `WindowCapture`: capture only the validated source target and return bounded image data.
- `TextRecognition`: turn an image into labeled, bounded OCR text.
- `AudioCapture`: capture the selected source scope for a bounded duration.
- `Transcription`: report local availability and transcribe without silently switching to a cloud service.
- `BrowserHostInstaller`: register/unregister the native messaging host for supported browsers.
- `PlatformPaths`: return app-data, cache, and diagnostic locations.

Return capabilities and typed outcomes such as unavailable, permission denied, protected content, unsupported source, timeout, cancelled, and success. An unavailable feature must not look like an empty successful capture.

Application identity should include an OS-qualified stable identifier, a display name, and validated process/window handles when needed. Preserve macOS bundle IDs; define Windows identities using appropriate executable/package information. Do not rely on mutable window titles or process IDs alone for persistent exclusions.

### 4.4 Concurrency and event-loop ownership

- Rune owns the main application event loop.
- Use a background async runtime for providers and other asynchronous services.
- Deliver state updates through Rune's application/window handles or another tested wake mechanism.
- Native callbacks produce events; they do not modify view state from arbitrary threads.
- Never block the UI thread waiting for HTTP, OCR, transcription, credential prompts, or disk writes.
- Keep UI-thread work bounded; bridge functions that must run on the main thread dispatch expensive work elsewhere.
- Do not start a second SwiftUI application or replace Rune's app delegate from the bridge.
- Document ownership, cancellation, shutdown, and callback lifetimes across the native boundary.
- Batch or coalesce streaming updates; preserve the existing approximately 80 ms display-update cadence initially.

### 4.5 State and cancellation model

Represent the reading lifecycle with explicit states, for example:

```text
Idle -> Capturing -> Choosing
Choosing -> AwaitingConsent | AwaitingContext | ChoosingChart | Generating
Generating -> Reviewing -> Repairing -> Reviewing -> Completed
Generating -> Completed
Any active state -> Cancelled | Failed
```

Keep context acquisition and provider checking explicit where they occur. The diagram is a starting model, not permission to collapse all concurrent work into one state variable.

Every request carries a session ID, request/attempt ID, and policy revision. Progress, drafts, OCR, references, review results, and final answers commit only when these still match. Cancelling or replacing a request invalidates pending callbacks before another request can use their output.

Closing the reader, pausing, changing provider/privacy policy, or replacing the source must cancel relevant work. Appearance changes should not cancel generation. Cancelling an HTTP client does not guarantee that already-dispatched inference stops or that charges are reversed; retain honest wording.

## 5. Rune dependency and development setup

### 5.1 Pin the framework and toolchain

Start from the investigated revision, then deliberately review any newer revision before adopting it. Do not depend on a moving `dev` branch or an unrelated package named Rune.

Candidate `rune/rust-toolchain.toml`, matching the inspected upstream file:

```toml
[toolchain]
channel = "1.99.1"
components = ["clippy", "rustfmt"]
```

The web target is unnecessary for the desktop launch. Install the selected Rust toolchain and required native build tools during implementation; the investigation did not install a replacement toolchain.

### 5.2 Required Cargo dependency configuration

The following is a starting dependency section for the root workspace manifest, not a complete application manifest:

```toml
[workspace.dependencies]
rune = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0" }
rune-kit = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0", features = ["markdown"] }

[patch.crates-io]
winit = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0" }
sctk-adwaita = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0" }
parley_engine = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0" }
wgpu-hal = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0" }
accesskit_windows = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0" }
accesskit_macos = { git = "https://github.com/anomalyco/rune", rev = "67d73a5f18390686c8e9b8b3a32b3414ebd66db0" }
```

Each member crate declares the workspace dependencies it actually uses. Cargo does not inherit a dependency's patches, so these patches belong in Pickle's workspace root. Keep Rune, rune-kit, and every Rune patch on the same revision. Reinspect upstream's patch table on every framework upgrade.

Commit `Cargo.lock` and use `--locked` in CI and release builds. Pinning Rune alone does not pin every transitive dependency. Initially retain the default accessibility feature; do not reduce dependency features without checking the resulting capabilities.

### 5.3 Build-machine prerequisites

| Machine | Required setup |
|---|---|
| macOS | Rust toolchain, Apple Command Line Tools/SDK, Swift compiler for the initial bridge, and an interactive GUI session for native tests |
| Windows | Rust MSVC toolchain, Visual Studio C++ Build Tools, Windows SDK, and an interactive desktop for native tests |
| Release machines | Platform signing configuration, protected credentials, packaging tools, and controlled artifact publication |

Prefer native builds on each operating system for the initial rollout. Cross-compilation can supplement this later, but does not replace native testing.

Set the macOS deployment target deliberately and verify that Rune, its dependencies, and the bridge support it. Confirm Windows runtime prerequisites in a clean VM instead of assuming developer-machine DLLs are present on user machines.

### 5.4 Developer commands to introduce

After the workspace exists, standardize commands equivalent to:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo run --locked -p pickle-app
cargo run --locked -p pickle-app -- --snap target/reader.png --scene sample
cargo run --locked -p pickle-app -- --tree target/reader-tree.json --scene sample
```

The `sample` scene and any app-specific commands above are planned work. They do not exist in the current Swift repository. Provide equivalent scripts for PowerShell and POSIX shells where platform commands differ. Keep native permission tests distinct from network-free headless checks.

## 6. macOS implementation

### 6.1 Bridge strategy

Initially extract useful services from `SelectionService.swift`, `InvocationController.swift`, `ScreenContext.swift`, `AudioContextService.swift`, and credential code in `Settings.swift`. Do not carry the entire existing `AppDelegate` or `RequestCoordinator` into the bridge.

Expose a small C-compatible ABI with opaque handles, explicit result codes, versioned data structures or bounded serialized payloads, and matching allocation/free functions. No Swift object should escape as an unowned Rust pointer. Define which thread invokes each callback and how callbacks are unregistered during shutdown.

For bridge calls:

- Keep secrets and captured text out of diagnostic logs.
- Specify byte lengths rather than assuming null-terminated text where content is arbitrary.
- Bound payloads and validate UTF-8 at the boundary.
- Catch and translate errors within the bridge; never unwind across the FFI boundary.
- Ensure cancellation releases screenshots, audio buffers, and completion closures.
- Handle callbacks arriving after cancellation without accessing freed state.

Choose static-library or bundled-dynamic-library packaging after a small linking test. Verify Swift runtime requirements on the minimum supported OS. Sign all bundled native code consistently. The bridge remains in the app's process; avoid a separate capture helper unless a tested platform requirement justifies one.

### 6.2 Desktop shell and panel

- Create an accessory application with an `NSStatusItem` and pause, reopen, paste, saved answers, settings, clear, and quit actions.
- Use Rune's nonactivating window/panel path for the reader; evaluate the launcher preset rather than assuming a passive edge overlay provides all required input behavior.
- Capture the source before showing or focusing any Pickle surface.
- Preserve one panel identity while compact actions expand into the result.
- Keep the bottom anchor stable while expanding, including after the user moves the panel.
- Support keyboard focus for follow-ups without losing the source snapshot.
- Test Escape, outside clicks, pinning, reopen, and quit.
- Test full-screen Spaces, Mission Control, multiple displays, display removal, Retina scaling, and Dock/menu-bar work areas.
- Verify whether resize/position APIs apply to the actual panel host in the pinned Rune revision.

### 6.3 Permissions and identity

- Preserve Accessibility checks and protected-field/excluded-app guards.
- Request Screen Recording only for features that use capture and explain its scope.
- Keep speech/transcription authorization separate from ordinary text reading.
- Provide clear denied, revoked, and restart-required states.
- Use a separate development bundle ID so prototypes do not replace the existing app's identity.
- Decide the production bundle identity and signing strategy before migration tests involving real Keychain items or existing preferences.
- Document that permission and credential continuity must be tested; sharing a service name does not guarantee silent access after an identity change.

## 7. Windows implementation

### 7.1 Selection capture

Implement a Windows UI Automation client that identifies the foreground source, obtains the focused element and a supported text pattern, and reads its selected range. Validate source identity before and after capture so a focus race cannot attach the wrong window's content.

Requirements:

- Run potentially blocking automation work off the UI thread with appropriate COM initialization and timeouts/isolation strategy.
- Handle providers that hang, disappear, expose no text pattern, or return empty/oversized ranges.
- Define how multiple selected ranges are represented; do not silently join unrelated text without a visible policy.
- Reject password/protected controls and excluded applications.
- Avoid capture from Pickle's own windows.
- Treat optional selection bounds as untrusted geometry and clamp placement to the chosen display's work area.
- Offer paste or the browser extension when capture is unsupported.
- Do not synthesize Copy or mutate the clipboard as an undocumented fallback.

Prototype target applications include Edge, Chrome, Firefox, Notepad, a common Office editor, a PDF reader, and an Electron editor. These are test targets, not support claims. Record versions, settings, success/failure, and fallback behavior.

### 7.2 Invocation and floating windows

- Start with a configurable conventional shortcut supported by Rune's Windows shortcut service.
- Choose the shipping Windows default after testing conflicts and accessibility. Do not automatically map macOS Control + Option alone to a Windows modifier gesture; Ctrl+Alt can interact with AltGr and international text entry.
- If modifier-only invocation is required on Windows, evaluate a native recognizer separately and test typing, repeat, interruption, layout, and security implications.
- Implement a system-tray menu and predictable single-instance/reopen behavior.
- Verify nonactivating display, foreground handoff for typing, Escape, click dismissal, and taskbar visibility on actual Windows installations.
- Account for foreground activation restrictions rather than repeatedly forcing focus.
- Validate per-monitor DPI changes, mixed scaling, taskbar positions, multiple displays, and monitor removal.

### 7.3 Windows privilege and credential boundaries

Normal reading from ordinary applications should work as a standard user. Do not make administrator execution or `uiAccess` the default workaround for capture problems.

UI Automation access to elevated or protected applications is restricted. Establish an honest unsupported/elevated-source state. If access to elevated applications becomes a product requirement, assess Microsoft's eligibility, signing, installation-location, and security requirements before changing the manifest.

Use a Windows credential vault implementation appropriate for the chosen desktop packaging model. Verify that it works for a standard user, after app updates, and after reinstall. Preserve endpoint scoping and explicit credential removal. Do not fall back to plaintext files when the vault is unavailable.

### 7.4 Screenshot, OCR, and audio

Evaluate Windows Graphics Capture or another supported native capture path for the selected window. Preserve source-target validation, screenshot bounds, explicit consent, cancellation, and the absence of a whole-desktop fallback.

Select an OCR implementation only after checking Windows version, package identity, language, runtime, licensing, and offline behavior. Some Windows APIs require package identity; that dependency can affect installer selection.

Source-app audio and on-device transcription are separate investigations. Verify process-specific capture availability, OS-version requirements, transcription model/language availability, and resource costs. Do not substitute microphone recording, all-system loopback, or cloud speech without an explicit product decision and accurate disclosure.

Return a capability-specific unavailable state when the OS, recognizer, or source app cannot support the feature. A failure must not silently widen capture scope.

## 8. Shared AI pipeline and rendering

### 8.1 Behavior to preserve

- Validate source, context, questions, history, and provider configuration before sending a request.
- Keep source snapshots immutable through the conversation.
- Distinguish source material from prior generated answers and untrusted supplemental context.
- Preserve Local versus Online choice and strict-offline restrictions.
- Treat a loopback SSH tunnel to another computer as remote-device inference for offline policy.
- Disable cloud checks in Local mode.
- Never silently switch providers when a server is unavailable.
- Keep evaluator outages and inconclusive results distinct from successful checks.
- Permit at most one prose repair followed by the complete relevant recheck.
- Validate every chart before display; do not stream unvalidated chart structures.
- Commit history only from the accepted final-result path.

### 8.2 Existing limits to port and verify

Use current code as the source of truth if it differs from older documentation. Initial parity targets include:

| Item | Existing baseline |
|---|---:|
| Selection | 12,000 UTF-8 bytes |
| Manual context | 6,000 UTF-8 bytes |
| User question | 2,000 UTF-8 bytes |
| Output text | 12,000 UTF-8 bytes |
| Follow-up history | 6 turns and 16,000 UTF-8 bytes |
| Provider response body | 256,000 bytes |
| Concurrent reading pipelines | 1 |
| Prose repair attempts | 1 |
| Chart bars / nodes / edges | 12 / 10 / 16 |
| OCR / visual summary | 6,000 / 3,000 UTF-8 bytes |
| Retained screenshot | 750,000 bytes and 1,440-pixel longest edge |
| Context readiness wait | Approximately 5 seconds with passage-only fallback |
| Local configured context / output | 8,192 / 1,024 tokens |
| Local conservative prompt budget | 6,656 UTF-8 bytes including schema and instructions |
| Explicit bookmarks / history entries | 200 / 1,000 |
| Source-app audio | 30 seconds |

Preserve provider-specific deadlines rather than replacing them with one global timeout: the current online request budget is 40 seconds, while Local generation uses a longer resource deadline. Keep bounded reads during streaming, not only a length check after buffering the entire response.

### 8.3 Provider and streaming work

- Port Cloudflare prose and structured response envelopes, reported model identity, usage aliases, and schema-capable chart routing.
- Port Jev's direct, REST-wrapped, and Completed-job response handling and typed-answer validation.
- Keep evaluator questions and provisional policy thresholds independently testable.
- Port Ollama metadata validation, cloud-alias rejection, local model capability checks, NDJSON completion handling, and prompt budgeting.
- Preserve malformed/truncated stream failures and UTF-8 handling across arbitrary transport chunks.
- Make HTTP redirect, cookie, cache, proxy, certificate, and response-budget policies explicit and test their intended behavior.
- Verify cancellation closes client consumption and invalidates state without claiming guaranteed upstream cancellation.
- Keep real-provider checks opt-in, fixed-fixture, bounded, and clearly potentially billable.

### 8.4 Rune reader and charts

Build the compact chooser, source disclosure, result area, follow-up composer, quality labels, consent prompts, context preview, settings, bookmarks, and knowledge-base views in Rune.

For Markdown, configure or preprocess output to preserve Pickle's supported formatting policy. `rune-kit` supports a broader feature set than Pickle currently allows; adopting it must not implicitly enable remote images, automatic link opening, executable content, or unrestricted embedded diagrams.

Use selectable text for word explanation and copy. Test selection across formatted paragraphs, lists, streaming updates, and review/repair replacement. Preserve text alternatives and evidence labels for charts. A bar chart should still resolve model-selected evidence IDs to locally extracted numbers. Flow relationships must retain their validation and visible limitations.

Do not adopt arbitrary Mermaid generation as a shortcut around the chart contract. Rune's diagram capabilities can be evaluated later behind the same validated model.

## 9. Context, browser bridge, and privacy

### 9.1 Context lifecycle

- Resolve the source target before showing the panel.
- Capture at most the permitted source scope for that session.
- Reuse captured context for retries/follow-ups instead of repeatedly recapturing.
- Treat OCR, web text, captions, and visual summaries as untrusted material with provenance labels.
- Keep OCR and model-derived visual summaries outside deterministic chart evidence.
- Do not allow late context to regenerate an answer or enter a newer session.
- Discard retained images and context on source replacement or explicit removal.
- Make destination-specific sharing consent accurate for Local servers and Online providers.
- Keep the bundled demonstration network-free.

### 9.2 Browser extension

Keep the existing Manifest V3 extension and extraction fixtures unless a specific incompatibility requires a change. Port or replace the Python native messaging host for shipping; avoid requiring users to install Python merely to use a packaged desktop app.

Host registration is OS- and browser-specific. Implement Mac manifest locations and Windows registration/discovery through the installer or explicit setup action. Support uninstall cleanup without removing another browser host or unrelated settings.

Preserve native message framing, size limits, authentication, freshness, duplicate rejection, URL validation, and source identity checks. If retaining the loopback listener, bind only to loopback, authenticate requests, and test port conflicts and multiple instances. Do not turn it into an unauthenticated localhost HTTP API.

Browser identity derived from host registration is part of the trust boundary; do not trust a page-supplied browser name. Caption excerpts must retain timestamps, scope, truncation, and availability labels.

### 9.3 Public webpage fetching

The existing implementation depends on macOS curl and a restricted WKWebView parser. Choose either a shared Rust extractor or separate isolated native extractors after proving their behavior on the same fixtures.

Retain public-address validation, DNS/rebinding protections, transport byte/deadline limits, original-host TLS verification, explicit redirect policy, and restrictions on scripts/subresources. An HTTP client port that checks a hostname once and then resolves it differently during connection is not equivalent to the current pinned-destination policy.

### 9.4 Persistence and diagnostics

- Persist explicit bookmarks and opt-in completed-answer history only according to their documented contracts.
- Do not save screenshots, OCR, audio, reference bodies, prompts, or credentials in history.
- History remains a log, never an implicit future prompt source.
- Use platform application-data directories and user-restricted permissions/ACLs.
- Keep diagnostics free of passage text, answer text, page titles, URLs, tokens, and private endpoint identifiers unless the user deliberately exports a clearly described report.
- Preserve invalid/unknown-version data and block replacement writes until recovery is explicit.

## 10. Data and configuration migration

### 10.1 macOS migration

Inventory existing preferences, explicit bookmark data, knowledge-base JSON, saved window state, and Keychain service/account identifiers. The current knowledge-base location is under the user's Application Support directory at `Pickle/KnowledgeBase/entries.json`; inspect the actual schema in `KnowledgeBase.swift` before implementing import.

Migration requirements:

1. Read legacy data through explicit schema/version adapters.
2. Validate sizes, enums, dates, identifiers, and strings before conversion.
3. Preserve source files and create a recoverable backup before any production rewrite.
4. Make migration idempotent; reopening the app must not duplicate entries.
5. Preserve the off-by-default history preference and distinguish bookmarks from automatic history.
6. Do not infer that old cloud consent also grants a new destination or broader capture scope.
7. Preserve credential scoping, and request authorization or re-entry when the OS requires it.
8. Test rollback to the old application without corrupting either application's data.

Decide whether the new app reads legacy storage directly or imports into a new versioned store. Do not let old and new applications write incompatible formats to the same file.

### 10.2 Windows first-run configuration

Windows starts with platform defaults and no inherited macOS permission or credential state. Provide setup for shortcuts, AI location, credentials, supported browser integration, and available context features. A user-specific Ross tunnel is an existing deployment example, not a general shipping default or bundled infrastructure.

### 10.3 Concurrent writes and updates

Use atomic replacement plus an explicit single-writer or locking strategy. File timestamps alone do not prevent simultaneous read-modify-write races. Test multiple process launches, crash recovery, failed writes, full storage, schema upgrades, and downgrade behavior.

## 11. Implementation phases and exit gates

Each phase produces reviewable artifacts and explicit pass/fail evidence. Checked items below identify implemented deliverables with the scoped evidence stated alongside them; they do **not** pass a phase's native or release exit gate. Unchecked items can contain implemented portions whose full required validation remains open. See the current execution records linked above for exact revisions and outcomes. Section 15 remains the unpassed public-release checklist.

### Phase 0: Workspace and release foundation

Deliverables:

- [x] Add the Rust workspace beside the Swift implementation.
- [x] Pin Rune, patches, toolchain, and lockfile.
- [x] Add platform interfaces and fake implementations for deterministic tests.
- [x] Add a network-free Rune sample scene and headless export commands.
- [x] Add native Mac and Windows build jobs.
- [x] Create separate development application identities and data directories.
- [x] Produce minimal launchable application bundles/artifacts for both systems.
- [x] Record proposed OS/architecture support and the machines used to validate it.

Implementation evidence: the [workspace manifest](../rune/Cargo.toml), [toolchain](../rune/rust-toolchain.toml) and lockfile pin the application and Rune patches. [Platform boundaries](../rune/crates/pickle-platform/src/lib.rs), [fake invocation resources](../rune/native/macos/tests/InvocationFixture.swift), injected browser observations and owned synthetic UIA providers exercise native contracts without personal inputs. [Scenes](../rune/crates/pickle-app/src/scenes.rs) isolate headless startup from live services and archives. [Packaging and CI](../.github/workflows/rune.yml) produced both development and optimized artifacts at the [verified `a4b4b93` checkpoint](RUNE-NATIVE-ACCEPTANCE.md#current-verified-development-checkpoint--23-november-2026). The Mac bundle identity and `PickleRune` data directory are separate from Swift Pickle. The [build requirements](../rune/README.md#build-and-run) and native record distinguish proposed macOS 14/Windows 11 targets from observed Apple Silicon macOS 26 and hosted Windows Server execution.

Exit gate: a clean checkout builds on both operating systems, the sample renders headlessly, and packaged prototypes open on clean test systems. No cross-app permissions or live AI are needed for the sample.

Exit status: **open for the declared device matrix**. Build/render/package evidence and observed development launches do not establish oldest-supported macOS or clean Windows 11 standard-user acceptance.

### Phase 1: Native shell and cross-app proof of feasibility

Deliverables:

- [x] Implement menu-bar/system-tray actions and background lifetime.
- [x] Implement supported-source selection capture on both systems.
- [x] Implement native invocation, source capture before focus, and immutable snapshots.
- [x] Implement compact chooser and in-place expansion into a reader.
- [x] Demonstrate a fixed-fixture or mock-stream answer through the actual application path.
- [x] Implement close, cancel, pause, reopen, and single-instance behavior.
- [ ] Validate real keyboard and mouse interaction, including follow-up text entry.
- [ ] Test supported browsers, a native editor, mixed-DPI displays, and full-screen behavior.
- [x] Record unsupported sources and their paste/extension fallback.

Implementation evidence: [native shell/invocation integration](../rune/crates/pickle-app/src/shell.rs), [selection/context admission](../rune/crates/pickle-app/src/context.rs), [request control](../rune/crates/pickle-app/src/controller.rs), [single-instance ownership](../rune/crates/pickle-app/src/instance.rs) and lifecycle fixtures implement these paths. The [native record](RUNE-NATIVE-ACCEPTANCE.md) preserves scoped Mac selection, conventional invocation, follow-up entry, dragging/reopen observations and hosted Windows selected-source → shortcut → chooser → consent → fixed loopback answer → ordinary quit evidence. [Setup guidance](../rune/README.md) records protected/elevated-source limits and manual passage/extension alternatives. These observations do not complete the unchecked interaction/application/display matrix.

Exit gate: installable Mac and Windows prototypes complete select → invoke → choose → read → dismiss/reopen without losing source identity, modifying the document, or leaking stale output. No unresolved framework issue may prevent that core interaction on either platform.

Exit status: **open**. Narrow native flows passed, but the complete keyboard/mouse, source-application, focus/full-screen, mixed-DPI and supported-client matrix is not accepted. Current Mac GUI/grants and Windows 11 standard-user execution remain separate from hosted component evidence.

Go/no-go decision: continue with Rune only after reviewing native evidence from both systems. If a blocker appears, isolate it with a minimal reproduction, determine whether a bounded adapter or pinned framework patch resolves it, and revisit the framework decision if it does not. A Mac-only success is not a pass.

### Phase 2: Shared reading pipeline

Deliverables:

- [x] Port models, budgets, errors, session/request identity, and policy revisions.
- [x] Port Ollama and one complete streaming Simplify workflow.
- [x] Add Expand, follow-ups, adjustments, and contextual word explanation.
- [x] Port Cloudflare, Jev, typed evaluator responses, and bounded repair/recheck.
- [x] Port bar/flow validation and Rune renderers with accessible alternatives.
- [x] Preserve provider choice, strict offline, local prompt budgeting, and no-fallback behavior.
- [ ] Translate existing behavioral fixtures into Rust tests and document intentional differences.

Implementation evidence: [core pipeline fixtures](../rune/crates/pickle-core/tests/pipeline.rs) exercise typed checking, one repair/full recheck, checker outages, cancellation and chart refusal; core chart/markup fixtures and Rune scenes cover source evidence and text alternatives. [Provider implementations/tests](../rune/crates/pickle-providers/src/) cover bounded HTTP streams and local budgets. Controller/Runtime tests cover word explanation, follow-ups, immutable source and policy revisions. The [implementation status](RUNE-IMPLEMENTATION-STATUS.md#reading-providers-and-validation) separates those deterministic contracts from real-provider integration; the native record also contains completed Local Simplify/follow-up/bar flows. Exhaustive one-by-one equivalence with all Swift fixtures is **not** established. Model-generated factual fidelity is not implied by a successful request or quality label.

Exit gate: the same deterministic input/provider fixtures produce equivalent policy outcomes on both platforms; cancellation, malformed streams, evaluator outages, and unsupported charts cannot become accepted success states.

Exit status: **not signed off as exhaustive legacy parity**. Shared Rust fixtures pass on both CI targets at the verified checkpoint, but they do not substitute for the still-open complete Swift-fixture equivalence audit or native/model compatibility testing.

### Phase 3: Complete reader, storage, and migration

Deliverables:

- [x] Finish settings, consent, error/retry, source preview, theme, and window persistence.
- [x] Add explicit bookmarks and opt-in knowledge-base history.
- [x] Implement platform credentials and endpoint scoping.
- [x] Implement and test Mac legacy-data migration.
- [ ] Validate first-run and returning-user flows on both systems.
- [ ] Complete keyboard, screen-reader, text selection, and copy workflows.

Implementation evidence: [versioned storage](../rune/crates/pickle-app/src/storage.rs), [window-frame storage](../rune/crates/pickle-app/src/geometry_store.rs), reader/settings Runtime tests and archive/library regressions cover atomic writes, capacity/corruption refusal, consent, optional history and explicit Save. Credential tests exercise account/endpoint binding and staged-operation guards without reading a real vault. [Migration fixtures](../rune/crates/pickle-app/src/migration.rs) import JSON history and XML/binary-plist bookmarks, preserve source files, reject damaged/unknown data, deduplicate and exclude secrets/consent. These are implemented and tested contracts, not proof of arbitrary real-user upgrades. The native record preserves bounded Mac copy/selection/geometry and hosted Windows reading/keyboard observations; VoiceOver/Narrator, full keyboard coverage, real saved-data restart and cross-version device acceptance remain open.

Exit gate: ordinary reading and saved-answer workflows work on both systems; upgrade, corruption, capacity, denied-credential, and rollback fixtures pass without data loss or implicit consent expansion.

Exit status: **open**. Portable archive/migration tests and scoped native reading evidence do not complete both-platform first/returning-user, vault-denial, upgrade/rollback, saved-answer restart or assistive-technology acceptance.

### Phase 4: Context and feature parity

Deliverables:

- [ ] Implement bounded source-window capture and OCR on both systems.
- [x] Implement public reference fetching/extraction under the existing safety contract.
- [ ] Package browser-native messaging hosts and validate Chrome/Edge installation.
- [ ] Validate loaded-page and available-caption workflows on real sites.
- [x] Implement explicit visual analysis and destination-accurate consent.
- [x] Implement or explicitly classify source-app audio/transcription capabilities on each system.
- [x] Maintain a feature matrix with supported, unavailable, experimental, and deferred states.

Implementation evidence: [public article policy/extraction](../rune/crates/pickle-providers/src/article.rs), [explicit image request and model checks](../rune/crates/pickle-providers/src/vision.rs), [context actions and cancellation](../rune/crates/pickle-app/src/context.rs), [provider fixtures](../rune/crates/pickle-providers/src/tests.rs), and [destination-consent fixtures](../rune/crates/pickle-app/src/tests.rs). The extractor is a bounded Rust heuristic, not Mozilla Readability equivalence. [Development setup](../rune/README.md#context-tools) describes on-device Speech on Mac versus a separately configured trusted local whisper executable/model on Windows. The [feature matrix](RUNE-IMPLEMENTATION-STATUS.md#4-feature-and-behavior-matrix) keeps device/runtime acceptance and platform differences explicit; implemented audio adapters do not establish working hardware compatibility.

Native gate remains open across the declared matrix. At `87a3c6f`, the signed development-identity Windows release app passed real source-window capture/OCR through the ordinary shortcut, context collection and actual Local request: a word painted only into the owned window was required as untrusted page OCR, while the unsigned screen-off control excluded it. Both flows also passed Save, ordinary quit, normal relaunch and reopened detail with full cleanup. At `f94cdfc`, both debug/release identity readings and both screen-off controls passed, all four CI jobs were green (426 Mac / 459 Windows workspace tests; three ignored each), and all 15 retained Windows native reports passed. Package acceptance now requires the complete actual-reading report. The unrelated isolated self-child helper still fails before capture and is retained as a manual diagnostic, with portable collector tests kept. Current Mac grants/capture, actual extension delivery, browser-specific pixels/tab matching and real-site captions remain unverified at the required scope. Packaged hosts and controlled browser document checks do not prove an installed extension connection. The [scoped native record](RUNE-NATIVE-ACCEPTANCE.md#source-87a3c6f-real-source-window-ocr-and-persisted-reading-verified) retains passed workflows, failed attempts and their limits.

Exit gate: every advertised feature has device evidence on its advertised OS. Any temporary scope reduction is explicitly approved and reflected in the interface and release notes; it is not treated as full migration completion.

### Phase 5: Release candidate and distribution

Deliverables:

- [ ] Produce signed, notarized Mac artifacts and signed Windows installers/packages.
- [ ] Implement an authenticated, signed update mechanism and release channels.
- [ ] Validate clean install, update, rollback, uninstall, and reinstall.
- [ ] Test GPU initialization failure, offline startup, server outages, sleep/wake, and display changes.
- [ ] Complete performance, compatibility, accessibility, privacy, and license reviews.
- [ ] Remove stale current-versus-historical documentation contradictions.
- [ ] Publish support boundaries, privacy behavior, setup instructions, and recovery steps.

Development scope: both platform packaging paths, fixture-based failure recovery, notice inventory, signed-manifest verification and bounded verified downloads exist. Their evidence is recorded in the [status report](RUNE-IMPLEMENTATION-STATUS.md#7-remaining-packaging-and-operational-gates) and [release runbook](RUNE-RELEASE-RUNBOOK.md). They do not establish a production publisher, published update channels, clean supported-client installation, screen-reader acceptance, sleep/wake/display behavior or the full performance matrix. Production distribution and public publication are deferred by the user's scope decision; these boxes remain open rather than treating CI test signing as public-release trust.

Exit gate: the release checklist in Section 15 passes on both operating systems and the remaining known issues are explicitly accepted for the declared release scope.

## 12. Testing strategy

### 12.1 Shared deterministic tests

Use the existing tests as a behavior inventory. Map each current check to a Rust test or a documented replacement; do not claim parity from matching test counts alone.

Coverage areas:

- Evaluator selection, missing context, limited answers, repair limits, and rechecks.
- Honest checked/concern/uncertain/unavailable labels.
- Bar quantities, incompatible units, finite numbers, unknown fields, and source evidence.
- Flow IDs, edges, conditions, omitted steps, and evidence.
- SSE/NDJSON chunk boundaries, Unicode, malformed input, truncation, and output limits.
- Provider endpoint construction, envelopes, error classification, usage, and model routing.
- Source fixation, stale callbacks, mode changes, cancellation, and late context.
- No cloud traffic in Local/strict-offline paths.
- History commit rules, duplicate suppression, capacity, corruption, and concurrent writes.
- Bridge/protocol framing, authentication, size, freshness, and duplicate rejection.

### 12.2 Rune headless UI tests

Create named deterministic scenes for compact selection, generating, streaming, reviewing, repairing, completed prose, charts, missing consent, provider failure, permission denial, empty history, and damaged history.

For each relevant scene:

- Inspect element keys, names, roles, focusability, and actions.
- Drive buttons and fields with scripted input.
- Assert cancel/retry/follow-up commands and resulting state.
- Check layout at minimum, typical, and large window sizes and multiple scales.
- Include long words, multiline source text, large fonts, and international text.
- Use controlled platform/font baselines for screenshots; avoid assuming identical pixels across every OS and GPU.

Headless tests cannot prove OS focus, actual permissions, taskbar/tray integration, screen-reader operation, or capture from another process. Keep those as separate native tests.

### 12.3 Native compatibility matrix

| Area | macOS checks | Windows checks |
|---|---|---|
| Browsers | Safari, Chrome, Edge as selected | Edge, Chrome, Firefox as selected |
| Editors | TextEdit and a representative editor | Notepad, an Office editor, an Electron editor |
| PDF | At least one selected PDF reader | At least one selected PDF reader |
| Invocation | Modifier-only gesture and configurable shortcut | Conventional shortcut, conflicts, AltGr/international layouts |
| Capture protection | Secure controls, exclusions, revoked AX access | Password/protected controls, exclusions, elevated sources |
| Windowing | Spaces, full screen, Dock, multiple displays | Foreground restrictions, taskbar, virtual desktops, mixed DPI |
| Assistive technology | VoiceOver and keyboard-only | Narrator and keyboard-only; NVDA if included in support |
| Credentials | Fresh save, denied access, rebuild/update | Standard user, save/read/remove, update/reinstall |
| Capture/transcription | Screen permission, source scope, recognizer availability | Capture availability, source scope, recognizer/runtime availability |
| Lifecycle | Launch, reopen, sleep/wake, quit | Login/start, duplicate launch, sleep/wake, quit |

Record application and OS versions, hardware, build revision, steps, result, limitations, and the fallback. A pending row is not a support claim.

### 12.4 Performance and reliability

Measure release builds on both systems. Capture process startup, time to chooser, first visible draft, final result, local-model verification overhead, idle CPU/GPU activity, memory after repeated sessions, resizing, and history browsing.

Separate application overhead from model/network latency. Do not treat short fixed-fixture timings as a model-quality or performance benchmark. Establish numerical budgets after baseline measurements and enforce regressions against those budgets.

Run bounded soak tests for repeated capture/cancel/reopen, fast streaming, provider disconnects, rapid mode changes, and context capture. Include failure to initialize a GPU adapter; surface an actionable launch failure instead of leaving an invisible background process.

## 13. Build and CI plan

- Run formatting, shared unit tests, linting, and fixture validation on pull requests.
- Build/check native Mac and Windows targets on every relevant change.
- Keep native libraries and bridge linking in the target-specific jobs.
- Run Rune headless rendering in supported runners with controlled graphics configuration; verify the actual adapter/backend used.
- Run permission/focus/cross-app checks on dedicated interactive machines, since ordinary hosted runners may not provide the required desktop state.
- Run full native compatibility and packaging checks before release candidates and after Rune upgrades.
- Build signed release artifacts only from reviewed tags/commits in protected jobs.
- Keep signing keys, provider tokens, update credentials, and private local-server settings out of repository files and public logs.
- Record source commit, Rune revision, toolchain, lockfile, architectures, hashes, and test results with each artifact.
- Verify a clean build from a fresh checkout without relying on the developer's temporary Rune clone or Cargo target directory.

Audit framework/dependency licenses, bundled fonts, Readability, native libraries, installer components, and any future OCR/transcription models. Produce notices and an inventory of shipped dependencies.

## 14. Packaging, signing, updates, and support

### 14.1 macOS

- Build a `.app` with stable production bundle identity and accurate usage descriptions.
- Include the Rust executable, required resources, and any native bridge libraries/runtime dependencies.
- Sign nested code and the app with the appropriate Developer ID identity and hardened-runtime configuration.
- Notarize, staple, and verify the deliverable through the selected distribution workflow.
- Test a downloaded artifact on a clean Mac, including quarantine, first launch, permissions, credentials, and update continuity.
- Choose a DMG/ZIP or installer workflow appropriate for the audience and publish installation/removal instructions.
- Preserve the ability to recover from a failed update without overwriting valid user history.

### 14.2 Windows

Choose packaging after the Phase 1 native prototype and context API investigation establish identity/runtime needs:

| Option | Appropriate use | Work to validate |
|---|---|---|
| Signed EXE/MSI installer | Direct desktop distribution and explicit installer control | Runtime dependencies, shortcuts/tray setup, host registration, update/uninstall |
| MSIX | Package identity and managed installation/update workflows | Capability compatibility, file/data paths, browser-host registration, upgrade behavior |
| Installer plus external-location identity | Traditional installation needing identity-gated APIs | Added registration complexity and lifecycle consistency |

Do not assume Microsoft Store acceptance or Mac App Store suitability. Direct signed downloads can be the first distribution channel. Store distribution requires its own technical/policy review and validation.

Sign the chosen Windows artifacts, validate publisher identity and standard-user installation behavior, and test clean VMs. Test missing runtimes, antivirus interactions, launch shortcuts, browser-host registration, update replacement, and uninstall cleanup. Code signing does not guarantee the absence of reputation warnings on every machine.

### 14.3 Updates

Define stable and beta channels, signed metadata/artifacts, version compatibility, verification before installation, and atomic or recoverable replacement. Keep application updates separate from local-model downloads and provider configuration.

Test interrupted downloads, bad signatures, insufficient storage, a running application, data-schema incompatibility, and rollback. Select a maintained updater or platform mechanism only after confirming it fits Rune/native packaging and supports both operating systems' signing requirements.

### 14.4 User onboarding and support

- Explain Local, self-hosted-on-another-computer, Online, and strict offline in user terms.
- Let users see where their selected material will go before sending it.
- Provide a network-free demonstration that verifies the interface independently of provider setup.
- Offer clear permission recovery and unsupported-app fallback instructions.
- Explain browser extension installation and caption availability honestly.
- Provide a content-free diagnostic report with explicit user review/export.
- Document which data uninstall removes, which history is retained, and how credentials can be removed.

## 15. Release acceptance checklist

### Product and platform

- [ ] Mac and Windows builds complete the same advertised reading workflows.
- [ ] Supported OS versions, architectures, and applications have recorded native evidence.
- [ ] Cross-app invocation preserves the selected source before focus changes.
- [ ] Unsupported/protected sources fail clearly and provide safe alternatives.
- [ ] No advertised feature depends on a developer-only machine, tunnel, toolchain, or secret.
- [ ] Feature differences between platforms are explicit and accepted.

### Correctness and privacy

- [ ] Relevant legacy behavior is covered by passing shared and native tests.
- [ ] Cancellation and policy changes reject late drafts, results, and context.
- [ ] Local and strict-offline settings prevent forbidden destinations.
- [ ] Charts preserve deterministic evidence validation and text alternatives.
- [ ] Quality labels do not imply fact verification or calibrated accuracy.
- [ ] Persistence, migration, corruption recovery, and rollback tests pass.
- [ ] Diagnostic and crash-report behavior respects the content policy.
- [ ] Secrets stay in platform credential storage.

### Shipping and operations

- [ ] Clean-machine installation and first launch pass on both platforms.
- [ ] Signing/notarization or Windows package verification passes as applicable.
- [ ] Update, failed update, rollback, uninstall, and reinstall pass.
- [ ] Screen readers and keyboard-only workflows pass the declared support matrix.
- [ ] Performance budgets and soak tests pass on representative hardware.
- [ ] Release artifacts include version/build provenance and required notices.
- [ ] Setup, privacy, support boundaries, and recovery documentation match the implementation.
- [ ] A release owner reviews remaining known issues and accepts the declared scope.

## 16. Risks and mitigations

| Risk | Consequence | Mitigation / decision gate |
|---|---|---|
| Rune prerelease/API changes | Build or behavior regressions | Pin revision and lockfile; isolate Rune-facing code; test upgrades |
| Windows UI Automation gaps | Inconsistent cross-app capture | Early application matrix, bounded capture, extension/paste fallback |
| Native focus/activation differences | Lost selection or unusable follow-ups | Capture before showing; real native tests on both systems in Phase 1 |
| macOS panel integration regressions | Incorrect geometry, focus, Spaces behavior | Test the actual pinned panel path; minimal reproductions for framework fixes |
| Bridge lifetime/thread mistakes | Crashes, leaks, stale callbacks | Small versioned ABI, explicit ownership, cancellation and shutdown tests |
| Context/audio API differences | Feature parity delays or widened capture | Capability interfaces, narrow scope, explicit availability and release matrix |
| Windows package-identity requirements | Installer redesign late in the project | Investigate capture/OCR/transcription dependencies before packaging choice |
| History or credential migration failure | Lost data or blocked returning users | Idempotent import, recoverable backups, identity tests, rollback fixtures |
| Weak local model output | Meaning errors despite working software | Honest labels, preserved validators, model evaluation separate from code tests |
| Cross-platform rendering assumptions | Unreadable or inaccessible UI | Native screen-reader, DPI, text-layout, and GPU testing |
| Developer-specific defaults | App unusable for new users | Neutral onboarding, explicit endpoint configuration, clean-machine tests |
| Scope expands before core proof | Long rewrite without a shippable product | Two-platform Phase 1 gate and reviewable phase deliverables |

## 17. Decisions to record during implementation

The following require an explicit recorded decision before the phase that depends on them:

1. Exact initial OS versions and CPU architectures.
2. Minimum supported application/browser matrix.
3. Windows shortcut default and whether modifier-only invocation is needed there.
4. Static versus dynamic packaging of the initial Swift bridge.
5. Production bundle/package IDs and migration strategy.
6. Windows credential, OCR, capture, and local transcription implementations.
7. Windows installer/package model and first distribution channels.
8. Update mechanism and release-channel ownership.
9. Which context/audio features are release-blocking versus explicitly experimental.
10. Whether to port the browser native host to Rust or package an alternative self-contained host.
11. Framework upgrade policy, patch ownership, and response to upstream blockers.
12. Model/provider onboarding, defaults, and any application-managed cloud service; do not assume a shared embedded API key.

Routine implementation choices can proceed within the agreed architecture. Decisions that change supported platforms, data-sharing scope, capture scope, distribution identity, or advertised features need review before they become release assumptions.

## 18. Immediate implementation sequence

1. Create the parallel Rust workspace with pinned Rune dependencies and a sample scene.
2. Add Mac and Windows native builds and minimal packaged artifacts.
3. Define selection, invocation, credential, shell, and capability interfaces with fake adapters.
4. Connect the Mac selection/gesture bridge and Windows UI Automation capture.
5. Implement one Rune chooser/reader panel and preserve its identity while expanding.
6. Exercise the complete flow with a mock stream on real applications on both systems.
7. Record the Phase 1 evidence and framework go/no-go decision.
8. Port the shared providers and pipeline only after that decision passes.

The first substantial milestone is two installable prototypes demonstrating the real cross-app reading interaction. A screenshot-only recreation of the current reader is insufficient evidence for the shipping goal.

## 19. References

### Rune sources at the investigated revision

- [Repository overview and prerelease guidance](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/README.md)
- [Core documentation and dependency setup](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/crates/rune/README.md)
- [Workspace manifest and patches](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/Cargo.toml)
- [Rust toolchain](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/rust-toolchain.toml)
- [Hello example](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/examples/hello/src/main.rs)
- [Launcher example](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/examples/launcher/src/main.rs)
- [Background app example](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/examples/background/src/main.rs)
- [Tray implementation and platform availability](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/crates/rune/src/tray.rs)
- [Shortcut parser](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/crates/rune/src/shortcuts/parse.rs)
- [Shortcut platform behavior](https://github.com/anomalyco/rune/blob/67d73a5f18390686c8e9b8b3a32b3414ebd66db0/crates/rune/src/shortcuts/README.md)

### Platform references

- [Windows UI Automation selected text](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextpattern-getselection)
- [Windows UI Automation security considerations](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview)
- [Windows packaging options](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/packaging/)
- [Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)

### Existing Pickle behavior and migration sources

- [Architecture and policies](ARCHITECTURE.md)
- [Testing and compatibility matrix](TESTING.md)
- [Local AI behavior and deployment](LOCAL-AI.md)
- [Provider contracts and historical verification](PROVIDERS.md)
- [Current distribution notes](DISTRIBUTION.md)
- [Current request coordinator](../Sources/PickleApp/Coordinator.swift)
- [Current request pipeline](../Sources/PickleCore/Pipeline.swift)
- [Current selection capture](../Sources/PickleApp/SelectionService.swift)
- [Current settings and credentials](../Sources/PickleApp/Settings.swift)
- [Current bookmarks](../Sources/PickleApp/Bookmarks.swift)
- [Current knowledge-base schema and store](../Sources/PickleCore/KnowledgeBase.swift)
- [Browser extension](../browser-extension/README.md)

Some existing documents contain historical statements superseded by later features. Resolve disagreements against current code and recorded tests, and reconcile those documents during release preparation.
