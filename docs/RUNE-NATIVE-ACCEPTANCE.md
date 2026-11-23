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

An independent review then caught a recovery gap: a retained menu-only shell could not retry the same configuration through unchanged settings Save. A transient registration state and explicit **Retry shortcut** control now recover the saved configuration after an initial registration error, without persisting drafts or making provider requests. Successful active registration clears retry; failed replacement preserves a prior healthy registration; paused/pending states gate retry. The new state/UI tests pass, an independent follow-up review found the gap addressed, and the complete local check now passes 239 Rust tests plus its helper/native/render checks. Context text previews also wrap within the reader width.

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

## Fixed OCR diagnostics at `334c6140a468e7b1d7066af616bcd25fdc7478b1`

[Run 45093518479](https://github.com/tchowd/pickle/actions/runs/45093518479) passed both Mac jobs. Both Windows profiles again failed the fixed OCR check. The optimized probe reported `recognizer_language: en-US`, `recognized_text: ""`, a 768×180 image, Segoe UI at 96 pixels, and 5,406 dark CPU pixels. This rules out a mere spelling/case mismatch; it does not establish why recognition returned no text.

The prepared follow-up retains the original exact baseline check and nonzero failure, while collecting bounded failure-only comparisons: original/premultiplied/Gray8/explicit-DPI SoftwareBitmap metadata and round-trip pixels, plus a 48-pixel fixed-word sample. An explicit test-harness environment variable requests create-new PNGs of only these generated fixtures. Source/window OCR never reads that variable or writes these diagnostics. The total verification deadline is 16 seconds; normal source recognition retains its existing deadline. Microsoft accepts SoftwareBitmap input, and its official example uses premultiplied BGRA; no violation of an `Ignore`-alpha contract has been established, so production normalization has not been changed merely on that hypothesis. See [the official sample](https://github.com/microsoft/Windows-universal-samples/blob/main/Samples/OCR/cs/OcrFileImage.xaml.cs) and [the deep-copy API contract](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.imaging.softwarebitmap.createcopyfrombuffer).

## Controlled comparison at `4a99cdf5c0701352d7a51fb9c52fdce05ba453f6`

The optimized Windows job in [run 45095620488](https://github.com/tchowd/pickle/actions/runs/45095620488) retained the expected overall failure while collecting the comparison evidence. Original BGRA/Ignore, BGRA/Premultiplied, Gray8 and explicit-96-DPI variants all returned empty text from the 96-pixel fixture. Each round-tripped to the CPU pixels exactly, with correct dimensions, stride and DPI. The same canvas, literal word, font family and recognizer produced exact **PICKLE** at 48 pixels (1,268 dark pixels). Both owned PNGs were inspected. This isolates sensitivity to the synthetic fixture's text size in this environment; it does not establish a universal font-size limit or complete page-OCR coverage.

The smoke baseline is therefore being changed to 48 pixels, preserving the 768×180 canvas, production BGRA/Ignore path and exact case-insensitive word matcher. The larger case remains diagnostic only. No comparison result can substitute for the baseline result. The corrected baseline and remaining signed identity/lifecycle steps still require a successful native CI rerun. Large text and real source-window capture remain separate acceptance cases.

## Local registration verification at `4a99cdf5c0701352d7a51fb9c52fdce05ba453f6`

The old app was quit normally and its absence verified. The new optimized bundle built successfully, passed signature/payload/version/capability/system-library inspection, and launched through a refreshed Finder entry for the exact current path.

- Executable SHA-256: `9241ee98be0eccd1998ae306ed3e56f32e2fa74ebd8d0884e5a448b975903974`.
- ZIP SHA-256: `8383a18a69881248cfa412dd8130a7bb7f6bd5add2d0b7a04a93be69d91817d8`.
- The GUI diagnostics exposed the exact commit. The existing Pickle-only privacy entries were refreshed for this rebuilt binary, and the relaunched GUI reported Accessibility and screen capture **true**.
- Turning off the modifier gesture successfully registered the conventional shortcut through the new strict path; turning it back on successfully staged its required native observers. No retry/error state appeared. This verifies accepted native configuration and rules out the earlier silent absent-shell/nil-observer paths for these calls; it does not establish actual keyboard event delivery.
- Saved state was independently checked as OCR off, original modifier gesture on, automatic chooser off. No inference or capture action was submitted during this registration check.
- A separate read-only `IsSecureEventInputEnabled()` probe returned false before the new launch. That is not historical evidence about the earlier failed gesture.

Apple's current [NSEvent contract](https://developer.apple.com/documentation/appkit/nsevent/addglobalmonitorforevents(matching:handler:)) requires Accessibility trust for key monitoring. [Apple DTS, May 2026](https://developer.apple.com/forums/thread/828052?page=1), states that Accessibility grants event listening as well as posting; no evidence justified requesting a separate Input Monitoring grant. Neither accepted registration nor these permissions prove delivery of a physical gesture.

### Physical conventional shortcut observation

With the same installed `4a99cdf` optimized bundle, the modifier gesture was temporarily disabled and OCR enabled. The user physically pressed Control + Option + P. Pickle opened its chooser with selected text and nonempty source-window OCR. The recorded source was **ChatGPT**, rather than the intended controlled native fixture. This establishes actual conventional hotkey delivery, Accessibility selection and nonempty local OCR for that source; it does **not** pass the controlled Café fixture or the modifier-only gesture. OCR contained recognition errors, so nonempty output is not an accuracy claim.

No reading, visual-analysis or context-sharing action was submitted. The unintended OCR context and selected passage were cleared through the native UI without exporting their contents. Teardown restored OCR off, modifier gesture on and automatic chooser off; Save settings succeeded and the empty reader was visible. An independent read of only those three stored booleans confirmed their saved values. A subsequent controlled-fixture retest is separate evidence and must record its own teardown.

### Reported modifier chooser; capture not independently observed

The next prompt explicitly asked the tester to use Cmd + Tab to the actual native fixture before pressing/releasing Control + Option without P. The user reported **“Invoked; chooser appeared.”** On inspection, Rune's accessibility tree and screenshot still showed its Settings page, with no selected passage or session-context preview. A read-only process inventory showed only the current Rune app and fixture, rather than another running Pickle executable. This discrepancy does not establish whether a chooser appeared transiently, which source it represented, or whether the modifier callback/capture completed. No controlled OCR result was available to verify; do not count this as a fixture pass or claim a proven cause.

Temporary OCR was turned off through the native UI and saved. The original modifier gesture remained on and automatic chooser off; an independent read of those three stored booleans confirmed the values. Returning to the reader and obtaining a fresh app binding showed the empty passage input. No inference, visual analysis, screenshot export or context-sharing action was submitted. Further investigation should gather evidence of native event delivery/state transitions rather than repeatedly refreshing already granted permissions.

For the earlier Mac CI success and artifact hashes, see the [implementation status](RUNE-IMPLEMENTATION-STATUS.md). Do not combine evidence from different commits into one claimed release pass.

## Complete hosted CI at `88f483e4a7402b941d90525f3f8601f3a754e3b2`

[Run 45097151814](https://github.com/tchowd/pickle/actions/runs/45097151814) completed successfully in all four jobs: Mac workspace/debug, Windows workspace/debug, optimized Mac package, and optimized Windows package. The source changes from `4a99cdf` adjust only the owned Windows OCR fixture and related documentation; the locally installed Mac remains the separately identified `4a99cdf` artifact.

Both Windows profiles passed ordinary NSIS installation/runtime/lifecycle, followed by test-signed external-identity installation and its complete runtime/lifecycle checks. The debug job additionally passed unsigned full-MSIX SDK inspection and pure identity ownership fixtures. The optimized evidence specifically records:

- The production BGRA/Ignore recognition path returned exact **PICKLE**, with `matched: true` and `passed: true`, using Segoe UI at 48 pixels on a 768×180 canvas. The `en-US` recognizer consumed pixels that round-tripped exactly; 1,268 dark pixels, 3,072-byte stride and 96-DPI metadata were recorded. This is real Windows OCR of a generated test image, not real source-window capture or a universal OCR-accuracy guarantee.
- Ordinary and identity-installed sample processes each owned a visible, uncloaked 560×300 Pickle window at the exact installed executable path. Two bounded native responsiveness probes passed for each: 2/0 ms ordinary and 3/0 ms identity. Release tooling stayed disabled. This is window/event-loop evidence, not interactive focus, pixel correctness or performance-budget acceptance.
- Installed version `0.1.0`, runtime package identity/capabilities, same-version repair, conflicting-directory refusal, same-path identity migration refusal before copying, running-instance install/uninstall/browser-cleanup guards, retryable locked-file uninstall, exact owned identity/browser removal, unrelated-file preservation and application-data retention passed. No real Chrome/Edge session or cross-version migration was exercised.
- Both packaged PE images passed the direct/delayed VC/UCRT import audit. Strict notices had zero inventory gaps. The hosted harness verified removal of its exact test package, certificate and private key. Test-only signing remains distinct from production publisher trust.

| Ordinary optimized artifact from this run | SHA-256 |
|---|---|
| Mac ZIP | `11dae503c4933ede124371c5841a2aac37b1bfb1e5010d081efa1c54854e2784` |
| Windows NSIS setup | `4128e138b792b854ee0ea2bea880adbfd01c110da64fe46346d2dbf858eefd45` |
| Windows ZIP | `c09d007553af6ca7f7c4a6037d2cab5f827177d3c305440da1f5cb9306319140` |

The enclosing optimized GitHub artifact digests are Mac `sha256:b0ee8d5a76a0907c3307c6faceb70d6aabd106b1dc24db8b450fe85e08b197c9` and Windows `sha256:0b767f4da98ba023a45c3416ead4317088967d529b37513303ec6b59513d0859`. The Windows ordinary ZIP contains no test identity payload. Clearly named identity-CI artifacts use a disposable publisher whose trust was removed; they are test evidence, not distributable signed releases.

These jobs ran on hosted development environments, including Windows Server 2025. The user declined a Windows 11 machine/VM; its interactive matrix remains unverified. Production signing/notarization, oldest supported OS versions, clean standard-user installation, browser/audio/cross-app capture, screen readers and owned update hosting remain release gates.

## Release-tool CI at `7d5ca736e709577032236a9ebe0ab168aafb85a7`

[Run 45100714970](https://github.com/tchowd/pickle/actions/runs/45100714970) passed both Mac jobs and Windows debug, including the new portable release helpers and offline candidate-verifier tests. Windows debug retained its established signed installation/OCR/lifecycle path. The optimized Windows job failed in the new isolated finalizer before signing: MakeAppx unpack attempted semantic validation of the sparse identity and required the intentionally external `PickleRune.exe` inside that package. Unsigned preparation, its receipt and static application checks had already succeeded. No successful CMS/timestamp/finalized-installer result follows from this failed job.

The extraction command now uses MakeAppx's documented `/nv` option for sparse packages, matching preparation. Explicit PE/MSIX identity, version, runtime/capability, file-inventory, publisher, signature and timestamp checks remain in place. The corrected finalizer still needs the optimized Windows rerun. See [Microsoft's MakeAppx options](https://learn.microsoft.com/en-us/windows/msix/package/create-app-package-with-makeappx-tool).

## Mac window-drag investigation

The user reported that the actual UI could not be moved around the screen. The Pickle header already had Rune's `.window_drag()` marker. Source inspection found that pinned Rune sends that drag through winit's retained original `NSWindow`, while the content is displayed in an adopted nonactivating panel. Rune's source-window bridge forwards fifteen native selectors but omits `performWindowDragWithEvent:`, so the request targets the hidden backing window.

An app-owned adapter supplies only that missing selector on winit's declared source class. It maps the exact visible panel/source pair weakly, forwards only an event belonging to that panel, preserves the original implementation for unrelated/mismatched/expired pairs, and refuses a foreign override. It does not alter global `NSWindow`, subclass KVO instances, add input monitoring, move by guessed deltas, or change Windows behavior. Native synthetic routing/lifetime/fallback fixtures and a full-header button/text hit-testing regression pass. Headless/isolated startup explicitly skips installation. The following rebuilt-app test provides the separate native movement evidence.

### Verified native movement at `ac5688f8a99ce032e21c2b8db6f5093c255f7d3c`

The old app was quit normally before rebuilding. The optimized current bundle passed strict signature, payload, version/capability and system-library inspection. The running executable's path/hash and the GUI diagnostic commit were checked against that exact artifact.

- Executable SHA-256: `bb5e1cf39febdc3827db62271a38f65ba731b1a9ba7c89164bc96d5723ff6d90`.
- Local ZIP SHA-256: `300f7905a6d2f9e51d8ec57283fb377ad922967d87204b6d278a60ada0e3c1ac`.
- Automated background dragging did not produce a measurable saved-position change, so it was not counted as a native movement pass. The user then physically dragged the blank header and explicitly confirmed **“Yes, it moves now.”**
- After that physical drag, normal quit persisted a changed window position. Reopening and quitting again without another move recorded exactly the same position, logical size and scale. This verifies movement plus save/restore on this Mac; it does not establish other displays/OS versions or Windows dragging.
- Native Pin toggled on and off successfully, and Settings opened normally. Pin was restored off. The app was left running at its empty reader, with no inference, vision, audio or context-sharing action submitted during the test.
- As with earlier ad hoc rebuilds, GUI diagnostics initially showed the existing capture grants false. Only Pickle Rune's stale Accessibility and screen-recording entries were removed/re-added for the exact current bundle, followed by normal restart. The current GUI then reported **both grants true**. Other apps and permission categories were left unchanged.
- Saved OCR off, original modifier gesture on and automatic chooser off were independently confirmed. Permission flags are not a new controlled selection/OCR pass; those acceptance cases remain separate.

### Complete CI and isolated Windows finalization at the same commit

[Run 45102216881](https://github.com/tchowd/pickle/actions/runs/45102216881) passed all four jobs. Mac workspace tests recorded **246 passed, two ignored**; Windows recorded **238 passed, two ignored**. These totals exclude standalone helper/native fixtures. The new Mac drag fixture, portable finalizer/signing fixtures, strict checks and both optimized packages passed.

The optimized Windows job now passed unsigned preparation before test-key creation, static PE/MSIX identity/runtime checks, and the complete finalizer. Its report binds source `ac5688f`, target `x86_64-pc-windows-msvc`, app version `0.1.0` and prepared-receipt SHA-256 `165b834167743096992d18655b21cd12cbe8f684e130011f6638107bb84165dd`. All five images—app, browser host, MSIX identity, generated uninstaller and final NSIS installer—passed exact disposable-signer and trusted timestamp verification. Independent MSIX CMS signer/RFC3161 checks also ran successfully. Gated output promotion succeeded; the test private key was removed before installed-app execution.

The installed identity then passed exact **PICKLE** OCR, an owned visible 560×300 native window with two successful responsiveness probes, repair/conflict/live-instance guards, ownership cleanup and retained-data checks. Exact test certificate/private-key cleanup passed. This demonstrates the protocol on the hosted Windows environment with a disposable publisher. The test is not a production CA identity, a fresh isolated production signing worker, a clean standard-user Windows 11 test, or a real cross-app capture/browser session.

| Ordinary optimized CI artifact | SHA-256 |
|---|---|
| Mac ZIP | `231a72dc668b4f2badbe942bb59f287b72c69c7027441b193629ab4805227def` |
| Windows NSIS setup | `00b2fce29276ca94a80094199468b7513fa241b86b9cda72ba9f99d17703ad20` |
| Windows ZIP | `74a96d5222e21fb9c1b8ac191335f13aadd7f974a5f6e5d47b25085abebfea1a` |

The optimized enclosing artifact digests are Mac `sha256:423ff25b2319712b4e342997b96c165d6fb526ec4f695f53bb12d5bc42c68d26` and Windows `sha256:9d5456860062a43289325749c98724601bf0505781778802a13a529e0733e870`. The clearly named test-signed finalizer installer has SHA-256 `1256a2459fbd9d2bb036386103b1db6f4fec45d8afe103b51c6abb935923a25f`; its test-signed ZIP is `6a88df968577bc2f709a4f82bb34db40a63aaf0db345ac6cc4686e3730c561e6`. Their disposable trust was removed; keep them as evidence, not public release candidates.

## Network-free sample follow-up audit on the installed `ac5688f` bundle

The idle reader was checked before opening the fixed demonstration. Simplify produced its labeled fixed answer. The follow-up field accepted a controlled question through the native text input; pressing Return left that question and state unchanged. Clicking **Ask** then produced a demonstration conversation turn, cleared the input, retained the original passage/base answer and left focus in the follow-up field. The sample provider is selected before any credential/provider construction, so this workflow made no AI/network or vault request. No Save action was submitted; the temporary sample was cleared back to the empty reader.

This exposed a concrete parity gap: the original Swift field uses `onSubmit(sendFollowUp)`, while Rune's single-line follow-up input had no submit handler. The source fix now attaches Return directly to that input and shares one handler with **Ask**. Controller validation/consent remains authoritative, busy stale submissions are refused, and other input fields keep their existing behavior. Six Runtime regressions cover Enter/Ask request equality, immutable source/result, blank input, Settings isolation, pause/pending/busy guards and long question/label layout. Long valid questions were also found to stay on one clipped line; source rendering now wraps at words and breaks an otherwise overflowing unbroken word. The source tests pass; the installed bundle still needs the updated native Return test before this is recorded as a native fix.
