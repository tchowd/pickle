# Rune rebuild: implementation status and shipping gates

Snapshot: **22 November 2026**. This document compares the current checkout with the [cross-platform plan](RUNE-CROSS-PLATFORM-PLAN.md). It records implementation and observed evidence separately. The plan remains the acceptance contract; its unchecked tasks must not be treated as completed merely because corresponding code now exists.

The repository now contains a substantial Rust/Rune application, native adapters for both target platforms, portable policy/provider implementations, migration, packaging, and a signed-download verifier. **It remains a development build. Public macOS and Windows shipping readiness has not been demonstrated.** In particular, the plan's two-platform native feasibility gate remains open until real cross-app workflows pass on Windows as well as macOS.

## 1. How to read this report

| Status | Meaning |
|---|---|
| Implemented; deterministic evidence | Relevant source exists and automated fixtures exercise its contract. This does not imply OS/device compatibility. |
| Implemented; native gate open | Platform code exists, but the supported-device workflow still needs recorded execution. |
| Partial | Some required behavior exists, with a concrete missing part or intentional difference below. |
| Release gate open | Requires release infrastructure, production credentials, distribution review, or acceptance evidence. |

This is a source-and-execution snapshot, not a certification. At `334c6140a468e7b1d7066af616bcd25fdc7478b1`, [run 45093518479](https://github.com/tchowd/pickle/actions/runs/45093518479) passed both complete Mac jobs, including native invocation rollback fixtures. Windows ordinary NSIS/runtime/window/lifecycle checks passed; repaired temporary certificate trust allowed signed identity verification, installation and runtime identity detection. Both Windows profiles still failed the fixed OCR word test. The optimized probe selected `en-US`, received an empty string, and confirmed 5,406 dark pixels in its 768×180 CPU fixture. Bounded comparisons of bitmap round-trip/format/alpha/DPI and a smaller text sample are prepared; they cannot change the baseline failure into a pass. Optimized local Mac selection invocation also remains a failed native observation despite granted permissions; see the [native record](RUNE-NATIVE-ACCEPTANCE.md).

Earlier Windows-only lint/lock-fixture errors and the `0c53edc` notice checksum failure were fixed. Reviewed license bytes now survive Windows checkout, and multi-command notice tests fail immediately. Headless screenshots and native window responsiveness validate different paths from real cross-app capture; neither closes the complete native compatibility matrix.

## 2. Actual application structure

The original Swift app remains beside the new [`rune/`](../rune/) workspace. The implementation is divided as proposed:

| Area | Actual location and responsibility |
|---|---|
| Shared product policy | [`pickle-core`](../rune/crates/pickle-core/): immutable source snapshots, request limits, prompts, typed evaluation, cancellation, chart evidence, and safe answer markup. |
| Providers and browser transport | [`pickle-providers`](../rune/crates/pickle-providers/): Cloudflare, Ollama, Jev, bounded streaming, local prompt budgets, public article fetching, authenticated browser bridge, and Rust native-messaging host. |
| Native services | [`pickle-platform`](../rune/crates/pickle-platform/): macOS Swift/AppKit/AX/capture bridge; Windows UI Automation, tray, credentials, window capture/OCR, and process-audio adapters. |
| Rune application | [`pickle-app`](../rune/crates/pickle-app/src/): controller/state transitions, reader and settings, context lifecycle, native shell, storage, migration, single-instance service, update verifier and UI. |
| Packaging | [`scripts`](../rune/scripts/), [`packaging`](../rune/packaging/), and [CI](../.github/workflows/rune.yml): platform packages, reproducible icons, target-specific notices, and development installer smoke testing. |

Rune is pinned to `67d73a5f18390686c8e9b8b3a32b3414ebd66db0`, with required patched dependencies and a committed application lockfile. The declared toolchain is Rust `1.99.0`. Updating Rune or these patches requires repeating the native compatibility gates.

**Follow-up implementation:** reading-level controls, appearance/pin preferences, independent window-frame persistence, configurable invocation and automatic chooser behavior are now present. Automated evidence is identified below; native device gates remain open. The integrated local check script passed after these changes.

## 3. Phase assessment against the plan

| Plan phase | Implemented in the checkout | Exit-gate assessment |
|---|---|---|
| 0: Workspace and release foundation | Rust workspace, dependency/toolchain pins, headless scenes, separate development identity/data, platform build workflow, Mac and Windows package scripts. | **Partial.** Local Mac artifacts exist; clean-checkout native Windows build/package/runtime evidence and clean-machine launches still need recording. |
| 1: Native shell and cross-app feasibility | Menu bar/tray, invocation adapters, selection capture, chooser/reader, pause/exclusions, cancel/reopen, single-instance control, window placement. | **Native gate open.** Code and unit tests cannot establish select → invoke → choose → read → dismiss/reopen on both operating systems. This remains the framework go/no-go gate. |
| 2: Shared reading pipeline | Simplify/Expand/follow-ups/adjustments, provider routing, streaming, typed Jev checks, bounded repair, deterministic charts, local budgets and strict offline. | **Implemented; deterministic evidence.** Real configured providers, malformed production responses, latency and cost behavior need integration evidence. The original Swift fixtures have not been proven exhaustively equivalent one by one. |
| 3: Reader, storage and migration | Rune reader, settings/consent/retry, reading levels, text size/opacity/theme intensity, pinning, independent window preferences, OS vault interfaces, optional history, explicit bookmarks and guarded legacy import. | **Implemented; native gate open.** Accessibility/keyboard, real display restoration, upgrade and rollback still need packaged-device testing. |
| 4: Context and parity | OCR, explicit vision actions, public references, packaged Rust browser host, extension setup, source-app audio/transcription adapters. | **Implemented; native gate open.** Real browser, capture and transcription compatibility is unproven across the declared matrix. Platform differences must remain visible. |
| 5: Release candidate and distribution | Development packaging, NSIS installer, notice inventory, Ed25519 manifest verification and bounded verified downloads. | **Release gate open.** Production signing/notarization, signed feed publication, clean install/update/rollback/uninstall, performance and support acceptance remain. |

## 4. Feature and behavior matrix

### Reading, providers, and validation

| Requirement | Current behavior | Evidence or remaining work |
|---|---|---|
| Source identity and limits | Immutable snapshots retain original selected text, app metadata and bounded untrusted context; requests preserve the selected source through follow-ups. | Core tests cover byte/Unicode bounds, IDs, context exclusion from evidence, and prompt construction. Validate capture before focus changes on each OS. |
| Simplify, Expand, follow-ups | Reader supports initial actions, six follow-ups, Shorter, More detail, Give an example, and selected-word explanation. | Controller/core fixtures and headless scenes exist. Real text selection, input focus, copy, and keyboard operation remain native checks. |
| Streaming and cancellation | Bounded SSE/NDJSON paths, request/session identity, cancellation and stale-callback guards. | Local HTTP and controller fixtures exercise malformed streams and stale work. Test source change, pause, quit, sleep/wake and network loss during actual streaming. |
| Local provider isolation | Configured Ollama connection; model/capability verification and local input budgets; no automatic cloud fallback. | Deterministic HTTP/budget tests. Test actual chosen models and a separately configured remote server/tunnel. |
| Online and Jev | Cloudflare generation; optional typed quality evaluation; one repair followed by full recheck; unavailable checking is labeled honestly. | Evaluator distributions, malformed responses, routing and repair fixtures. No claim of factual verification or calibrated accuracy. Live paid inference is a separate explicit test. |
| Strict offline | Blocks Online AI, remote-device inference, public-page fetches and update traffic. Same-device inference requires explicit assertion and a loopback endpoint. | Policy fixtures exist; test network-observation scenarios in a release build. A loopback tunnel is not proof of local inference. |
| Deterministic bar/flow charts | Bars require selected-source numeric evidence; flows require literal source relations and valid source IDs. Auxiliary references cannot supply chart facts. | Core chart fixtures and accessible text rendering exist. Validate chart navigation and screen-reader output on both platforms. |
| Safe answer rendering | Markdown whitelist preserves basic prose, lists and emphasis; external link/image destinations, HTML, tables and active formatting are removed or escaped. Autolinking is disabled in the reader. | Dedicated markup tests cover rich/hostile input and plain-text list preservation. |
| Reading-level control | Settings exposes Automatic, Plain, Balanced and Technical choices; the controller includes the selected level in requests. Changing level cancels in-flight reading; appearance changes preserve it. | Shared prompt fixtures and preference runtime tests exist. Actual reading/provider behavior remains integration work. |

### Desktop shell and native services

| Requirement | macOS implementation | Windows implementation | Remaining evidence |
|---|---|---|---|
| Selection | Accessibility selected-text access with source identity/security checks. | Bounded UI Automation worker; protected/elevated sources are not claimed supported. | Native editor, Chrome, Edge, unsupported app and password/secure-field cases. No synthesized copy fallback. |
| Invocation | Optional Control + Option press/release recognizer plus configurable shortcut. | Configurable conventional shortcut, initially Ctrl + Alt + P. | Native conflicts, key repeat, interrupted gestures and AltGr/international layouts. Automatic selection previews preserve the working session until an action is chosen; this needs actual source-app testing. |
| Background shell | Accessory panel and status item. | System tray and floating window. | Focus handoff for typing, Escape/close, quit, taskbar behavior, full-screen/Spaces, sleep/wake. |
| Reopen and single instance | Shared file lock plus authenticated bounded loopback Reopen channel. | Same shared protocol with protected user-local metadata. | Deterministic tests cover auth rejection, contention, slow clients and crashed-primary recovery. Test installed second launch and OS reopen behavior. |
| Pause and exclusions | Policy checked before capture and requests, including context/audio. | Canonical executable/basename/browser aliases used consistently. | Tests cover stale/changed exclusions. Verify actual source identity and transient focus races. |
| Credentials | Keychain with explicit authorization UI for rebuilt binary access. | User credential vault. | Local endpoint and Online account scopes are distinct; an unscoped legacy cloud token is never automatically reused or deleted. Draft secrets are bound to the visible recipient and cleared on connection transitions. Standard-user vault behavior across updates/reinstall still needs native evidence. |
| Geometry | Source display/work-area positioning and logical/physical coordinate conversion; independent versioned window-frame storage. | Native work-area/display conversion and the same portable store. | Background loading/writing, corruption preservation, concurrent writers and pure clamping have fixtures. Real negative origins, mixed DPI, monitor removal and restored native placement remain device tests. Chooser previews cannot overwrite preferred reader dimensions. |
| Appearance | Reading text size, background opacity and theme intensity; explicit Pin/Unpin. | Same shared reader and native topmost toggle. | Preference/runtime fixtures exist. Pin defaults off and suppresses automatic previews when enabled. Verify desktop compositing, topmost behavior, contrast and system accessibility preferences on devices. |
| Accessibility | Rune accessibility retained; semantic labels and chart alternatives exist. | Same shared semantic content. | VoiceOver, Narrator, keyboard-only, scaling/contrast and accessibility-tree quality need device acceptance. |

### Context, reference, and audio

| Requirement | Current behavior | Remaining gate or difference |
|---|---|---|
| Source-window OCR | Explicitly enabled; screenshot retained in session; ordinary reading shares bounded OCR text rather than image bytes. macOS ScreenCaptureKit/Vision and Windows capture/OCR adapters exist. Unpackaged Windows builds report page OCR unavailable and reject the operation before capturing. | Microsoft's Windows OCR API requires package identity. The ordinary NSIS package cannot establish full context parity. Validate the MSIX/identity packaging path, installed OCR languages, permissions, source changes and cancellation before shipping this feature. [API requirement](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr). |
| Visual analysis | Separate explicit Cloudflare image-sharing action or configured local vision model, without cloud fallback. | Test actual image payload, capability rejection, destination-scoped consent and each supported model. |
| Public article fetch | Strict public URL checks; DNS validation and pinned destination, host TLS, byte/deadline caps, no redirect/script/subresource execution. | Portable extractor is heuristic Rust article extraction, not identical Mozilla Readability output. Real-site extraction parity and adversarial networking need release acceptance. |
| Extension and captions | Existing Chrome/Edge extension retained; bundled Rust native host; authenticated bounded loopback transport and browser/extension identity binding. | Actual load/connect/disconnect, loaded-page/caption availability, browser updates, copied host refresh and uninstall lifecycle. Captions depend on what the page exposes. |
| Source-app audio | Explicit bounded capture, in-memory audio, no microphone fallback. macOS uses available on-device Speech; Windows uses process loopback and a separately configured trusted local whisper executable/model. | Actual hardware/OS tests, unavailable recognizer/model, application-process scope, descendant processes/other tabs, denial/cancel and transcription runtime. This is a documented platform difference, not proven parity. |
| Context lifetime and consent | Context defaults off; destination-sensitive disclosure; source changes, manual commit, pause and policy changes clear/cancel stale context. | Regression fixtures exist. Repeated native capture/source-change and consent flows still need end-to-end execution. |

### Persistence and migration

| Requirement | Current behavior | Evidence or gate |
|---|---|---|
| Explicit bookmarks | Save current answer/passage; capacity 200. | Tests cover dedup, limits and corruption behavior. Follow-up saves do not inherit unrelated base chart metadata. |
| Optional history | Knowledge-base recording defaults off; accepted completed answers only; capacity 1,000. Refused/failed/chart-refusal results are not successful answer records. | Controller/storage regression fixtures. Test UI search/open/delete and restart with real packaged data. |
| Content minimization | Stores passage/answer and reference pointers, not complete reference bodies, screenshots, OCR, audio, prompts or credentials. | Schema/source inspection and fixtures; review future diagnostics/export additions separately. |
| Support diagnostics | Explicit Settings review of a typed allowlisted report, followed by a native Save dialog and background atomic private-file export. No automatic transmission. | Seven fixtures cover schema/content boundaries, reviewed-byte equivalence, full JSON accessibility, cancellation/stale/headless rejection and safe destinations. Native Mac export produced a 671-byte allowlisted report with mode 0600. Windows Save-dialog acceptance remains open. |
| Atomicity and corruption | Versioned archives, atomic replacement, cross-process locks; malformed/unknown archives block replacement. Settings writes are serialized. | Automated storage fixtures; actual crash/power-loss/permission-denial and rollback execution remain. |
| Legacy macOS import | Explicit import of history JSON and savedAnswers plist Data; preserves chart/reference metadata, validates all sources before writes, deduplicates and leaves legacy files untouched. | Nine migration fixtures, including Swift date formats. Credentials and consent are deliberately not imported. Validate with a backed-up real legacy installation. |

## 5. Evidence observed locally

The following are bounded observations from this checkout, not a public release sign-off:

| Command or artifact | Observed result | What it does not prove |
|---|---|---|
| Workspace tests through `MACOSX_DEPLOYMENT_TARGET=14.0 RUSTUP_TOOLCHAIN=stable bash rune/scripts/check.sh` | **239 passed, 0 failed, 2 ignored**. Breakdown: app 122; core 44; platform 29; providers 43; browser-host integration 1. | Native Windows execution, all OS permissions, real UI focus, live AI/model compatibility, or release packaging. |
| Ignored tests in that command | Signing-script/OpenSSL manifest integration and live public article smoke are excluded from the normal test count. Both were then run explicitly and passed independently at `dae7880`: the signing fixture used a publicly known test-only key; the article fixture fetched `https://example.com/` without calling an AI provider. | These do not prove production publisher-key management, update hosting or real-site extraction parity. |
| Full local check script | Workspace formatting, all-target checks, strict workspace Clippy, Rust tests, icon formatting/Clippy/three tests, ten notice tests, eight PE-runtime verifier tests, four gateway tests, browser extraction/four legacy-host tests and seven UI renders passed. Controlled native Vision/JPEG/geometry and invocation registration/rollback fixtures passed in the Mac branch. | A Mac run does not prove Windows linking, installation or native interaction. |
| Geometry integration tests | 14 passed: 12 store/worker/clamping fixtures and two controller privacy/lifecycle regressions. Background writes coalesce; ordinary quit flushes after the event loop. | Actual native close/quit, forced OS session termination, display restoration and automatic chooser placement. |
| Standalone icon-builder tests | Three tests passed; generated ICO/ICNS recognized by file tooling; ICNS decoded at 1024 px. | Actual Windows Explorer/tray rendering or every shell icon cache behavior. |
| `python3 rune/packaging/test_notices.py` | Ten tests passed, including a real Git checkout with Windows newline conversion enabled and strict rejection of changed notice bytes. | Distribution/legal approval of the app or every non-Cargo component. |
| Strict notice generation, macOS arm64 | 385 reachable normal/build packages, 257 distinct full notice texts, zero unresolved inventory gaps, including native Save dialog support. | Actual production feature/artifact equivalence or license-owner review. |
| Strict notice generation, Windows x64 MSVC | 362 packages, 254 distinct full notice texts, zero unresolved inventory gaps, including native Save dialog support. | Windows build/runtime or installer component redistribution review. |
| `node Tests/BrowserExtensionTests/extract.test.js` | Passed video-window, caption/transcript, unavailable-source, host-boundary and Unicode-limit fixtures. | Actual Chrome/Edge native messaging or changing video-site DOMs. |
| Local Mac development bundle | Rebuilt arm64 development bundle passed strict codesign verification with ad hoc signing. Native UI: sample chooser → answer → pin → close → Finder reopen preserved the answer and pin state. The explicit accessory-app reopen handler resolved the observed reopen failure. | No Developer ID team, notarization, Gatekeeper acceptance or clean-machine launch. Later changes must be rebuilt and retested; this observation is not a release sign-off. |
| Permission probes | An earlier command-line probe reported false; the installed GUI diagnostics later reported Accessibility and screen-capture permission **true** after the tester granted access. | Permission responsibility can differ between a CLI invocation and GUI launch. Use the installed GUI process and actual capture results to assess access; do not interpret the earlier CLI values as a failed grant. Capability flags alone are not capture evidence. |
| Controlled Mac source | Native capture retained the fixture's selected Café paragraph and source identity, excluding surrounding paragraphs. The GUI displayed a completed Local Explain-more response. | The response invented unsupported background details; successful transport is not answer-quality validation. See the [native record](RUNE-NATIVE-ACCEPTANCE.md). |
| Real Mac OCR attempt | An image of the correct source window was retained, but text recognition was empty and the preview appeared blurred/padded. OCR was restored off without continuing the sharing prompt. | This was a failed recognition test. Candidate SCK sizing/color changes pass controlled RGB/BGRA/dark-image Vision/JPEG and geometry fixtures, but real repaired capture still needs GUI retesting. |
| Optimized local Mac binaries at `0c53edc` | `cargo build --release --workspace --locked` passed; the app binary was 41,117,248 bytes, its version probe loaded, and `otool` showed only system/Swift libraries. | No optimized native GUI or package acceptance. Release correctly rejects `--snap` without Rune's tooling feature, so the attempted headless command produced no rendering/performance evidence. |
| Optimized local Mac bundle at `dae7880` | Release packaging/inspection and native launch passed. Repaired tint and note wrapping rendered correctly; diagnostic JSON exposed the exact commit. Both GUI permission flags became true after replacing stale Pickle-only privacy entries with the exact current bundle and relaunching. | A subsequent reported physical gesture still produced no chooser, so optimized invocation is a failed native observation and repaired OCR remains untested. OCR off/original gesture on/automatic off were restored and saved. This ad hoc bundle is not a signed/notarized public release. |
| Optimized CI packages at `dae7880` | Complete Mac optimized job passed. Ordinary Windows optimized NSIS installation, version/capabilities, exact owned native window/responsiveness, PE runtime audit, live guards, uninstall retry and ownership/data retention passed. | Windows then failed at trust validation for the disposable identity-test certificate; installed identity/OCR remain unverified. The separate Mac debug job timed out in the legacy Python host fixture. See exact artifact hashes and scope in the [native record](RUNE-NATIVE-ACCEPTANCE.md). |
| Headless reader/chooser artifacts | PNG and element-tree files produced from fixed scenes. | Native capture, nonactivating focus, real accessibility, tray menus or installed graphics/runtime dependencies. |

The standalone icon, Python and native Swift helper tests are outside the main Cargo workspace; they are not included in the 239-test figure. The legacy Swift baseline was rerun separately: `swift run PickleChecks` passed all 75 checks.

Seventeen named fixture scenes now cover pipeline stages, validated charts, consent, permissions and empty/damaged history. All fixture startup has no live Services, archive/lease initialization or provider/vault access; native-only permission, reveal and export actions are guarded. Local connection discovery tests use isolated HTTP fixtures, and recipient-bound credential/consent/transaction tests make no real vault calls.

The successful earlier macOS CI job built `aarch64-apple-darwin`. Its downloaded `PickleRune-macos.zip` SHA-256 is `81efeb4d7893a42ffa4fcc825316094c1602e96fc11d7b766018694d80444049`; the enclosing GitHub artifact digest is `sha256:8f902ce1579cca5d94b8d7253d58e299aa3906a7b6790bb49a0f44595891aafa`. These identify the earlier commit's development package, not later working-tree changes or a notarized release.

At `0c53edc0ea0cdb06fe3c7366501a011388003ecc`, the complete macOS job in run `45089162614` also passed, including the controlled native processing fixture and development packaging. Its downloaded inner `PickleRune-macos.zip` SHA-256 is `31a5db67b2e1a7555206cbc6c5fd62e39bdd8d20b688e4a97501282ddbe8bdea`; the enclosing GitHub artifact digest is `sha256:d0de27d6ad1d9dc36fe603097ca0917c77d14ba01fc1f8176902233a348ee44f`. This CI bundle was not substituted for the separately tested older local GUI artifact.

## 6. CI and documentation review

The root README points to the development app and detailed plan. The [Rune README](../rune/README.md) accurately separates development builds from release work, lists platform prerequisites, and provides commands from the `rune/` working directory. Native/provider details and the [update publishing instructions](../rune/scripts/UPDATES.md) are linked. Build and helper paths referenced here exist in the inspected checkout.

The workflow uses macOS and Windows hosted runners, a pinned Rust toolchain, locked dependency checks, formatting, Clippy, workspace tests, a headless sample, platform development packages, and uploaded diagnostics/artifacts. The Windows job additionally creates an NSIS installer and runs the installed executable's capabilities/headless/uninstall smoke path.

Remaining CI and release gates:

1. The workflow now explicitly runs standalone icon-builder formatting/Clippy/tests, Python notice-generator tests, JavaScript extraction fixtures, local gateway tests and the Mac legacy host-framing tests. These are outside the main Cargo test count; successful remote execution still needs to be recorded.
2. A separate optimized-package matrix now runs alongside debug checks. At `dae7880`, Mac inspection passed the bundle payload, signatures, version/capabilities and system library dependencies. Windows ordinary release smoke passed an owned native sample process and bounded window responsiveness instead of unavailable release tooling, plus installation/runtime/uninstall checks. Its subsequent temporary identity-certificate trust failure still needs a repaired rerun. A protected production release job still needs production signing, notarization, provenance, and publishing from reviewed commits/tags.
3. The Windows smoke is useful but noninteractive. It does not establish tray/focus/UI Automation/capture/audio/credential behavior under a logged-in standard user or test installation over a running GUI app.
4. A latest hosted runner does not establish macOS 14 minimum-version support, the supported Windows 11 release range, multiple GPUs, mixed-DPI displays, international keyboards, or screen-reader support.
5. The complete `scripts/check.sh` passed locally with seven scenes and both native Vision/JPEG/geometry and invocation transaction fixture suites. Preserve this as Mac-only evidence until native CI and device runs are recorded.
6. Remote CI outcomes and the exact source commit/artifact hashes must be recorded after execution. The presence of a workflow file is not a passed workflow.

## 7. Remaining packaging and operational gates

### Install, update, and uninstall

The Mac package script creates an app/ZIP, embeds the browser host/extension/notices/icon, supports an explicit signing identity and verifies the resulting signature. Its default ad hoc signing is appropriate only for development. Public distribution requires Developer ID application signing, hardened runtime review, notarization, stapling and Gatekeeper verification on a clean Mac. Verify embedded host signing and Swift/system runtime resolution outside the development toolchain.

The Windows script creates a per-user package and optional NSIS installer, embeds the icon and notices, and supports explicit Authenticode signing. Public distribution requires clean Windows 11 MSVC/runtime validation, publisher verification, install/upgrade/reinstall behavior and recorded uninstall results.

The [identity packaging scripts](../rune/packaging/windows/MSIX.md) provide both full MSIX and a preferred NSIS plus signed external-identity package. The latter keeps a stable executable path and lets the uninstaller disconnect owned browser hosts and unregister the exact publisher/family/external path. Production builders require an existing trusted certificate and never create trust. A separate hosted-runner-only test creates a short-lived test certificate, exercises installation and fixed-fixture OCR, and removes its exact certificate/private key and package afterward. That test is configured, not yet proven executed successfully.

The Windows builder scopes static CRT flags to an explicit application target, separately from host procedural macros, and audits ordinary and delayed PE imports before packaging. No VCRedist is bundled. Eight portable verifier fixtures pass. At `dae7880`, the optimized CI package passed the two-executable PE audit, installed native-window loading and ordinary NSIS lifecycle smoke. Production signing, installed identity/OCR, real browser continuity, upgrade/data migration and the Windows 11 interactive matrix remain open.

Two installer lifecycle issues found in the initial inspection have now been addressed in source, with Windows runtime validation pending:

- The installer now uses a read-only running-instance probe, refuses running-app install/upgrade/uninstall, checks deletion failures and removes the installation registration after owned-file cleanup. Test both interactive and silent uninstall while the app is running, including damaged/missing executable and denied-delete cases.
- Uninstall now invokes explicit exact-application browser cleanup before removing executables and fails closed on cleanup failure. Verify copied native-host/registration removal in Chrome and Edge, retain unrelated/legacy installations, and test update refresh of connected hosts on Windows.

The installer retains user history and credentials by design. Document how users explicitly remove those separately, and test reinstall without silently expanding previous consent.

### Signed updates and feed ownership

The client verifies a domain-separated Ed25519 manifest signature before trusting payload URLs, validates strict metadata/target/version/expiry, and checks download size/SHA-256 before publishing a cached artifact. Downloaded installers are not executed automatically. An unconfigured build fails closed; strict offline blocks update requests.

Release ownership still needs:

1. An owned HTTPS manifest/artifact service and declared channel policy. Current transport rejects redirects; the chosen hosting URLs must serve bytes directly rather than rely on a redirecting release URL.
2. An externally managed production signing key, backup/recovery and rotation policy. Compile the public key, feed URL and channel into the release; no production secret or endpoint is supplied here.
3. A protected publication pipeline that signs the final platform artifacts first, then computes hashes and signs the manifest, with expiration/availability monitoring and release provenance.
4. Actual tests of valid updates, wrong signatures/target/hash/size, expired feed, outage, interrupted download, replacement of a running app, failed install, rollback and older-schema data handling.
5. User instructions for the current manual quit-and-install flow, with an explicit recovery/support path. A verified download alone is not a completed application update.

### Licenses and redistributed components

The [notice process](../rune/packaging/THIRD-PARTY-NOTICES.md) now produces strict target inventories with full notices. Missing crate archive texts are supplied from recorded upstream revisions; where upstream only declares or links a license, the record preserves that declaration and separately identifies verbatim pinned SPDX references. No copyright holders or grants have been invented.

The release owner must still approve the original app's distribution terms, upstream caveats and non-Cargo components: NSIS/runtime redistribution, any additionally bundled binaries/models, fonts, and actual feature-specific assets. Zero inventory gaps is evidence of complete recorded declarations/texts, not legal clearance. Generate and archive notices from the exact release lockfile/features and package them with the final artifacts.

## 8. Native acceptance record required before shipping

Create one record per tested OS version, architecture, GPU/display setup and source application. Attach source commit, artifact SHA-256, signature details, permissions, steps and outcome. At minimum:

- **Core interaction:** supported native editor, Chrome and Edge; select before invocation; chooser; streamed answer; follow-up focus; text selection/copy; cancel/close/reopen/quit; repeated launch; unsupported/protected sources with safe fallback.
- **Privacy and capture:** denied/revoked permissions, secure fields, exclusions and pause during in-flight work, changed source/window, stale context, explicit image disclosure, destination changes, strict offline and real credential isolation.
- **Context:** source-window OCR, loaded article/captions and unavailable captions, browser registration/update/disconnect, source-app audio scope/transcriber availability, cancellation and no unintended microphone/system capture.
- **Desktop behavior:** mixed-DPI/negative-origin monitors, display removal, full-screen/Spaces, sleep/wake, keyboard layouts, tray/menu, keyboard-only and VoiceOver/Narrator.
- **Durability:** saved answers/import/restart/corruption/capacity, denied writes, crash during persistence, reinstall and downgrade/rollback with retained data.
- **Distribution:** fresh standard-user machine without build tools, install/launch, signature verification, update/failure/rollback, running-app uninstall, host cleanup, reinstall, network outage and graphics initialization failure.
- **Performance:** optimized startup, chooser latency, first token/final answer, idle CPU/GPU, repeated-session memory, resize/history behavior and a documented soak duration.

Proposed initial support remains macOS 14+ on Apple Silicon and tested Windows 11 x64 releases. Intel Mac, Windows ARM64 and Windows 10 are not supported by inference from source compatibility alone. Publish the exact tested matrix and any feature differences before release.

The user declined setting up a Windows machine or VM for interactive testing. No such environment was provisioned. Continue the existing GitHub runner checks and report the Windows 11 interactive matrix as unverified rather than treating CI as its replacement.

## 9. Next sequence

1. Finish full local checks and obtain both native CI results; preserve package artifacts and logs against the exact commit.
2. Execute the updated installer/host lifecycle tests and independent helper/extraction CI steps; review failures against actual installed behavior.
3. Run the two-platform native feasibility matrix with permissions explicitly granted or denied by the tester. Record failures rather than replacing device evidence with capability flags.
4. Validate the integrated reading/appearance/window/invocation preferences on native devices, and explicitly document any release-scope differences before claiming full migration parity.
5. Validate real configured AI, vision, OCR/browser/audio workflows and persistence/migration with controlled fixtures and supported models.
6. Provision release signing and owned update hosting; run clean-machine install/update/rollback/uninstall and accessibility/performance acceptance.
7. Reconcile this snapshot with recorded evidence and the plan's release checklist. Only then decide that the declared macOS and Windows release is ready to publish.
