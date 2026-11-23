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

OCR was then restored to **off**, which cleared the session context and canceled the pending sharing action. The source capture and consent boundary worked, but screenshot scaling and recognition remain unresolved for this artifact. Compare a controlled offscreen Vision fixture with the production processing path before changing capture configuration, then rerun the native test after rebuilding.

### Quality and UI findings

- The Local response asserted unsupported details about the researchers' expertise and nutrition research. The test therefore establishes transport/UI functionality, **not answer accuracy**. No claim of factual verification or model calibration follows from a completed response.
- The native Settings view showed a gray material background and clipped long notes. Pinned Rune's native material replaces the same node's fill, and ordinary text is unwrapped by default. Source fixes now separate the tint layer and wrap notes; their layout fixtures pass. A rebuilt native GUI still needs acceptance.
- The initial diagnostics preview replaced accessible JSON text with a generic label. The source now labels the container and exposes the full JSON; a semantic regression test passes. Native screen-reader verification remains open.

### Still unverified

Successful source-window OCR recognition, explicit vision, browser installation/captions, source-app audio, denied/revoked access, secure fields and exclusions, follow-up focus/copy, other native editors and browsers, mixed-DPI/multiple displays, full-screen/Spaces, oldest supported macOS, screen readers, clean-machine installation, production signing and performance/soak budgets remain separate acceptance items. A permission boolean is not proof of successful capture or OCR.

## Windows evidence boundary

The user declined setting up a Windows 11 machine or VM for interactive acceptance. No such environment was provisioned. Existing GitHub-hosted Windows checks continue; their server image does not establish the Windows 11 interactive matrix.

- At commit `86473ea128175685ea062e7f49b03da641f94110`, Windows workspace checking passed, then strict Clippy found an unused target-specific helper. Conditional compilation was corrected.
- At commit `4ecefab8814ac53eb861e895e5caa3e919dd90e9`, checking and strict Clippy passed. The app test suite then reached 83 passing tests and one failure: a test attempted to read a byte-range-locked file, which Windows correctly refused. The fixture now seeds the invariant before locking and checks the bytes after release; the read-only/no-reopen contract remains unchanged. Native rerun is required.
- Installed package identity, real fixed-fixture Windows OCR, static runtime audit, NSIS/full-MSIX packaging, repair/upgrade/uninstall and browser-host continuity are implemented or configured for CI but must be recorded only after those exact steps execute successfully.

For the earlier Mac CI success and artifact hashes, see the [implementation status](RUNE-IMPLEMENTATION-STATUS.md). Do not combine evidence from different commits into one claimed release pass.
