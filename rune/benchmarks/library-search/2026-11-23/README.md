# Synthetic saved-answer search — 23 November 2026

This is a development CPU measurement, using 1,000 synthetic History entries and 200 synthetic Saved entries. Each has a 12,000-byte passage, a 12,000-byte answer, a 1,200-byte question and fixed metadata. Their pretty-encoded archives would occupy 26,182,034 and 5,236,434 bytes (31,418,468 combined). The harness creates these entries in memory; it opens no app archive, native window, vault or provider connection. It does not change preferences or capture grants.

The measurement host was an Apple M4 Max with 64 GiB RAM, macOS 26.6.2 build 25G83, arm64, using optimized Rust builds. It is one development machine with uncontrolled system load, not a supported-device performance guarantee. Query timings use 15 samples per case; headless Runtime timings use five samples per case. Reported p95 uses the harness's nearest-rank calculation; with these small sample counts it often equals the maximum.

## Comparison scope

- `baseline-lowercase.json` measures the original per-frame lowercase/contains implementation, before native matching and the background scheduler. Desired canonical-accent, sharp-s/full-case and sigma equivalences fail independently of timing. A completed fast query is not correctness evidence.
- `native-original-order.json` measures the first native-index/background implementation, scanning fields in their original order. It is retained as an intermediate observation, not the final product performance claim.
- The final implementation sorts each row's independent field references by byte length once during index construction, then uses exactly the same native `any` predicate over their complete content. It changes neither row order nor field content, and never joins separate fields.

Both early reports identify the repository base commit and compiled storage/view hashes. Their complete dirty source snapshots were not retained, and the intermediate report lacks a native-index source hash. They must not be presented as fully attested, independently reproducible release revisions. The final report includes additional compiled-source hashes and a separate source manifest.

Mac uses Foundation's `localizedCaseInsensitiveContains`, as the original Swift app does. Windows uses current-user-locale [FindNLSStringEx](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-findnlsstringex) with linguistic case flags. Unicode matching can differ by OS and locale. These Mac measurements do not predict Windows timing or imply identical collation rules.

## Observed CPU times

History queries over 1,000 entries, p50 / p95 milliseconds:

| Query | Original lowercase | Native, original field order | Final native, shorter fields first |
|---|---:|---:|---:|
| Absent term | 25.30 / 25.75 | 180.65 / 185.90 | 181.85 / 185.68 |
| Greek sigma equivalence | 25.25 / 25.72 (missed matches) | 576.10 / 585.66 | 43.96 / 45.06 |
| Canonical reference-title equivalence | 26.14 / 26.62 (missed matches) | 272.40 / 282.26 | 8.10 / 8.56 |

Native search uses more CPU than the original lowercase implementation, especially for absent terms. Short-field ordering substantially reduces the two intermediate late-field cases above, while keeping the exact native predicate. It is not an overall speedup over the original search. Final Mac desired-equivalence counts are correct for all 1,000 / 200 entries; Windows collation remains platform-specific.

The final History/Saved index construction took 65.57 / 13.01 ms, once on the worker. The largest final pending Runtime CPU frame was 1.13 ms and completion frame 1.43 ms across the sampled cases. For the absent History query specifically, its pending frame had p95 0.28 ms, while the independent worker query took about 182–186 ms. The original absent-query UI transition took about 25.6–25.9 ms. These figures compare UI work and result latency separately: the new implementation keeps measured UI work short while the result itself can take longer.

[manifest.json](manifest.json) records immutable report hashes. [source-manifest.json](source-manifest.json) records the final measured source hashes; the final JSON has matching compiled hashes for the six implementation/harness files. All ten measured files match commit `31104f1fb4f4052b1be0902c51c5f6d5be63ae55` byte-for-byte. A later Windows native observation corrected a full-width test expectation and explanatory comment; the search flags, field ordering and scheduler did not change. Use the pinned commit to resolve measured hashes, rather than claiming the later comment bytes were measured. No timing rerun was needed for that comment/test-only correction.

## Responsiveness and retained data

Search CPU time and UI responsiveness are distinct. Production keeps one active worker and only the latest queued request per store, using immutable shared entry snapshots and a reusable native text index. Query, locale and store-generation checks reject stale completions. Empty queries browse all entries immediately; pending and failed searches have distinct UI states. Native string calls remain synchronous within bounded row/byte batches; an unusually large legal field is never truncated solely to meet a batch target.

The headless Runtime harness measures admission plus its pending CPU layout frame, then executes the exact production `SearchJob::run` separately and explicitly publishes the completion before measuring its frame. It cannot drain Rune's private AppHandle queue. It does not measure real async queue delay, native input latency, GPU submission, display refresh, accessibility or scrolling on a packaged device. The reported combined query transition includes worker time and must not be described as the time the UI thread is blocked.

The entries' owned Vec/String capacities total approximately 31,464,000 bytes. This excludes allocator overhead, native strings, encoding/JSON temporaries, retained Runtime/font caches, RSS and GPU memory. Mac's native index reports logical UTF-8/UTF-16 source sizes, not Foundation physical allocation or peak RSS. Windows reports its exact retained boxed UTF-16 payload separately. Copy-on-write archive mutations can temporarily retain an older worker snapshot; these measurements do not establish a process memory ceiling.

## Reproduction

From `rune/`, run the explicitly ignored test in an optimized build:

```sh
RUSTUP_TOOLCHAIN=stable MACOSX_DEPLOYMENT_TARGET=14.0 cargo test --release -p pickle-app storage::library_search_benchmark::release_library_search_baseline -- --ignored --exact --nocapture --test-threads=1
```

The historical test name is retained; its current body runs the production native worker, not the old lowercase implementation. Preserve the emitted JSON and its compiled source hashes. Avoid concurrent builds or sustained test workloads during a comparison. The test intentionally has no hardware-dependent pass/fail threshold; inspect correctness fields and CPU/frame scopes separately.
