# Pickle Local AI deployment

Configured on September 21, 2026:

- Client: specter. Inference runs on **ross**, Apple M4 Max / 64 GB.
- Ollama 0.34.1 remains at `127.0.0.1:11434` on Ross. Existing process and Qwen 27B installations were preserved; neither large model was loaded.
- Downloaded `llama3.2:1b`, digest `baf6a787fdffd633537aa2eb51cfd54cb93ff08e28040095462bb63daf552878`, 1.32 GB on disk, Q8_0.
- A Pickle-only gateway runs on Ross at `127.0.0.1:11435`, from `~/.pickle-local-ai/gateway.py`. Its user LaunchAgent is `~/Library/LaunchAgents/com.pickle.local-ai-gateway.plist`.
- The gateway accepts only the pinned downloaded model, disallows remote/cloud model metadata, management endpoints, image uploads and oversized contexts, and permits one generation at a time. It requires a bearer key, stored in a mode-0600 file on Ross and in the client Keychain. It forwards cancellation by closing the upstream connection. Existing global Ollama cloud configuration was not changed.
- On specter, `com.pickle.ross-tunnel` forwards `127.0.0.1:11436` to the Ross gateway using the existing SSH alias `ross`, whose destination is that machine's Tailscale IP. No router forwarding, Funnel, public listener, or tailnet policy changes were made.
- Pickle's configured server is `http://127.0.0.1:11436`. **This is a tunnel to another computer, not same-device inference.** Its same-device switch is off; strict offline mode blocks it.

## Operation

Local is the default. Choose **Online** or **Local** in **Settings → Connection → AI location**. Your choice is remembered; Cloudflare credentials, model and Jev preference are unchanged. Test connection reads metadata only; it does not load the model or generate an answer.

The SSH tunnel starts at login, but exits if the connection fails and does not retry indefinitely. If Ross sleeps or the connection drops, use **Reconnect to Ross** in Connection settings, then **Test connection**. Reconnecting is explicit; Pickle sends no wake packets. Offline mode prevents inference through this tunnel. The gateway remains a lightweight loopback service and uses the existing Ollama process.

## Tailscale Serve status

Installed Tailscale is 1.94.1. There were no existing Serve routes. The requested private HTTPS command reported **Serve is not enabled on your tailnet**; the waiting command was stopped and Serve configuration remains empty.

To replace the working SSH tunnel with preferred private HTTPS, the tailnet administrator must enable Serve and HTTPS in the Tailscale admin console.

Then inspect current routes and add only the intended route on Ross:

```sh
/Applications/Tailscale.app/Contents/MacOS/Tailscale serve status
/Applications/Tailscale.app/Contents/MacOS/Tailscale serve --bg --https=443 http://127.0.0.1:11435
```

Use the inference machine's MagicDNS name in Pickle and save the gateway key for that address (keys are endpoint-scoped). Restrict that service to specter in existing Tailscale policy where possible; do not replace the policy or unrelated routes. Gateway authentication remains required. Do not enable Funnel. Direct HTTPS and its device ACL have **not** been configured or verified; the SSH route is live and tested.

## Removal and maintenance

Unload only Pickle's agents using `launchctl bootout gui/$(id -u)/com.pickle.ross-tunnel` on specter and `launchctl bootout gui/$(id -u)/com.pickle.local-ai-gateway` on Ross, then remove their corresponding plist files. This does not stop Ollama or remove models. Never commit `access-token` or copy it into diagnostic logs.

The gateway source is in this folder; tests are run with `python3 scripts/local-ai/test_gateway.py`. Its deployed model digest is intentionally pinned. An operator must explicitly review/update that pin after changing model weights. Other models, including a future vision model, require a reviewed gateway allowlist change or a separate trusted endpoint; Pickle never downloads them.

Official references: [Ollama chat API](https://docs.ollama.com/api/chat), [context/keep-alive configuration](https://docs.ollama.com/faq), [Tailscale Serve](https://tailscale.com/docs/reference/tailscale-cli/serve).
