# Pickle

A native macOS menu-bar reading assistant. Highlight a passage, press and release **Control + Option** together, then choose **Simplify**, **Expand**, or **Chart**. Results appear in a movable, resizable floating panel. Pickle never edits the source app.

This is a **development build**, not a signed/notarized release. The generative model is Cloudflare Workers AI’s Llama 3.3 70B. Jev is integrated through **Cloudflare `typesafe/jev`**, separately from generation. Live Llama and Jev checks now pass. After the user funded AI Gateway, a September 18 browser review confirmed $10.00 credit and the production Swift pipeline completed a Jev-checked simplification. Cloudflare’s Completed-job envelope is now supported.

## Run

Requires macOS 14+, Swift 6 Command Line Tools, and an Apple Silicon Mac for the supplied build. No third-party Swift dependencies.

```sh
./scripts/build-app.sh
open dist/Pickle.app
```

Launching or reopening Pickle brings the reader window forward, including after it was closed. The leaf menu-bar item provides pause/resume, paste, reopen, sample, settings, clear, and quit. The bundled sample makes **no network calls** and is explicitly labeled as a fixed demonstration.

Build and run deterministic checks:

```sh
swift run PickleChecks
# Also build and run the native offline smoke check:
./scripts/test.sh
```

The [recorded test results](docs/evidence/deterministic-checks.txt) contain the latest run. The dependency-free test runner works with Command Line Tools. On the development machine, XCTest was unavailable with CLT and full Xcode required license acceptance; the project does not change that system setting. Every check prints PASS/FAIL and exits nonzero on failure. There are currently 58 checks, plus native window, persistence, and reading-action smoke checks.

## Set up selection capture

1. Open **Pickle → Settings → Request Accessibility access**.
2. In **System Settings → Privacy & Security → Accessibility**, enable Pickle yourself. If necessary, add `dist/Pickle.app` using the + button.
3. Return to Pickle Settings and refresh permission status. Reopen Pickle if macOS requires it.
4. Select text in another app, then press **Control + Option** together, then release both keys. Pickle automatically imports the highlighted text and shows a two-line preview in a compact floating bar with **Simplify**, **Expand**, and **Generate chart**. You do not need to copy or paste manually. Capture occurs before any panel receives focus; no request starts until you click an action.
5. Click an action to expand the same floater upward and show the result inline. The floater is centered 24 points above the active screen’s usable bottom edge, keeping clear of the Dock. Escape or the × button dismisses the bar. Command+1 / Command+2 / Command+3 still work inside the result panel.

Control + Option is the default shortcut. Press both modifiers together and release them to open Pickle. No letter key or double-tap is needed. Other keys, extra modifiers, clicks, and pause interrupt the gesture. Settings can restore a conventional key combination. Global shortcut monitoring and selection capture require Accessibility permission.

A development rebuild can invalidate Accessibility trust because the app is ad-hoc signed. Remove/re-add the app if permission stops working. Stable Developer ID signing is recommended before daily use.

Selection capture uses `AXSelectedText` and optional selection bounds. Optional page context captures the source window once per session using Screen Recording permission and reads visible text locally with Apple Vision. Optional browser references can additionally fetch the current public page, and the companion extension can share a loaded page or captions. Pickle does not synthesize Copy. Known secure roles/ancestors, protected-content flags, and excluded bundle IDs are blocked locally. Browser pages with an exposed URL can start a session without selected text; otherwise manual paste remains available. Accessibility cannot promise uniform coverage across apps or all custom password controls.

## Cloudflare configuration (CLI supported)

Both roles use Cloudflare credentials. No TypeSafe key or direct TypeSafe endpoint is used.

```sh
npx --yes wrangler@4.134.0 whoami
npx --yes wrangler@4.134.0 ai models list --search jev --json
# On accounts that expose the new catalog schema:
npx --yes wrangler@4.134.0 ai models schema typesafe/jev
```

In Settings, enter the Cloudflare account ID and save a personal **Workers AI API token** in Keychain. Cloudflare’s documented token setup is linked in [provider notes](docs/PROVIDERS.md). The default writing model is `@cf/meta/llama-3.3-70b-instruct-fp8-fast`.

For development, import your existing Wrangler credential without printing or writing it to a file:

```sh
python3 scripts/cloudflare-check.py --account YOUR_32_CHARACTER_ACCOUNT_ID --import-keychain
```

This calls `wrangler auth token --json`, captures stdout in process memory, and pipes the credential directly to the built app’s Keychain importer. **Wrangler OAuth credentials expire** and carry Wrangler’s account scopes; use a narrower personal Workers AI token for regular use. Reimport after refreshing CLI authentication if needed. After a rebuild, use **Authorize saved credential…** in Settings and handle the macOS Keychain prompt yourself; normal actions now fail with a clear message rather than blocking on that prompt. Never paste API tokens into source or commit them.

Before the first content request, Pickle explains the cloud recipients and asks you to continue. That disclosure is separate for Jev. Jev is **disabled by default**. Enabling it sends the selected material and, for quality checks, the candidate answer through Cloudflare to TypeSafe. Local-only mode blocks both cloud roles; this build includes an offline sample but no local AI writer.

### Jev through Cloudflare

Enable Jev in its own settings section. Model: `typesafe/jev`. The connection test sends only a tiny fixed sample and is explicitly marked billable. It does not run automatically.

The account’s runtime returned **402 / code 2021: “Insufficient balance; add money to your gateway or use BYOK.”** Fund/configure Cloudflare’s AI Gateway billing yourself before using this route. No billing changes, TypeSafe account creation, Worker deployment, or Hermes endpoint changes were made. CLI catalog returned `[]` and schema lookup returned code 6002; neither result proves the model cannot be routed, because the actual runtime reached the billing check.

A limited live smoke check is opt-in and potentially billable (two fixed requests, no private selection):

```sh
python3 scripts/cloudflare-check.py --account YOUR_ACCOUNT_ID --live
```

Do not rerun it merely to confirm the known unpaid Jev state. Read [the actual smoke result](docs/evidence/cloudflare-smoke.json).

## What is implemented

- Native menu bar, settings, global shortcut, optional nonactivating menu after mouse release, pause, exclusions, manual paste, and secure-field checks.
- Immutable source snapshots, app metadata, optional AX coordinates, pointer fallback, monitor clamping, and fixed-selection conversation.
- SwiftUI/AppKit panel with source disclosure, actions, progress, cancel, copy, retry, follow-up, context entry, translucent pickle-themed surfaces, and accessibility labels.
- Injectable Cloudflare writer and Cloudflare Jev clients; ephemeral bounded transport; no redirects, logging of content, arbitrary HTML, JavaScript, or remote assets.
- Five separate Jev evaluator banks: chart routing, context, difficulty, fidelity, and expansion support. Relevant questions are batched; dependent post-generation checks wait for the candidate.
- Explicit pass/concern/uncertain/unavailable policy, one repair maximum, full relevant recheck, honest quality labels, and generation fallback during Jev outages.
- Strict chart schemas, locally extracted bar quantities, finite/unit/evidence checks, graph ID/edge/condition validation, native rendering, and text alternatives.
- Task cancellation and stale progress/result rejection, size/timeout/conversation bounds, in-memory sessions and content-free diagnostics.
- Keychain credential storage, first-cloud disclosure, local-only network gate, scripts, deterministic checks, draft evaluation fixtures, and distribution guidance.

## Incomplete or deferred

- **Representative Jev accuracy, latency, cost, and repair effectiveness:** broader evaluation is pending human-reviewed labels; a funded live connection and one complete simplification workflow passed. Provisional policy thresholds are not calibrated on a representative human-reviewed dataset.
- **Cross-app compatibility:** Accessibility permission and selection testing in a browser and a native app are pending; see the [test matrix](docs/TESTING.md). No universal app support claim.
- **Polish validation:** fast/reverse/multiline highlighting, keyboard-only VoiceOver audit, multiple monitors, full-screen spaces, permission revocation, and long-running network stress tests need real-device coverage.
- **Distribution:** Developer ID signing, notarization, universal Intel build verification, app icon, installer, and auto-update are deferred. The current app is locally ad-hoc signed.
- **Optional features:** local AI providers, automatic persistent history and arbitrary graph layouts are not included. Explicit local bookmarks and prose streaming are supported.
- **Bar parsing:** intentionally conservative explicit currency/percentage and a small unit whitelist. Dates, unitless numbers, implicit units, conversions, locale-specific decimal formats, and illustrative quantities are not supported.

[Architecture and policies](docs/ARCHITECTURE.md) · [Provider verification](docs/PROVIDERS.md) · [Testing and evaluation](docs/TESTING.md) · [Packaging](docs/DISTRIBUTION.md)

To explicitly rerun the two live native Swift chart checks after configuration (potentially billable; Jev is not invoked):

```sh
python3 scripts/live-charts.py --live --account YOUR_ACCOUNT_ID
```

The final pipeline test produced both source-backed bars and the complete sample flow. Earlier failed attempts and subsequent passes are retained in `docs/evidence/native-live-charts.txt` rather than hidden.

For an isolated, network-free visual check of the action bar, run `./dist/Pickle.app/Contents/MacOS/Pickle --preview-actions`. It uses a bundled sample and temporary settings; quit it before launching the normal app.

### Fast writing with working diagrams

The writing-model setting controls Simplify, Expand, and follow-ups. Diagrams always use `@cf/meta/llama-3.3-70b-instruct-fp8-fast` for JSON Schema support, so the writing model can remain Llama 3.2 1B. The result shows the model that actually answered. Cloudflare rejects schema requests to 1B with HTTP 403 / code 5025; this is now distinguished from credential errors. See [live diagram routing verification](docs/evidence/diagram-model-routing.txt).

### Inline floating responses

Both Control + Option and the optional mouse-release chooser initially open at bottom center, then remember their moved position. Clicking an action keeps the same native window and expands it upward to show progress, consent/context prompts, errors, prose, charts, and follow-ups. The result scrolls inside the floater. Closing cancels pending work; reopening the session returns to the same floater. The menu-bar manual-paste window remains available.

### Jev funding and runtime verified

On September 18, the user funded AI Gateway and browser review confirmed $10.00 in credits for the configured account. A fixed-sample Jev request returned HTTP 200 using Unified Billing (0.875 seconds). Cloudflare wraps the model answer in `result: { state: "Completed", result: ... }`; Pickle now decodes this alongside the older direct and REST-wrapped formats. One live production Swift simplification returned “Checked against your selection” from `jev-1.13.0` in 2.33 seconds. Earlier 402/pending notes describe the historical unfunded state. This is a connectivity and integration check, not an accuracy benchmark. The saved Wrangler credential is still temporary.


## Reading tools and personal preferences

- **Adjust an answer:** Shorter, More detail, and Give an example send a follow-up grounded in the same passage. Adjustments participate in the existing six-turn conversation limit.
- **Explain a word:** Select a word or short phrase in the original passage or an answer, right-click, and choose **Explain selection in context**. The **Explain a word** button provides an alternative for keyboard use. Pickle uses the supplied passage, conversation, and available page context.
- **Resize:** Drag the lower-right corner of the expanded panel. Size and placement persist across launches. Compact actions stay compact; expansion uses the remembered reading size and stays within the screen.
- **Save:** The bookmark beside an answer saves that answer and its passage on this Mac. Open the header bookmark or **Saved answers…** in the menu bar to search, copy, or remove entries. Saving is explicit; Control + Option still clears the working session. The local library holds up to 200 answers and is separate from cloud credentials.
- **Appearance:** Settings → Appearance controls reading text size (14–24 pt), background opacity, and pickle-theme intensity. Changes preview immediately without cancelling a reading request.
- **Streaming:** Prose arrives incrementally using Cloudflare's SSE responses. Settings → Reading → Show answers as they arrive can disable streaming for models without support. Charts stay buffered until validated; drafts are temporary and may be revised by answer checks. Interrupted or oversized streams fail instead of becoming completed answers. See [Cloudflare's streaming API description](https://blog.cloudflare.com/workers-ai-streaming/).

Streaming was verified with deterministic SSE fixtures, including Unicode, truncated/malformed events, output bounds, and stale callback cancellation. No live billable provider request was used for this feature validation.

## Page context

Enable **Settings → Privacy → Page context**, then use **Allow screen access** to grant macOS Screen Recording permission. Invoke Pickle again from the source app. Each new selection session takes one source-window screenshot; retries and follow-ups reuse it. Manual paste and samples do not capture a window. Failure or a five-second timeout falls back to the selected passage.

Apple Vision reads the screenshot locally. Ordinary reading requests include at most 6,000 UTF-8 bytes of extracted page text after the page-context disclosure; they do not upload the image. The context disclosure lets you preview or remove the capture. Screenshot pixels are capped at 2,000 on the longest edge for OCR, then reduced to at most 1,440 and compressed under 750 KB for retention. Screenshots, extracted text, and visual summaries stay in the working session and are cleared for a new selection; bookmarks do not save them.

**Analyze visuals with Cloudflare** explicitly uploads the screenshot and selection once to the vision model, then reuses a bounded text summary. It may incur a charge and requires the [Cloudflare vision model](https://developers.cloudflare.com/workers-ai/models/llama-3.2-11b-vision-instruct/) to be enabled in your account. OCR may misread text and visual summaries may misinterpret images; neither supplies numeric evidence for generated charts. Excluded apps and rejected selections are not captured.

Validation uses synthetic local OCR, mock provider payloads, context bounds, chart-evidence isolation, and session cleanup. Real-window capture permissions and live vision inference still need device/account validation.

## Browser references and videos

Enable **Settings → Privacy → Browser and video context → Include webpage references**. Control + Option resolves a supported browser’s exposed URL and fetches a bounded article reference once per session. Page-only sessions work without a text selection. References are reused for follow-ups, previewable/removable, and included only after the reference-sharing disclosure. Late context never regenerates an answer. Public fetching is disabled in offline mode.

For loaded pages and captions, use **Connect browser extension** in the same settings section. The packaged app includes the extension folder and setup controls. See [browser setup and limitations](browser-extension/README.md). Available actions are **Use this page**, **Explain this moment**, and **Summarize video**. YouTube currently needs its transcript opened; generic players need loaded caption tracks. Long transcripts are explicitly excerpts, not full-video coverage.

**Listen for 30 seconds** provides an explicit, local audio-transcription fallback when a source window is known. It needs macOS permissions and an available on-device speech recognizer. It records source-app audio from the moment you start, including other playing tabs in that app, and writes no audio file.

Verification includes 58 core checks, caption/extension fixtures, native-host framing, isolated HTML parsing, source cleanup and late-result rejection, and a real authenticated loopback bridge test. A public HTTPS fetch of example.com and article extraction passed. Live browser-extension installation, site compatibility, caption extraction on real videos, and microphone-free source-audio capture remain unverified on-device. No private page, real audio, or live AI request was used for validation.
