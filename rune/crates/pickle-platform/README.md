# Pickle native platform services

This crate owns operating-system integration. Rune owns application lifetime, windows,
and the main event loop. Platform callbacks must not start a second application.

## Implemented services

| Service | macOS | Windows |
|---|---|---|
| Selection | Accessibility selected text with secure-ancestor checks | UI Automation text pattern, password checks, MTA worker and source validation |
| Source identity | Bundle ID, PID, CGWindowID | Executable path, PID, HWND |
| Invocation | Control + Option release gesture or configured Carbon hotkey | Configured conventional hotkey, Control + Alt + P by default |
| Desktop menu | NSStatusItem | Win32 tray icon and popup menu |
| Credentials | Keychain, noninteractive reads by default | Credential Manager, current-user generic credential |
| Screenshot | Source-window ScreenCaptureKit | Source-window Windows Graphics Capture |
| OCR | Apple Vision, on device | Windows OCR in a package-identified installation, user-profile languages |
| Source audio | ScreenCaptureKit plus on-device Speech | WASAPI process loopback plus explicitly configured local whisper.cpp |

Windows source audio includes the source process and its child processes. Mac capture
includes audio from the selected source application. In a browser, either can include
other playing tabs. No microphone or whole-system loopback fallback is used.

## Thread and ownership rules

- Install and drop `NativeShell` on Rune's main/UI thread on macOS.
- Capture selected text before opening/focusing the reader. On macOS call
  `capture_selection` on the main thread; on Windows it dispatches UI Automation to
  a bounded MTA worker.
- Native callbacks report `PlatformEvent`; use Rune handles to update view state.
- Use `source_is_excluded` for capture, pre-request policy, and browser-context
  acceptance. Windows basename, path, qualified identity, and known browser-host
  aliases use the same matching rules. An alias has no trusted installation path,
  so a matching browser executable/path exclusion also blocks extension messages
  from that browser family. Mac bundle identities remain exact matches.
- Execute `capture_page_with_cancel` and `record_audio` from a worker while Rune's
  event loop is running. Never block the UI thread for capture or transcription.
- Cancel context operations when source, privacy policy, provider policy, or window
  lifetime changes. The application must also gate every delivered result by session.
- A source window's ID/PID is retained in the snapshot; capture does not read a new
  frontmost window after Rune opens.
- The Swift bridge owns C-string allocations. Rust copies and releases them with
  `pickle_string_free`, and native task handles release/cancel once through RAII.
- Native failure messages are bounded static descriptions rather than raw provider,
  operating-system, or process output.

## Library collation and conditional browser capture

`LibrarySearchIndex` retains separate immutable text fields and is constructed and
queried on a worker. Mac calls the original application's Foundation
`localizedCaseInsensitiveContains`; Windows calls
[`FindNLSStringEx`](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-findnlsstringex)
with linguistic case flags and the current user locale. Windows NLS equivalence
is not a promise of identical Foundation behavior. Native errors and changed
locale tokens fail the whole result, never silently select a simpler comparison.
Row/byte batching targets bound routine cancellation intervals without truncating
legal long fields or queries; one native comparison remains synchronous.

`capture_browser_page` accepts an already resolved browser source and ephemeral
exact document URL. It refuses metadata/URL mismatches and observes native window
and document identity immediately before and after capture. On Mac it reads only
the sole exposed top-level AXWebArea URL, correlating the public AX window frame
with one eligible CGWindowID. On Windows it reads only the sole visible Document
whose Chromium `IsWebContentRoot` custom property is true, using its ValuePattern.
Address bars, links and document descendants are excluded. Missing permissions or
Windows package identity return a text-only fallback without prompts.

These checks are observational: same-document mutation, navigation away and back,
or native identity reuse cannot be made atomic with an OS screenshot. They do not
establish extension tab identity. Windows uses a separate single-flight MTA thread
with a bounded caller wait; timed-out native work retains admission until it
finishes. Mac similarly holds async admission until the task actually finishes.
Synthetic native fixtures and actual-browser acceptance are separate evidence.

## Invocation configuration and automatic chooser

`NativeShell::configure_invocation(InvocationConfig)` applies a conventional shortcut,
the Mac modifier-only gesture preference, and optional automatic mouse-release
monitoring. Shortcut strings accept forms such as `ctrl+alt+p`, `cmd+shift+space`,
and `ctrl+shift+k`. Invalid, unmodified typing keys, and conflicting registrations
return errors. Windows does not silently substitute a modifier-only hook; that
gesture is unsupported there and must remain disabled.

Apply saved configuration immediately after installing the shell, and persist new
settings only after registration succeeds. A conflicting default shortcut does not
prevent the Windows tray/settings from starting. `set_paused` returns `Result`:
pause removes global monitors/hooks and shortcut registrations; resume re-registers
them and can fail if another application acquired the shortcut while paused. Keep
the application/services paused if native resume fails, and display the error.

Shortcut replacement stages the new registration before removing the old one. Failed
registration leaves the previous configuration active. Native registration IDs are
never reused within a shell lifetime, so a queued event from an old shortcut cannot
invoke a newly configured shortcut or a paused/modifier-only shell. On Windows, a
request timeout atomically cancels an uncommitted change; if native commit won that
race, the caller waits for the definitive reply rather than reporting a false failure.

Automatic monitoring emits `HideChooser` on another application's mouse-down and
`AutomaticInvoke` approximately 180 ms after mouse-up. New drags, configuration
changes, pause, and stale timer events cancel the pending invocation. Own-process
button clicks do not hide the chooser. No input is swallowed, replayed, or logged.
Mac modifier events flow through the same Rust recognizer tested by unit tests.

Windows low-level hook callbacks only inspect the input target and post a message;
they never run UI Automation or provider requests. Invocation callbacks run off the
hook's message thread, with at most one such callback active. Mac callbacks remain
on AppKit's main thread so capture occurs before the chooser changes focus.

The application must distinguish automatic previews from committed source sessions:
ignore empty/unsupported automatic captures, gate them while busy/paused/pinned,
recheck current policy after capture completes, and commit a pending preview only
when an action is chosen. The native layer does not decide that product policy.
Invalidate in-flight automatic captures on `HideChooser` as well: mouse-down can
arrive while Windows UI Automation is still reading the previous selection.

## Work-area geometry

`window_work_area(window_number, fallback_x, fallback_y)` returns the usable screen
rectangle for clamping the reader, excluding the menu bar, Dock, or taskbar. Call it
on Rune's UI thread using the actual reader's `WindowHandle::window_number()` and
`WindowPlacement.position` fallback coordinates. Requery after display/scale changes.

Windows queries `MonitorFromPoint`/`GetMonitorInfoW` after Rune has initialized
per-monitor DPI awareness. The result is `rcWork` in desktop physical coordinates.

On macOS, the pinned winit implementation defines physical window coordinates by
flipping AppKit global points against the primary CoreGraphics display height and
then scaling the *entire global origin* by that window's backing scale:

```text
x = round(visibleFrame.minX * windowScale)
y = round((primaryDisplayHeight - visibleFrame.maxY) * windowScale)
width  = round(visibleFrame.width  * windowScale)
height = round(visibleFrame.height * windowScale)
```

The native helper resolves the actual NSPanel by its CGWindowID/window number and
uses its screen and backing scale. Adding an unscaled monitor origin to a scaled
size would disagree with `WindowPlacement` on mixed-DPI desktops. When no real
window can be resolved, only an unambiguous containing scaled screen is accepted;
overlapping mixed-DPI point interpretations return `None` rather than guessing.
Pure conversion tests cover negative origins, Retina scaling, screens above the
primary, and invalid dimensions. Physical multi-monitor behavior still requires
device testing; these tests do not establish it.

`restore_work_area(x, y, saved_scale)` selects the current display for a saved
window origin and returns `(WorkArea, current_scale)`. On macOS it first divides the
saved global position by the recorded scale, then chooses a containing or nearest
current screen. Rescale the saved origin by `current_scale / saved_scale` before
clamping; use the returned scale to size saved logical content. On Windows the saved
origin stays in desktop physical pixels; only logical content size uses the target
monitor's effective DPI scale. Invalid scale/layout or failed native lookup returns
`None`, allowing the application to fall back to its current monitor. Call on the UI
thread and verify actual multi-monitor restore behavior on both platforms.

## Credentials and permissions

The development vault namespace is `com.pickle.reader.rune.credentials` on Mac and
`Pickle.Rune:` on Windows. It intentionally avoids replacing the legacy app's data or
credentials. Production migration and identity decisions belong to the application.

`permission_status` reads status without prompting. Accessibility/screen permission
requests are explicit functions; an ordinary selection/capture call never requests
access automatically. Mac audio requests speech authorization only after the explicit
recording action. The app bundle needs `NSSpeechRecognitionUsageDescription`.

Windows UI Automation does not grant access to protected/elevated applications. This
crate does not enable `uiAccess`, require administrator execution, or synthesize Copy.
Windows capture and OCR can fail when OS support, language packs, package identity,
graphics drivers, or source-window policy prevents them; retain passage/extension
fallbacks and test the final package.

Windows OCR explicitly requires [package identity](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr).
The traditional NSIS/ZIP build is unpackaged: the runtime package-name probe makes
`capabilities().window_capture_ocr` false there, and page capture returns an explicit
unsupported-installation error before capturing any pixels. Selected-text capture
continues to work. This combined API does not silently return an image-only reference
when OCR cannot run. An MSIX or another supported identity-bearing release and real
language-pack/device verification remain shipping gates for Windows page references.

`verify_ocr_runtime()` is a content-free installed-package probe, exposed by the
application's `--verify-windows-ocr` maintenance option. It requires actual package
identity, creates a named MTA worker, draws the fixed word `PICKLE` in Segoe UI into
an owned 768×180 memory bitmap, and runs the same Windows OCR recognition helper as
page references. It checks for the exact known word and returns only success or a
bounded static failure. Recognition has a five-second deadline; the whole probe
waits at most eight seconds. It never obtains a screen/window DC, captures user
content, installs OCR languages, requests permissions, writes images, or accesses
the network. A missing recognizer reports that a supported installed OCR language
matching the user's preferences is required. Non-Windows calls explicitly fail.

The fixture test checks rendering on Windows; portable tests check known-word
matching. Passing those tests or cross-checking the code does not prove installed
Windows recognition. Record the explicit CLI probe result after installing the
identity-bearing package on the Windows runner/device, then test real source-window
capture separately; a controlled bitmap does not exercise WGC, focus, GPU mapping,
window exclusions, or protected-source handling.

## Configured Windows transcription

Windows source audio requires an explicit `LocalTranscriberConfig`:

```text
executable: absolute path to a trusted whisper.cpp whisper-cli executable
model:      absolute path to an already downloaded local model
language:   auto, en, or another supported language code
```

Call `configure_local_transcriber` after loading or saving these preferences. It
validates files and caches the configuration. `capabilities().source_audio` is false
on Windows until configuration succeeds; capability checks do not stat files in a
view/frame loop. The engine validates again before recording, so removed files fail
without capturing audio.

Persistent settings transactions should call `prepare_local_transcriber` before
writing settings, then pass the opaque prepared value to
`install_local_transcriber` only after the write succeeds. Installation changes only
the in-memory cache and is infallible: it does not recheck files after a successful
disk commit. Recording still validates files immediately before using them.

No engine or model is downloaded automatically. The chosen executable is trusted by
the user and receives recorded audio. Pickle invokes it directly, never through a
shell, using `-m MODEL -f - -nt -np -l LANGUAGE`. whisper.cpp supports WAV on stdin.
Audio remains in memory, and transcript output is read from stdout. Child stderr is
discarded, input/output are bounded, cancellation terminates the process, and local
inference has a 180-second deadline. No output-file flags are supplied.

The Windows recorder targets only the validated source PID and child processes, using
`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`. It records up to 30 seconds of 16 kHz
mono PCM, stops on cancellation/source disappearance, rejects silence, and creates an
in-memory WAV for the configured local engine. It never falls back to default-device
loopback. The shipping OS baseline must include the process-loopback API (Windows 11
does; Microsoft documents the API from build 20348).

The configured executable is external user-selected code. Pickle does not sandbox or
prove the behavior of arbitrary binaries; only choose a trusted local whisper.cpp
build. This interface does not itself provide a bundled engine/model installation
experience. Shipping one requires packaging, licensing, integrity, and model-choice
work in the application.

## Verification and remaining device work

Development commands:

```sh
cargo +stable test -p pickle-platform
cargo +stable check -p pickle-platform --all-targets --target x86_64-pc-windows-msvc
```

The Rust Windows target must be installed for cross-checking. Cross-checking verifies
API/type integration, not Windows linking or execution.

The Mac bridge was built against a macOS 14 deployment target and exercised through a
non-prompting permission-query test. Tests cover source bounds/exclusions, native JSON
null/error decoding, cancellation before native access, in-memory WAV bounds, a local
fixture executable receiving stdin, cancellation of an active child, and oversized
transcript rejection. No private audio, model download, or live speech inference is
used by deterministic tests.

Still required on real devices:

- Browser/editor capture, permissions, menu/tray behavior, and shortcut conflicts.
- Mac source-window screenshot/OCR and actual source-app audio/Speech permissions.
- Windows WGC/D3D staging, OCR language/package compatibility, and WASAPI activation.
- Windows local whisper.cpp inference with an explicitly selected real model.
- Standard-user clean install, mixed-DPI displays, sleep/wake, source disappearance,
  cancellation during native work, and upgrade continuity.
- Minimum supported macOS and Windows releases and hardware architectures.

App executables/tests linking the Swift bridge need their own macOS Swift runtime
rpath (normally `/usr/lib/swift`). Cargo dependency `rustc-link-arg` does not propagate
that setting automatically to downstream binaries.

## Primary references

- [Microsoft process loopback parameters](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params)
- [Microsoft application loopback sample](https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/ApplicationLoopback)
- [whisper.cpp CLI](https://github.com/ggml-org/whisper.cpp/blob/master/examples/cli/cli.cpp)
- [whisper.cpp in-memory/stdin audio decoding](https://github.com/ggml-org/whisper.cpp/blob/master/examples/common-whisper.cpp)
