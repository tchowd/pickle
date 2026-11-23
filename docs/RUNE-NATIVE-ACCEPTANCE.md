# Rune native acceptance observations

These are bounded development observations, not release approval. Keep successful execution, unavailable evidence, and failed checks distinct. The full compatibility matrix in the [plan](RUNE-CROSS-PLATFORM-PLAN.md) remains the release contract.

## Mac controlled-source session, 22 November 2026 local time

| Item | Observed evidence |
|---|---|
| Device | macOS 26.6.2, Apple Silicon (`arm64`). This does not establish macOS 14 compatibility. |
| App | Development `rune/artifacts/Pickle Rune.app`, version 0.1.0, ad hoc signature; Rune revision `67d73a5f18390686c8e9b8b3a32b3414ebd66db0`. |
| Executable SHA-256 | `58389e5d4416f6027a94d1df434df64422d6d50893bfb5afc04ac146fa7fdb18` |
| Local package ZIP SHA-256 | `6375d5f46da8eae2e75915f3b71a07112dab45d321b7d5b916203351e6084459` |
| Source provenance limitation | Built from an uncommitted local checkpoint; its diagnostic `source_commit` is null. The hashes identify the exact tested artifact. Do not relabel it as a later committed build. |
| Controlled source | The separately built [AppKit fixture](../rune/tests/native-fixtures/README.md), an ordinary standard text view with fixed middle-paragraph selection and BEFORE/AFTER surrounding text. No personal document was needed. |
| Permission grant | The user reported enabling Accessibility and Screen Recording. The running app's diagnostics preview then visibly reported `accessibility: true` and `screen_capture: true`. |
| Diagnostic context difference | Directly running the bundle executable's `--permissions` from the agent's shell still returned false for both flags. That command's process context does not prove the permissions of the separately launched GUI process. Use the observed GUI report for this session. |

### Passed observations

1. The controlled fixture's native accessibility state exposed only the middle paragraph as selected text, including `Café` and `—`.
2. Following the user's keyboard interaction, the Rune reader displayed **Pickle Capture Fixture** as source and the complete selected paragraph. Neither BEFORE nor AFTER surrounding paragraph became the selected source. This is real cross-process selection evidence, beyond a pasted or headless sample.
3. The reader displayed a completed **Explain more** result in **Local · configured server** mode while retaining that selected source. The user initiated this reading interaction; the agent did not submit a cloud request. The configured server's actual inference location was not independently established.
4. Earlier native checks on the development app exercised fixed sample → answer → pin → close → Finder reopen. The answer and pin state were retained. An explicit accessory-app reopen handler resolved the initially observed reopen failure.
5. The native diagnostics Save sheet opened correctly from the accessory app. A reviewed report was saved into a newly created test-only temporary folder. The resulting JSON was 671 bytes, had file mode `0600`, contained only the allowlisted build/platform/policy schema, reported both GUI permissions true, and contained no fixture passage, server address, model name, credential or exclusion data. No report was transmitted.

### OCR attempt: capture present, recognition failed

After the tester's permission grant, OCR was enabled temporarily and the user invoked the controlled source again. The reader retained an image of the correct fixture window and displayed a new context-sharing consent prompt. No Continue action was taken by the agent. The OCR text was empty. The retained image looked blurred and occupied only part of a wider white canvas; a direct screenshot of the same fixture was crisp at 1400×778 pixels. This is a failed OCR recognition observation and an image-sizing/configuration investigation, not a successful OCR test.

OCR was then restored to **off**, which cleared the session context and canceled the pending sharing action. The source capture and consent boundary worked, but screenshot scaling and recognition remain unresolved for this artifact. Production-path offscreen Vision/JPEG fixtures now pass for RGB, BGRA and white text on a dark background; image geometry fixtures preserve the right edge and expected dimensions. A candidate ScreenCaptureKit fix uses the filter's content rectangle/pixel scale and best capture resolution. The Mac was initially locked when the quit/rebuild step was attempted; after manual unlock it was replaced by the optimized bundle recorded below.

### Quality and UI findings

- The Local response asserted unsupported details about the researchers' expertise and nutrition research. The test therefore establishes transport/UI functionality, **not answer accuracy**. No claim of factual verification or model calibration follows from a completed response.
- The native Settings view showed a gray material background and clipped long notes. Pinned Rune's native material replaces the same node's fill, and ordinary text is unwrapped by default. Source fixes now separate the tint layer and wrap notes; their layout fixtures pass. A rebuilt native GUI still needs acceptance.
- The initial diagnostics preview replaced accessible JSON text with a generic label. The source now labels the container and exposes the full JSON; a semantic regression test passes. Native screen-reader verification remains open.

### Still unverified

Successful source-window OCR recognition, explicit vision, browser installation/captions, source-app audio, denied/revoked access, secure fields and exclusions, follow-up focus/copy, other native editors and browsers, mixed-DPI/multiple displays, full-screen/Spaces, oldest supported macOS, screen readers, clean-machine installation, production signing and performance/soak budgets remain separate acceptance items. A permission boolean is not proof of successful capture or OCR.

## Optimized Mac retest at `dae7880537b23e2c78a68244125c4b8182134c67`

After manual unlock, the prior GUI was quit normally and its absence verified. A fresh release bundle was built with the exact source commit supplied to build metadata, passed strict ad hoc signature verification, and passed the package inspection script (payload, installed version/capabilities and system-only loader dependencies).

- App executable SHA-256: `d40e236c57e41a06b3259c547b2d6d422de86172a38f4c20527ce01f7ab57ccc`.
- Package ZIP SHA-256: `2236a0339feb45b9b473ef56ed9464407945bf20c39256e7143b4a98f7216c6b`.
- The optimized native GUI launched. Settings now showed the intended dark tint and wrapped long notes without the earlier horizontal clipping.
- The complete diagnostics JSON was exposed in the accessibility tree, including the exact source commit. Native screen-reader usability still requires separate testing.
- That GUI reported `accessibility: false` and `screen_capture: false`, unlike the older binary. The user invoked the gesture, but no chooser appeared. This does not test the capture-sizing fix. A permission refresh was requested for this rebuilt binary.
- Temporary OCR was restored **off**, verified in the native GUI, before the permission handoff. No screenshot or passage was sent for inference during this retest.

After the user refreshed both grants, System Settings still showed Pickle Rune enabled while the current GUI reported false. Finder also initially exposed a stale item pointing at a retained `Previous.app`; navigating out of and back into `artifacts` refreshed it to the correct current bundle. Re-selecting the current app through Add alone did not resolve the mismatch. Removing only Pickle Rune's existing entry and re-adding the exact `artifacts/Pickle Rune.app` in each privacy category, followed by normal/OS-requested relaunch, made the current GUI report **both permissions true**. Other applications' entries were left unchanged. The binary was not rebuilt during this refresh.

### Invocation retest did not reach OCR

The user then reported another physical Control + Option invocation with the controlled fixture selected. The optimized app remained in Settings, with no chooser or reader error. This is an **invocation failure observation** despite verified permission flags; it does not establish repaired screenshot capture or OCR recognition. The cause remains under investigation, and the earlier debug artifact's successful capture cannot be transferred to this release artifact.

An attempted automated conventional-shortcut test was inconclusive: the UI tool delivered a control character to the text view instead of exercising the global shortcut. The controlled fixture's exact text and middle-paragraph selection were restored and visually verified. This is a tooling limitation, not proof that the product's conventional shortcut fails.

Teardown restored OCR **off**, the original modifier gesture **on**, and automatic chooser **off**, saved settings, and returned the app to its empty reader. A read of only those three stored boolean fields independently confirmed the saved values. No inference or context-sharing action was submitted during this retest.

The subsequent source review found two independent silent-success paths: invocation configuration accepted an absent native shell in live mode, and native event-monitor registration discarded nil results. Source fixes now reject explicit invocation changes when the live shell is missing and stage required observers/hotkeys transactionally, preserving prior registration on failure. Unchanged invocation settings no longer re-register observers on unrelated settings Save, so manual connection recovery stays available. Seven targeted Rust tests, strict Clippy, and a permission-free native rollback fixture pass. These fixes do not prove the cause of the observed gesture failure, and the installed `dae7880` bundle was deliberately left unchanged.

## Windows evidence boundary

The user declined setting up a Windows 11 machine or VM for interactive acceptance. No such environment was provisioned. Existing GitHub-hosted Windows checks continue; their server image does not establish the Windows 11 interactive matrix.

- At commit `86473ea128175685ea062e7f49b03da641f94110`, Windows workspace checking passed, then strict Clippy found an unused target-specific helper. Conditional compilation was corrected.
- At commit `4ecefab8814ac53eb861e895e5caa3e919dd90e9`, checking and strict Clippy passed. The app test suite then reached 83 passing tests and one failure: a test attempted to read a byte-range-locked file, which Windows correctly refused. The fixture now seeds the invariant before locking and checks the bytes after release; the read-only/no-reopen contract remains unchanged. Native rerun is required.
- At commit `0c53edc0ea0cdb06fe3c7366501a011388003ecc`, [run 45089162614](https://github.com/tchowd/pickle/actions/runs/45089162614) passed Windows checking, strict Clippy and all workspace tests (222 passed, two ignored), deterministic rendering, helper checks and the static-CRT application build. A notice checksum mismatch and subsequent notice-generation failure prevented packaging/installation. The multi-command notice step also incorrectly reported success after that earlier failed test; its shell is being corrected to stop immediately. These failures must remain visible until a repaired rerun passes.
- Installed signed package identity, real fixed-fixture Windows OCR, full-MSIX and identity lifecycle/browser continuity remain separate gates. The later optimized ordinary NSIS/runtime results are recorded below; they do not establish those identity-dependent features.

## Optimized CI package observations at `dae7880537b23e2c78a68244125c4b8182134c67`

[Run 45090494358](https://github.com/tchowd/pickle/actions/runs/45090494358) passed the complete optimized macOS package job. This is distinct from the debug Mac job, which passed workspace/helper checks but timed out in the legacy Python browser-host fixture before rendering/packaging. That fixture now uses CI's configured Python instead of `/usr/bin/python3`, retains its five-second child deadline, and propagates bounded relay errors; four local tests pass, with a remote rerun required.

On **Windows Server 2025**, image `windows-2025-vs2026` version `20261116.315.1`, the ordinary optimized package passed:

1. Strict notices: 362 packages, 254 distinct texts, zero gaps.
2. PE audit of both packaged executables: no direct or delayed VC/UCRT runtime imports.
3. NSIS installation, exact installed version/capabilities and a native sample window owned by the launched process. Evidence recorded a visible, uncloaked 560×300 window titled Pickle, with two successful bounded responsiveness probes. This is window/event-loop evidence, not pixel correctness or cross-app capture.
4. Running-app install/uninstall/browser-disconnect refusal, auxiliary-file lock and uninstall retry, ownership-aware removal and application-data/unrelated-file retention.

The job then failed in the separate signed-identity setup: SignTool rejected the temporary test certificate chain as untrusted. Installed identity and fixed-fixture Windows OCR did **not** run successfully. The overall Windows job remains failed; ordinary-installer success must not be presented as a complete identity/package pass.

The debug Windows job also passed ordinary installer rendering/lifecycle checks, unsigned full-MSIX creation/SDK structural inspection, and identity-ownership fixtures before the same certificate-trust failure. These were package inspections and pure ownership fixtures, not signed OS deployment.

The disposable hosted-runner harness has been corrected to import its exact test-only leaf into machine `TrustedPeople`, matching [Microsoft's package-signing guidance](https://learn.microsoft.com/windows/msix/package/create-certificate-package-signing). It requires an already-elevated disposable runner, never imports into Root, tracks imports before mutation, and removes/verifies only its exact certificate/private key in `finally`. Production trust checks are unchanged. Syntax/helper tests pass; actual trust/deployment and OCR require a new CI run.

| Artifact from that run | SHA-256 |
|---|---|
| Optimized Mac ZIP | `146fa14427bda02e10de9b7dc73e66bd53872172c66c46f361cd629702f4c46a` |
| Ordinary Windows optimized NSIS setup | `cd21e3df5c1292c0b793ad012b2bcefe1cbafd4f05f7d22d076bbf30b5d7b763` |
| Ordinary Windows optimized ZIP | `551d62e1a35cd2463b130419a40c5ef6a01256be5d448d4cedfaa720d35b4a2e` |

The enclosing GitHub optimized Mac artifact digest is `sha256:55c93b85d3bd426e181a69e2c08029b5d5a66a6db0851239312c9f4dbbd4b4cf`; Windows is `sha256:3d415b953af3c50e83cf1f25f650743a0628fdabe86615c41c5cb89d8b37f1f7`. All are development artifacts; no production signing/notarization or public release is established.

## Follow-up CI at `c038fae778a47a9bc9374a549e661d209ec124aa`

[Run 45091937443](https://github.com/tchowd/pickle/actions/runs/45091937443) passed both Mac jobs, including the corrected legacy-host fixture and optimized package inspection. Windows optimized ordinary installer/native-window/lifecycle checks passed again. The temporary certificate now passed SignTool validation; the identity package and application executables were signed, the NSIS identity installation succeeded, and the installed process reported package identity/OCR capability.

The fixed OCR call created an engine and completed `RecognizeAsync`/`GetResults`, but the text failed the strict `PICKLE` comparison. The error was **not** missing package identity, missing engine, an API exception or a recognition timeout; those have separate earlier failure paths. Actual recognized text was absent from this artifact's output. The next source change emits bounded, escaped diagnostics only for this owned in-memory fixed fixture, retaining the same exact matcher and nonzero failure. Do not attribute this to fonts, bitmap layout, languages or an OCR defect until that evidence is available.

For the earlier Mac CI success and artifact hashes, see the [implementation status](RUNE-IMPLEMENTATION-STATUS.md). Do not combine evidence from different commits into one claimed release pass.
