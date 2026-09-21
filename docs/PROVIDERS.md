# Provider verification

Checked 2026-09-17 EDT (2026-09-18 UTC). All production inference in this project uses **Cloudflare**. Rodeo/Hermes was read only as an implementation reference; its source, running services, deployment, and public endpoint were not changed or reused.

## Writing model

- Model requested: `@cf/meta/llama-3.3-70b-instruct-fp8-fast`.
- `POST https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run/@cf/meta/llama-3.3-70b-instruct-fp8-fast`.
- Bearer Cloudflare token; JSON `messages`, `max_tokens`, `temperature`, `stream:false`; chart requests use Cloudflare JSON Schema mode.
- Standard REST `success` / `result` envelope. Prose is a string in `result.response`; JSON Schema mode returns an object in that field. Both shapes are handled. The live response included a resolved `model` and usage in `prompt_tokens` / `completion_tokens`; both native and input/output token aliases are decoded.
- Cloudflare lists a 24,000-token context and $0.293 / million input tokens, $2.253 / million output tokens. Check your account for current billing/allowances.

Sources: [model page](https://developers.cloudflare.com/workers-ai/models/llama-3.3-70b-instruct-fp8-fast/) and [REST/token setup](https://developers.cloudflare.com/workers-ai/get-started/rest-api/).

## Jev via Cloudflare

- Model requested: `typesafe/jev`.
- `POST https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/run`.
- Bearer Cloudflare token. Body: `{ "model": "typesafe/jev", "input": { "state": {...}, "questions": {...} } }`.
- The app uses Noul and Choice primitives. Noul answers have `type` and `noul`; Choice answers have `type`, `choice`, `confidence`, and `probabilities`. The response also reports `model`, `answers`, and `usage`.
- The client accepts the documented model payload and the standard Cloudflare REST envelope, then checks required question keys, answer types, finite probability ranges, full choice distributions and selected-option consistency.
- Cloudflare lists a 32,000-token context. Its Jev page points to the signed-in dashboard for price. **Account-specific Jev pricing was not accessible via the CLI catalog and is not assumed from TypeSafe direct pricing.**
- Jev is a third-party model: disclose Cloudflare routing and TypeSafe evaluation. A native app does not make these calls local.

Source: [Cloudflare Jev reference](https://developers.cloudflare.com/ai/models/typesafe/jev/) ([Markdown](https://developers.cloudflare.com/ai/models/typesafe/jev/index.md)).

### Actual CLI/runtime results

Wrangler 4.134.0 authenticated to the existing account with AI access. `ai models list --search jev --json` returned an empty list; `ai models schema typesafe/jev` returned model schema not found (6002). A runtime request to the documented endpoint returned **402**, code **2021**, **“Insufficient balance; add money to your gateway or use BYOK.”** This is a live billing failure, not a successful evaluation. The user explicitly chose to leave live Jev testing pending. No gateway funding or BYOK changes were attempted.

One Llama baseline request returned 200 with an explanation preserving uncertainty. Its resolved model was `@cf/meta/llama-3.3-70b-instruct-sd`. See [recorded evidence](evidence/cloudflare-smoke.json). Native app Simplify, Expand and follow-up also passed. The native Swift chart pipeline produced valid bar and full three-step flow results after structured-response/schema fixes; see [chart evidence](evidence/native-live-charts.txt). An earlier flow was incomplete despite having literal evidence; a narrow explicit-operation-list completeness check now catches that case.

## TypeSafe reference contract and limitations

Official TypeSafe docs were read to understand Jev semantics and all five evaluators. The **direct** API is `POST /v1/systemone` with `state`, `model`, and `questions`, but this project does not call it. Direct `jev-latest` currently resolves to `jev-1.13.0`; Cloudflare owns its own route/alias. The model version reported by a response is retained with the result.

TypeSafe’s direct model page lists $0.042 / million input tokens, free outputs, 64k total request / 32k state-plus-longest-question budgets, and dynamically changing rate limits of 250,000 tokens/second and 1,200 requests/minute. **These are direct-provider terms, not verified Cloudflare billing or limits.** Early-access availability and account entitlements still apply. Vendor calibration/speed claims are not guarantees for Pickle.

References: [API](https://docs.typesafe.ai/api), [models and limits](https://docs.typesafe.ai/models), [introduction](https://docs.typesafe.ai/introduction), [Jev announcement](https://typesafe.ai/blog/introducing-system-one-models-and-jev), [documented model weaknesses](https://docs.typesafe.ai/model-jaggedness/jev-1.13).

Jev’s documented weaknesses include numeric precision, literal interpretation, indirection, adversarial state and long irrelevant context. The app keeps arithmetic/schema rules in code and never displays “verified” or “guaranteed accurate.”

JSON output shape and schema format reference: [Cloudflare JSON Mode](https://developers.cloudflare.com/workers-ai/features/json-mode/). Cloudflare warns that models can still fail to meet a schema; app validation remains mandatory.

## Diagram model routing

Verified September 18, 2026: with the same valid credential, Llama 3.2 1B prose returned HTTP 200 while JSON Schema returned HTTP 403 / code 5025 (unsupported JSON Schema). Cloudflare documents JSON Mode support for Llama 3.3 70B: https://developers.cloudflare.com/workers-ai/features/json-mode/ . Structured generation now selects the 70B model before sending; prose retains the configured writing model. Bounded transport inspects only numeric codes from at most 16 KiB of a 403 body to avoid incorrectly calling 5025 an authentication problem. No provider error text is displayed or logged. Live Swift tests with writing model set to 1B passed for both bar and flow, resolving to `@cf/meta/llama-3.3-70b-json`. Jev was not invoked.

## Jev completed-job envelope verified

After user-funded credits, `typesafe/jev` returns `success: true, result: { state: "Completed", result: { model, answers, usage }, gatewayMetadata: { keySource: "Unified" } }`. The client unwraps this envelope and still validates every typed answer. It accepts the previous direct and standard REST formats as well, and rejects incomplete jobs. Production Swift integration passed on a public rainwater fixture, with both context/difficulty and fidelity checks completed.
