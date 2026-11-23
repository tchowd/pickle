# Synthetic Local benchmark

From `rune/`, build without contacting a server:

```sh
cargo build --locked -p pickle-providers --example local-benchmark
cargo test --locked -p pickle-providers --example local-benchmark
```

Running without arguments, or with `--help`, prints usage only. Inference requires explicit `--run`:

```sh
cargo run --locked -p pickle-providers --example local-benchmark -- --run > /tmp/pickle-local-baseline.jsonl
```

Defaults are `http://127.0.0.1:11434` and the already installed `llama3.2:1b`. Optional `--address URL` and `--model NAME` use the production Local configuration validator. No credentials, application preferences, archives, private input, model download, or cloud evaluator are used. An explicitly chosen server receives the fixed corpus. Loopback does not prove where inference runs: records retain `same_device: false` and `inference_location: unverified`.

The seven independent cases are the Swift benchmark's simplify, supplementary context, follow-up, bar and flow cases, plus the exact recorded synthetic shuttle passage for Explain more and the recorded question about rain/other routes. Follow-ups use fixed prior explanations, so a bad earlier generated answer cannot contaminate later cases. They run serially through the real `Pipeline` and `OllamaProvider`, including installed-model verification, streaming where supported, and production chart validators. The first case is named `cold_simplify` for corpus parity; the tool does not unload models or guarantee a cold model cache.

Each bounded JSONL record includes the actual source, question, supplementary page context, fixed prior explanation, answer/provenance/timing/quality or a sanitized error. `pipeline_succeeded` means the pipeline returned a result. `chart_validated` covers its existing structural/source-evidence checks. **Neither asserts factual fidelity**: every record says `factual_fidelity: manual_review_required`, including unsuccessful cases. Manually compare quantities, what each quantity counts, qualifications, source scope, and clearly labeled background. Exit zero means all seven pipelines returned results; a failure exits one after the remaining cases finish. Each case has a 120-second outer deadline and cancels its request on timeout; each JSON line is capped at 128 KiB. Broken output stops the run.

Capture a baseline before changing prompts or inference options, then compare the same corpus and model after one deliberate change. Do not equate a single successful run with provider-quality acceptance.

For a rejected chart, `raw_chart_candidate` may contain the provider's final successful structured generation, capped at 16 KiB with an explicit `truncated` flag. Its `accepted` field is always false; `pipeline_succeeded` and `chart_validated` remain false. This benchmark-only observer delegates the original prompt, provider flags, events, cancellation and result unchanged. It captures no drafts or errors, resets between cases, and makes no additional request. Raw candidates are diagnostic untrusted output, never accepted chart evidence.

The text provider requests `think: false` only for models advertising the `thinking` capability, using the metadata it already verifies. Models that cannot disable it may use their lowest supported level; see [Ollama's API controls](https://docs.ollama.com/capabilities/thinking). This is not a factual-quality check. Buffered/structured replies report `first_token_seconds: null` because no individual token was observed; chat total time starts before opening the HTTP request. The outer case `seconds` also includes pipeline setup/model verification.

The [recorded development investigation](../../../benchmarks/local-reading/2026-11-23/README.md) retains fixed-input results, counterexamples, and their limits.
