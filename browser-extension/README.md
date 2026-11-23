# Pickle browser context

This development extension shares the current page or available captions with the native Pickle app. It runs only when invoked; it does not continuously watch tabs, export cookies, download videos, or send content to an AI provider itself.

## Connect to Pickle Rune on Mac or Windows

1. Open the packaged Pickle Rune app. The package contains its own Rust native-messaging host; users do not need Python or developer tools to run that host.
2. Enable **Settings → Include webpage and browser references**.
3. Choose **Show extension folder** in Pickle Settings.
4. In Chrome (`chrome://extensions`) or Edge (`edge://extensions`), enable Developer mode and choose **Load unpacked**. Select the folder Pickle revealed.
5. Copy the extension ID. Back in Pickle, paste it into **Extension ID**, then choose **Connect Chrome** or **Connect Edge** for the browser you configured.
6. Pin the Pickle extension button. Its **Use this page**, **Explain this moment**, and **Summarize video** actions prepare a new Pickle session. Choose a reading action in Pickle to generate an answer.

On Mac, the default invocation is Control + Option pressed together and released. On Windows it is Ctrl + Alt + P. Selection and an exposed browser URL can be captured without the extension when the source app provides them; unavailable sources retain the paste path. Chrome/Edge installation, real sites, browser updates and copied-host continuity still require native acceptance on each advertised platform. Safari, Firefox, Brave and Arc are not established Rune extension targets by these instructions.

Repeat **Connect Chrome/Edge** after moving Pickle, changing the extension ID or updating a copied host. To disconnect Rune, use its matching **Disconnect Chrome/Edge** control and remove the extension in that browser. The app removes only registrations/host files it owns. Do not delete unrelated or original-app registrations by filename.

The ordinary Windows ZIP/NSIS build can use the browser bridge. Choose the signed identity-bearing Windows installer when you also need source-window OCR; extracting a ZIP does not register that identity.

## Original Swift Mac app

The original Swift application remains in this repository alongside Rune. Its bridge uses `/usr/bin/python3` and its setup is under **Settings → Privacy → Browser and video context → Connect browser extension**. The repository alternative `python3 scripts/install-browser-bridge.py EXTENSION_ID chrome` is for that legacy Mac bridge; it is not Rune's Windows/Rust setup. Its additional browser choices still need live compatibility testing. Use each app's own setup/disconnect controls to preserve separate bridge ownership.

## Video behavior

- **Explain this moment** includes available cues from 90 seconds before the current playback position through 10 seconds after it. The captured timestamp appears in Pickle.
- **Summarize video** includes available transcript text, capped at 12,000 UTF-8 bytes. Truncation and uncertain completeness are labeled. This does not imply access to the whole video.
- Standard HTML video caption tracks work when cues are already loaded. For YouTube, open **Show transcript** first; the extension reads transcript rows exposed in the page. This adapter may need updates when YouTube changes its layout.
- Embedded/cross-origin players should be opened on their own page. Private tabs are rejected. Captionless videos report an actionable error rather than inventing video content.
- For audio fallback in Rune, play the video, invoke Pickle from that browser, then choose **Listen for 30 seconds**. Mac uses an available on-device Speech recognizer and the required OS grants. Windows requires a separately configured trusted local whisper executable/model. This captures the selected application's audio scope, which can include other playing tabs or descendant processes; it does not recover earlier audio, so replay that part first. No recording file is saved. The resulting transcript remains untrusted session context and is shared only through an explicit reading action. Actual audio/runtime compatibility remains a separate native acceptance gate.

## Data handling

References live in the session and are cleared for a new session. A native-messaging host relays bounded JSON through an authenticated loopback connection. Bridge files contain connection/authentication and registration metadata, not page or caption text; the host manifest allows only the configured extension ID. Pickle requires a separate disclosure before including references in AI requests. In Rune, strict offline blocks public-page fetching, cloud AI and inference on another computer; explicitly confirmed inference on this device and local browser extraction remain available. Saved-answer/history policy belongs to the app and is separate from bridge configuration.

Rune can attempt optional screen context before opening the chooser when screen context is enabled and existing OS grants permit it. The extension sends its exact `location.href` as a temporary document URL, independently of the retained reference pointer. Rune requires the registered browser's foreground native window and one exposed top-level document with that exact URL, rechecking document/window identity and URL around capture. It never reads an address-bar/link URL as page proof. The temporary URL is not retained in answers or diagnostics. Older packets, ambiguous windows, changed pages, unsupported accessibility trees and slow OS calls retain text/captions without OCR. These observations cannot make navigation atomic or prove an extension tab ID; real Chrome/Edge acceptance remains open. They do not narrow app audio to one tab.

The native public-page fetcher uses validated, pinned public addresses with a bounded body/deadline. It sends no browser cookies, follows no redirects and executes no webpage JavaScript or subresource requests. It cannot read login-only content or every dynamic site; the extension can extract the loaded document when explicitly invoked. Extraction and caption availability vary by page and do not imply full-video access.

Vendored Readability 0.6.0 is maintained by Mozilla and licensed under Apache-2.0; see `Readability-LICENSE.md`.
