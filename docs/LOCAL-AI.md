# Local and Online AI

Pickle now has persistent Local/Online selection. Local sends inference to a configured self-hosted Ollama API, which may run on another computer. It is distinct from the unchanged `localOnly` strict-offline preference. Local is the default when no mode preference exists; an explicit saved choice is preserved. Mode switching lives in Settings, and the reader shows a compact location label. The configured Ross connection is reachable and has generated a fixed-sample answer using the packaged app and its Keychain access key.

Online keeps the saved Cloudflare writing model (`@cf/meta/llama-3.2-1b-instruct` on this installation), schema-capable cloud chart model, optional cloud vision, and saved Jev preference. No cloud inference was used to test this change.

Local supports prose, follow-ups, answer adjustments, OCR text, article/transcript excerpts and NDJSON streaming. Jev and automatic semantic repair are disabled; deterministic validation remains. Charts use Ollama's JSON Schema format and existing evidence validation. The optional local vision setting checks for a downloaded model with vision capability before sending an image; no vision model was downloaded or live-tested, and the current Ross gateway intentionally permits text only.

Mode changes cancel the runner and review work, reject progress/drafts/results by request identity and policy revision, retain source/session context where possible, and never regenerate. Failure never selects another provider automatically. Users can Retry or explicitly Switch to Online. Streamed drafts remain visible through the existing Online review/refinement path.

## Bounds and network behavior

- Context: 8,192 tokens; output: up to 1,024 tokens; keep-alive: two minutes.
- Prompt budget: conservatively at most 6,656 UTF-8 bytes including system, encoded text fields and structured schema, reserving 512 tokens for templates. This uses a conservative byte upper bound rather than pretending to have the server tokenizer locally.
- Selected passage, manually supplied source context and current question are preserved in full or rejected with a shorter-passage request. Supplementary references, OCR, prior answers and recent conversation are bounded excerpts. Original session data remains intact.
- One active app runner; gateway accepts one generation and rejects excess work with Busy instead of an unbounded queue. Superseded streams are cancelled.
- Metadata requests have an eight-second deadline; generation transport has a 90-second resource deadline. No automatic retry or cross-provider fallback.
- Stream/body, output, image and request-size limits apply. Incomplete NDJSON, provider errors, malformed events and output-length termination cannot become completed answers. Redirects, HTTP cookies and response caches are disabled. Raw server errors/content are not logged.
- Local model verification rejects remote/cloud metadata before each generation. The installed gateway also pins the allowed GGUF digest and blocks all model-management routes.
- Strict offline blocks webpage fetching and inference on another computer, including loopback SSH tunnels. Same-device inference requires an explicit same-device setting plus a literal loopback/localhost endpoint. Existing offline preferences migrate unchanged.
- Audio remains on-device; unconfigured Local vision never sends images to Cloudflare. Changing modes does not alter Online consent or Jev preferences. Local consent is endpoint-specific; Local access keys are separate, endpoint-scoped Keychain entries.

## Measured results — fixed fixtures only

All measurements used the actual Swift provider/pipeline over SSH on Tailscale, with no private content. These are small integration samples, not an accuracy benchmark.

| Fixture | End-to-end | Model load | Generation | Result |
|---|---:|---:|---:|---|
| Cold simplification | 1.184 s | 0.808 s | 0.094 s | Returned prose |
| Warm passage + context | 0.558 s | 0.005 s | 0.240 s | Returned prose; misinterpreted library opening/closing hours |
| Follow-up | 0.407 s | 0.005 s | 0.121 s | Correctly identified temporary hours |
| Bar chart | 0.481 s | 0.010 s | 0.159 s | Passed existing source-evidence validation |
| Flow chart | 3.101 s | — | — | Rejected: invalid connection evidence |

Warm `/api/chat` first-token latency was 0.113–0.117 s, excluding preceding read-only model verification. Cold chat first-token latency was 0.967 s. Prose decode speed was about 262–277 tokens/s in these very short samples; this does not include prompt processing or network/model-verification overhead.

Cancellation was tested after an actual streamed token: client consumption stopped in 0.0037 s; a new request 200 ms later completed in 0.329 s. This verifies stream cancellation and rapid acceptance of new work; individual GPU instructions were not instrumented.

Ollama reported 1,715,921,223 bytes (about 1.60 GiB) loaded on GPU at 8K context. Before setup, `top` reported 47G used / 16G unused; after benchmark it reported 50G used / 13G unused. This is a shared host, so the total delta includes unrelated workloads and filesystem cache. Swap remained 0 before and after. The later `memory_pressure` “84% free” value is a pressure indicator, **not unused RAM**. The two large Qwen models stayed installed and unloaded.

Evidence: [fixture output](evidence/local-ai-benchmark.jsonl), [cancellation](evidence/local-ai-cancellation.txt). A separate packaged-app check confirmed Keychain lookup and Local generation, with saved mode still Online.

## Remaining limitations

The 1B model is fast but not consistently faithful: one prose fixture changed the meaning of library hours, and one flow chart failed validation. There is no equivalent to Jev in Local mode. Missing/invalid charts fail gracefully; no validation was weakened. Local vision, sleep/wake recovery, extended multilingual context behavior and real-browser Local workflows need further device testing.

Tailscale Serve requires an administrator enablement step. The working connection currently uses private SSH over Tailscale instead. See [deployment and optional HTTPS migration](../scripts/local-ai/README.md).
