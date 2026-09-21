# Pickle browser context

This development extension shares the current page or available captions with the native Pickle app. It runs only when invoked; it does not continuously watch tabs, export cookies, download videos, or send content to an AI provider itself.

## Connect

1. Build and open the latest Pickle app.
2. Enable **Settings → Privacy → Browser and video context → Include webpage references**.
3. Open **Connect browser extension → Show extension folder**.
4. In Chrome's extension manager (`chrome://extensions`), enable Developer mode and choose **Load unpacked**. Select that folder.
5. Copy the extension ID. Back in Pickle, select Chrome, paste the ID, and click **Connect**.
6. Pin the Pickle extension button. Its **Use this page**, **Explain this moment**, and **Summarize video** actions prepare a new Pickle session. Choose a reading action in Pickle to generate an answer.

The native bridge currently requires `/usr/bin/python3` (available with Apple Command Line Tools on the development Mac). Edge, Brave, and Arc have setup choices but need live compatibility testing. Safari and Firefox extension packaging is not included. The native Control + Option shortcut can still capture an exposed browser URL without an extension.

Repository alternative: `python3 scripts/install-browser-bridge.py EXTENSION_ID chrome` from the project root. Rerun setup if you move Pickle or change the extension ID. To disconnect, remove the extension and its `com.pickle.reader.json` file from the browser's `NativeMessagingHosts` folder.

## Video behavior

- **Explain this moment** includes available cues from 90 seconds before the current playback position through 10 seconds after it. The captured timestamp appears in Pickle.
- **Summarize video** includes available transcript text, capped at 12,000 UTF-8 bytes. Truncation and uncertain completeness are labeled. This does not imply access to the whole video.
- Standard HTML video caption tracks work when cues are already loaded. For YouTube, open **Show transcript** first; the extension reads transcript rows exposed in the page. This adapter may need updates when YouTube changes its layout.
- Embedded/cross-origin players should be opened on their own page. Private tabs are rejected. Captionless videos report an actionable error rather than inventing video content.
- For audio fallback, play the video, invoke Pickle from the source browser with Control + Option, then open **No captions? → Listen for 30 seconds**. This needs Screen Recording and Speech Recognition access and an available on-device recognizer. It captures source-application audio, including other playing tabs in that app. It cannot recover earlier audio; replay that part first. Audio stays local and no recording file is saved. The resulting transcript can be shared through an explicit reading action.

## Data handling

References live in the session and are cleared for a new session. A native messaging host relays bounded JSON through an authenticated loopback connection; only the connection port/token are written to disk. Source text is never written to bridge configuration. The host manifest allows only the configured extension ID. Pickle requires a separate disclosure before including references in AI requests. Offline mode blocks public-page fetching and AI calls; local browser extraction is still possible.

The native fetcher contacts only the current public page, without browser cookies, redirects, subresources, or webpage JavaScript. It pins a validated public IPv4 address, limits the body to 2 MB, and falls back after five seconds. It cannot read login-only content or every dynamic site. The extension reads the loaded document for those cases.

Vendored Readability 0.6.0 is maintained by Mozilla and licensed under Apache-2.0; see `Readability-LICENSE.md`.
