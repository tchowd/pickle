# Pickle Rune

The Rust/Rune rebuild of Pickle for macOS and Windows. It lives beside the original Swift app while native compatibility and release validation continue.

This is a **development build**. macOS execution has been exercised on the development Mac. Native Windows CI has passed workspace tests, rendering, ordinary installer/runtime/lifecycle checks, and complete test-signed identity/OCR/lifecycle checks in debug and optimized builds. See the [execution record](../docs/RUNE-IMPLEMENTATION-STATUS.md). Developer ID signing/notarization, Windows production signing, published update hosting, and the full compatibility matrix remain release work.

## Build and run

Requirements:

- Rust 1.99.0, with `rustfmt` and optionally Clippy. Rune is pinned to commit `67d73a5f18390686c8e9b8b3a32b3414ebd66db0` along with its required patched dependencies.
- macOS: Apple Command Line Tools and the Swift compiler. The native bridge targets macOS 14; actual oldest-OS device testing remains necessary.
- Windows: the Rust MSVC toolchain, Visual Studio C++ Build Tools, and the Windows SDK. The intended initial Windows release target is Windows 11 x64.

From this directory:

```sh
cargo run --locked -p pickle-app
```

If an installed `stable` toolchain reports the same Rust version, `cargo +stable` can select it without downloading a separately named toolchain. Use the pinned toolchain in CI/release builds.

The app opens a reader and stays running after its window closes. Use the menu-bar/system-tray menu to reopen it or quit. A second normal launch forwards Reopen to the first process. If the reader cannot initialize its graphics/window, a native error dialog explains the next steps. On macOS, a minimal helper shows that dialog independently of Rune; the parent keeps its instance lock until acknowledgment. Quit waits for background work and window preferences to finish; an unsafe shutdown deadline exits with a fixed failure code instead of releasing the instance lease while writers remain.

### macOS bundle

```sh
bash scripts/build-macos.sh debug
```

Open `artifacts/Pickle Rune.app`. The script also produces `artifacts/PickleRune-macos.zip`. The default signature is ad hoc for development; it is not sufficient for public distribution. A previous bundle is retained in the printed staging directory when rebuilding.

For an optimized build, use `release` instead of `debug`. Set `PICKLE_SIGN_IDENTITY` only when the release owner's Developer ID identity is configured. Notarization is a separate explicit release step.

After packaging, `bash scripts/test-macos-package.sh` checks payload presence, signature integrity, the installed executable's version/capabilities, and system-only dynamic library dependencies. It does not establish native GUI behavior, notarization or clean-machine acceptance.

### Windows package

In PowerShell:

```powershell
./scripts/build-windows.ps1 -Configuration release
# Requires NSIS on the build machine:
./scripts/build-windows.ps1 -Configuration release -Installer
```

The package contains `PickleRune.exe`, the Rust browser host, the extension folder, and notices. The installer is per-user and does not require running Pickle as administrator. Set `PICKLE_SIGN_CERT_THUMBPRINT` when a release signing certificate is available in the build environment. Unsigned packages remain development artifacts.

On an isolated Windows test account, run `./scripts/test-windows-package.ps1 -Configuration release` for an optimized installer, or `-Configuration debug` for a debug installer. Both check installed version/capabilities, live-instance guards and ownership-aware uninstall/data retention. Debug requires Rune snapshots; release keeps agent tooling disabled and instead requires a visible window owned by the exact installed sample process plus bounded native responsiveness probes. A missing/hung window fails the test.

Both profiles also run an owned native source window through the real shortcut, exact chooser source, consent, a fixed loopback Local reply, keyboard scroll-back and ordinary quit. This acceptance fixture requires PowerShell 7 and Python 3; it uses no AI model or download. Identity deployment keeps its Windows PowerShell/Appx host and delegates these checks to a bounded Core child. Original profile bytes are restored after the app exits and releases its lease. See the [native execution record](../docs/RUNE-NATIVE-ACCEPTANCE.md) for passed variants and open checks. CI retains detailed evidence; the hosted Windows Server runner does not replace the declared Windows 11 interactive matrix.

The Windows UI Automation reader intentionally does not claim access to elevated, password, or protected application content.

Windows page OCR requires package identity; it is disabled in the unpackaged ZIP/NSIS build. The [MSIX packaging instructions](packaging/windows/MSIX.md) provide an explicit identity/signing path and unsigned CI inspection commands. Installed OCR, runtime dependencies, browser-host continuity across package updates, and clean-machine install/upgrade/uninstall remain release gates. An unsigned MSIX artifact is for inspection and is not a distributable user release.

The preferred complete Windows installer uses NSIS with a signed external identity package, retaining a stable application path and ownership-checked uninstall cleanup. `scripts/build-identity.ps1` requires an existing trusted publisher certificate. The [runtime audit](packaging/windows/RUNTIME.md) scopes static CRT compilation to the Windows application target and checks packaged PE imports before distribution. Production builders never create certificates or modify certificate trust. The separate signed-identity CI test changes trust only within a disposable GitHub-hosted runner and removes its test certificate/private key afterward.

### Release candidates

Use the [release runbook](../docs/RUNE-RELEASE-RUNBOOK.md) to separate unsigned builds from production signing access. The [Mac finalizer](packaging/macos/RELEASING.md) notarizes and staples a copy of an already authorized Developer ID-signed bundle. The [Windows preparation/finalization path](packaging/windows/RELEASING.md) compiles before signing-key access, then verifies and signs the selected payload without executing the app. Both require explicit owner inputs and fresh output locations; neither publishes a release.

After removing signing access, each candidate can verify its signed update metadata offline with `--verify-update-manifest FILE --from-version VERSION`. It uses only the trust root/channel/target/version compiled into that binary, emits bounded metadata, and initializes no app storage or network. See [update publishing](scripts/UPDATES.md) for required public-key matching and artifact-byte checks. Production credentials, real notarization/signing evidence and device acceptance remain necessary.

## First reading session

1. Launch the app and choose **Try a sample** to test a clearly labeled fixed, network-free demonstration.
2. In **Settings**, choose Local or Online and configure the connection.
3. Save connection changes before saving their credential. Credentials go to the OS vault, never the settings file. Use **Authorize saved credential…** if a macOS rebuild needs explicit Keychain authorization.
4. Paste a passage, or highlight text in a supported application and invoke Pickle.
5. Choose Simplify, Explain more, or Visualize. The first request to a destination explains what is shared and requires explicit continuation.

On Mac, selection invocation initially uses **Control + Option**, pressed together and released. On Windows it initially uses **Ctrl + Alt + P**. Settings offers a configurable conventional shortcut, the optional Mac gesture, and an opt-in automatic chooser. On Mac, turn off the modifier gesture to use the conventional shortcut. A conflicting shortcut preserves the previous registration. Grant Pickle Rune Accessibility permission yourself if you want cross-app capture. Selection availability depends on what each source app exposes; manual paste remains available.

If initial shortcut registration fails, Settings shows **Retry shortcut** after the system condition is resolved. It retries the saved configuration without saving draft fields. Pasting stays available, and unrelated connection settings can still be saved. Development bundles use ad hoc signatures by default; a rebuilt binary may require refreshing its existing privacy grants for the exact current bundle.

Automatic chooser previews preserve your current reading session until you choose an action. **Pin** keeps the reader on top and suppresses automatic previews. Settings also exposes reading levels, text size, opacity and green-accent intensity. Reader window geometry is stored separately from connection settings and clamped to a current display when restored.

Ask a follow-up with Return, or explain a word or phrase before requesting a full answer. **Clear follow-ups (keep passage)** removes the conversation while retaining the passage, base answer and unsent question. Copy produces readable text rather than Markdown syntax.

New answers come into view as they arrive. Scroll back during an answer to read earlier text without being pulled to the end; returning to the bottom resumes following. A new source starts at the top. Opening the full passage, context preview, Settings or saved answers stops automatic following until you start another request.

### AI locations

- **Local** calls a configured Ollama server and does not silently fall back to Cloudflare. The default address is `http://127.0.0.1:11434`, with `llama3.2:1b`; install/configure your own model. A server can be on another computer, including behind a loopback SSH tunnel.
- **This server runs on this device** is an explicit assertion about where inference actually happens, not just the address spelling.
- **Online** uses your Cloudflare account/token and chosen writing model. Charts route to the schema-capable model independently. Optional Jev checks use TypeSafe through Cloudflare and have their own disclosure.
- **Strict offline** blocks Online AI, other-computer inference, public webpage fetching, and update checks/downloads. Same-device inference requires both a loopback endpoint and the explicit same-device setting.

No personal Ross/Tailscale connection or shared cloud credential is bundled. No models are downloaded automatically.

## Context tools

Source-window OCR and browser references are off by default in this rebuild. Enable the features you want in Settings. Capture failures fall back to the passage, and late context cannot regenerate an answer. Finishing pending context preserves your current Settings or saved-content view; an explicit new reading action returns to Reader.

- Source-window screenshots are retained in the session. Ordinary reading sends bounded OCR text, not image bytes.
- **Analyze visuals with Cloudflare** explicitly sends the screenshot and selection to Cloudflare and may incur charges.
- **Analyze visuals locally** uses a configured downloaded local vision model after capability verification. It never switches to cloud vision.
- OCR, references, and visual interpretations remain labeled untrusted context and cannot supply deterministic chart quantities/edges.
- **Listen for 30 seconds** captures only the selected source application's audio scope, not microphone input. Other playing tabs/process descendants in that application may be included. No audio file is written.
- macOS audio uses an available on-device Speech recognizer and explicit speech authorization.
- Windows audio requires absolute paths to a trusted `whisper-cli.exe` and downloaded model in Settings. Process-loopback capture stays in memory and passes WAV through the local engine's stdin. OS/runtime/model compatibility still needs actual device validation.

Context-sharing consent is scoped to the configured AI destination. Removing context, changing the source, pausing, or changing relevant policy cancels pending work and rejects late callbacks.

### Browser extension

Use the packaged app for the extension workflow:

1. In Settings, choose **Show extension folder**.
2. Load that folder from Chrome/Edge's extensions page using its developer controls.
3. Copy the extension's ID into Pickle, then choose **Connect Chrome** or **Connect Edge**.
4. Enable browser references and use the extension's page/video actions.

The browser-specific native host is a bundled Rust executable; users do not need Python. It binds the extension ID and browser identity, then sends bounded authenticated messages to a loopback listener. Source content is held in memory. Host installation is explicit. Actual browser installation, update, and removal behavior remains part of the native release matrix.

## Saved answers and migration

- **Save** explicitly bookmarks the current answer and passage, up to 200 items.
- The optional **knowledge base** records accepted completed answers, up to 1,000 items. It is off by default. Turning it off keeps existing records.
- Search and reading saved answers are local and do not replace the working source or submit AI requests.
- Reference records retain URL/title/type pointers, not full reference bodies, screenshots, OCR, audio, prompts, or credentials.
- Local files use a versioned schema, atomic replacement, and a cross-process write lock. Malformed or unsupported archives block replacement writes.
- Each library has a separate **Delete all** confirmation. It clears that entire store, including entries outside the current search or page, while retaining the other store and settings. Cancel keeps all entries. Unreadable archives offer **Show archive file** and **Try again**; repair or move the protected file yourself before retrying. Deletion never resets an unreadable archive.

Settings offers **Import original Pickle history** on macOS. It reads the original knowledge-base JSON and explicit bookmark data, preserves chart/reference metadata, deduplicates imports, and leaves source files untouched. It does not import credentials or privacy consent. The new app uses a separate development identity and storage location.

Settings → **Support diagnostics** lets you review the complete report before choosing a destination in the native Save dialog. The report includes bounded build/platform metadata, capability/permission booleans and general policy flags. It excludes passages, answers, history, context, credentials, endpoints, model names, file paths and exclusions. Export is local and explicit; nothing is submitted automatically.

## Tests and reproducible UI scenes

```sh
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
mkdir -p artifacts
cargo run --locked -p pickle-app -- --scene sample --snap artifacts/reader.png --tree artifacts/reader-tree.json
cargo run --locked -p pickle-app -- --scene choosing --snap artifacts/chooser.png
cargo run --locked -p pickle-app -- --scene settings --snap artifacts/settings.png
```

Twenty-two isolated scenes include `chart-bar`, `chart-flow`, `streaming`, `reviewing`, `repairing`, `consent-local`, `consent-online`, `permission-denied`, `history-empty`, `history-damaged`, `saved-quality`, `saved-quality-detail`, `saved-metadata`, `saved-metadata-detail`, `saved-delete-confirm`, and `error`. `scripts/check.sh` renders thirteen representative states. Saved scenes seed only fixed synthetic entries to show historical warnings, questions, source pointers, chart text-alternative annotations and deletion confirmation. Fixture startup never loads archives, acquires the normal instance lease, or supplies live provider/vault services; native permission, export and reveal actions are guarded. Rendering still needs a supported graphics adapter/backend. State/layout tests and local HTTP fixtures are distinct from actual OS capture, focus, permissions, and screen-reader tests.

```sh
cargo run --locked -p pickle-app -- --capabilities
cargo run --locked -p pickle-app -- --permissions
```

These report native capability/permission state without requesting access or making inference requests. Capabilities may depend on the configured local transcription engine in a running application.

On the development Mac, the shell-launched permission probe and the separately launched GUI reported different values after a grant. For native GUI acceptance, use Settings → Support diagnostics in the actual running app and verify the capture workflow. A permission boolean alone does not prove capture or OCR works. See the [native acceptance record](../docs/RUNE-NATIVE-ACCEPTANCE.md).

The original Swift checks remain available independently from the repository root with `swift run PickleChecks`.

## Updates and release ownership

Update checks are explicit. The application verifies the publisher's Ed25519 signature before interpreting release artifact URLs, then verifies artifact size and SHA-256 before publishing a download into the local cache. It does not execute an installer automatically. The user quits the application and opens the verified installer/archive.

Development builds with no compiled update trust configuration report that the feed is unavailable; they never fall back to unsigned updates. See [signed-update publishing](scripts/UPDATES.md) for build-time trust configuration, signing, expiration, and artifact preparation. No production key or release endpoint has been created by this rebuild.

Before public release, complete both native compatibility matrices, signing/notarization, clean-machine installation/update/uninstall testing, browser-host lifecycle cleanup, dependency notices, release-feed hosting, and the full [release acceptance checklist](../docs/RUNE-CROSS-PLATFORM-PLAN.md#15-release-acceptance-checklist).

## Architecture and further detail

- [Detailed rebuild plan](../docs/RUNE-CROSS-PLATFORM-PLAN.md)
- [Native platform services](crates/pickle-platform/README.md)
- [Provider, article, and browser contracts](crates/pickle-providers/README.md)
- [Third-party notice process](packaging/THIRD-PARTY-NOTICES.md)
- [Original architecture and behavior](../docs/ARCHITECTURE.md)

### Development reading checks

The opt-in [synthetic Local benchmark](crates/pickle-providers/examples/README.md) runs the real pipeline on fixed passages, without reading application data or downloading models. It separates returned results from manual factual review; [recorded cases](benchmarks/local-reading/2026-11-23/README.md) retain known small-model failures. Thinking-capable text models request the server's no-thinking/lowest-supported mode; this does not verify their answers.

To package a candidate without replacing the running development bundle, use `bash scripts/build-macos.sh release artifacts/quality-candidate`. The optional output directory holds its own notices, stage, prior bundle and ZIP; omitted output keeps the existing `artifacts` default.
