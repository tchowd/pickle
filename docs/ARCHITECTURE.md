# Architecture and behavior

`PickleCore` contains provider-independent immutable models, typed evaluators, policy, validation, the async workflow, transport, and lifecycle gating. `PickleApp` provides AppKit/SwiftUI presentation, Accessibility capture, Control + Option event monitoring and optional Carbon shortcuts, settings, Keychain, and the in-memory session. `PickleChecks` is the deterministic test executable.

| Module | Responsibility |
|---|---|
| SelectionService | Read focused app’s selected text; reject excluded/known secure controls; capture optional bounds before focus changes |
| InvocationController | Recognize Control + Option alone on release or register a conventional shortcut; optional mouse-release monitoring; hide on mouse-down; stop monitors when paused |
| RequestCoordinator | Local action/consent/exclusion checks, immutable input assembly, session updates, explicit retry/context UI |
| RequestRunner | Own one active task and generation identity; drop cancelled/late results and progress |
| RequestPipeline | Select relevant evaluators, generate, validate, one repair, recheck, return honest status |
| GenerativeProvider / CloudflareProvider | Provider-independent text/structured generation contract; Cloudflare writing transport |
| DecisionClient / JevClient | Cloudflare native evaluation contract; validate typed answers before policy |
| DecisionEvaluators | Five independent testable question banks; predefined repair mapping |
| DecisionPolicy | Provisional 0.2/0.8 Noul thresholds; choice confidence separate; no inference of real-world error rate |
| ResultValidator / ResultRenderer | Strict chart structure/evidence checks; native Swift Charts and graph connections with text alternatives |
| SessionStore / SettingsStore / CredentialStore | Memory-only content, persistent nonsensitive preferences, Keychain credentials |
| ContentFreeMetrics | Memory-only call, usage, failure, repair, cancellation, latency counters; no content/titles/tokens |

## Boundaries

No selection event invokes inference. Explicit actions construct a bounded immutable request after pause, exclusions, and provider disclosure checks. Manual context can be supplemented with a single source-window screenshot and local OCR when page context and macOS Screen Recording access are enabled. The text is sent only after page-context disclosure; images upload only through the explicit visual-analysis action. The snapshot remains fixed through follow-ups, retries, and app switching. Opening an automatically detected selection does not replace the active session until an action is chosen. Automatic capture is suppressed during requests and while pinned.

Pausing or changing request/privacy settings cancels in-flight reading work; appearance-only changes do not; generation IDs reject stale callbacks. Closing the result panel also cancels. HTTP cancellation cannot revoke material already sent or prevent an already-dispatched upstream provider from completing/billing its work. Network sessions do not follow redirects, share cookies, or persist responses.

## Policy

All problem questions use high probability to mean a concern, not a pass. At or below 0.2 is clear pass; at or above 0.8 is material concern; the middle is inconclusive. Missing/malformed answers are never treated as a pass. Difficulty uses a predefined Choice and a separate confidence cutoff. These thresholds are **development policy only**, not validated calibration. Evaluate on human-reviewed examples and pin behavior/version before release.

Simplify: context + difficulty → optional user context → generation → fidelity dimensions → at most one repair → full fidelity recheck.

Expand: context + difficulty → optional user context → generation → source-support dimensions → at most one repair → full support recheck.

Follow-up: context questions scoped to source + user question → generation with bounded prior conversation → source-support questions. Prior answers are never evaluation evidence. A follow-up’s check label is displayed with that turn.

Chart: chart suitability questions → local routing. If Jev is disabled/unavailable/inconclusive, ask for a format. A clear unsupported result does not generate a chart. Numeric or graph validation is always required. No post-chart generative repair is performed; invalid structured results surface a retryable error.

A service failure in prechecks allows generation and labels semantic checks unperformed. A failure in checking/rechecking never gives a success label. Remaining concerns after a repair remain visible. General background is allowed only when separated from source-specific assertions. The source itself is not fact-checked.

## Deterministic chart contract

Bar JSON names only evidence IDs extracted locally from source. Model-authored values, labels, URLs, scripts, and unknown fields are rejected. App code resolves IDs to immutable finite numbers and units, requires two or more distinct compatible quantities, and renders from a zero baseline (including negatives). Literal source excerpts accompany all quantities. Matching units/evidence cannot prove that the model picked semantically comparable metrics; the user still needs to inspect the source.

Flow labels and edge evidence must occur verbatim in supplied material; the schema restricts edge quotes to local source spans. A narrow recognizer also requires all steps and connections in simple English lists such as “is received, validated, and saved.” Other forms still need semantic review; all IDs, references, connections, size bounds and conditions are checked. Literal matching cannot prove every edge’s semantic correctness or detect every omitted nuance. The renderer shows each connection and its evidence instead of executing arbitrary graph markup. Branches are individual accessible connections, not a spatial auto-layout engine.

## Limits

| Item | Limit |
|---|---:|
| Selection | 12,000 UTF-8 bytes |
| Additional context | 6,000 UTF-8 bytes |
| Automatic OCR / visual summary | 6,000 / 3,000 UTF-8 bytes |
| Retained screenshot JPEG | 750,000 bytes; longest edge 1,440 pixels |
| Capture and OCR wait | 5 seconds, then selection-only fallback |
| Question | 2,000 UTF-8 bytes |
| Output | 12,000 UTF-8 bytes / 1,800 requested output tokens |
| Follow-up history | 6 turns and 16,000 UTF-8 bytes |
| HTTP response body | 256,000 bytes, bounded while reading |
| HTTP request | 40 seconds each; no automatic retry |
| Automatic repairs | 1 per generated prose result |
| Concurrent reading pipelines | 1 |
| Bars / nodes / edges | 12 / 10 / 16 |
| Latency samples retained | Last 100, memory only |

Limits reject oversized selected/manual source input. Automatic OCR is truncated to its budget with a visible notice. Multi-stage user-visible latency includes actual completed work. Rate limits are surfaced for explicit retry; no automatic billable retry loop. Provider response usage is recorded when present; exact dollar cost is not inferred from direct TypeSafe pricing.


## Incremental prose and opt-in persistence

`StreamingGenerativeProvider` adds a cumulative draft callback without changing the buffered provider contract. `CloudflareProvider` sends `stream: true` only for prose; structured charts keep their existing schema validation. `BoundedHTTPTransport.stream` uses ephemeral, redirect-blocking URLSession transport with resource timeouts and a raw response byte budget. `ProseStream` bounds decoded output and requires an SSE terminator. `RequestRunner` gates both drafts and final outcomes by request identity. The streamed draft stays visible during review and the single repair, labeled Reviewing or Refining. Completion replaces it with the final answer; failed repair retains the original answer with its internal unchecked status.

Appearance settings and window frames are stored in the existing preferences domain. `BookmarkStore` persists only explicitly saved passage/answer pairs, with duplicate suppression and a 200-item limit. Library retrieval is local and does not submit requests or repopulate the working session. A decode failure preserves the stored data and blocks replacement writes.

## Session page capture

`ScreenContextService` resolves the frontmost source window before Pickle takes focus. ScreenCaptureKit captures only that window, validating window ID, PID, and bundle ID, with no whole-display fallback. Vision OCR and JPEG compression run off the main thread. `RequestCoordinator` gates delivery by session/generation and bounds the wait; removing context, disabling it, or beginning a new session cancels capture and drops retained data. Reading requests reuse OCR. The optional `VisualContextClient` uses a separate bounded request and retains only a summary alongside the compressed session image. Model-derived visual summaries are labeled and are never evaluation or chart evidence; OCR is excluded from deterministic chart evidence too.
