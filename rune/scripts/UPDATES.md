# Signing and publishing updates

The application update service is deliberately unconfigured until a publisher supplies a trusted manifest URL and public signing key at build time. No update request runs at startup. Checking, downloading, and opening an installer are separate explicit actions. The update module never executes an installer.

## Trust and manifest format

Set these environment variables when building a release:

```text
PICKLE_UPDATE_MANIFEST_URL=https://publisher.example/pickle/stable.json
PICKLE_UPDATE_PUBLIC_KEY_HEX=<64 hex characters encoding an Ed25519 public key>
PICKLE_UPDATE_CHANNEL=stable
```

The private signing key stays outside the repository and application bundle. Keep it in a protected signing environment; do not embed it in CI files or distribute it to users. Signing-key rotation/revocation and publisher hosting are release-owner decisions. A manifest cannot introduce its own trusted key.

The JSON envelope has exactly `schema`, `payload_hex`, and `signature_hex` fields. Schema is `1`. The signature authenticates:

```text
UTF-8 bytes of "Pickle update manifest v1" + a zero byte + canonical payload bytes
```

Payload bytes are decoded and signature-checked before any payload URL is parsed. Canonical JSON uses compact UTF-8 serialization and these field orders:

```json
{"version":"1.0.0","channel":"stable","published_at":"2026-11-22T12:00:00Z","expires_at":"2026-12-06T12:00:00Z","notes":"Release notes","artifacts":[{"target":"aarch64-apple-darwin","url":"https://publisher.example/Pickle-1.0.0.dmg","sha256":"<64 hex characters>","size":123,"filename":"Pickle-1.0.0.dmg"}]}
```

Version/channel, validity dates, notes, every artifact URL, checksum, size, target, and filename are covered by the signature. Unknown fields, noncanonical payload encodings, duplicate targets, expired manifests, future publication dates, validity longer than 30 days, unsupported installer formats, and unsafe filenames are rejected. The stable channel rejects prerelease versions. Version comparison ignores build metadata and prevents offering equal or older versions.

## Release signing workflow

Requirements: Python 3 and OpenSSL 3 on the release machine. The script uses an existing Ed25519 PEM key and does not generate a key. Establish the key through the publisher's controlled key-management process before using this command.

First build and platform-sign/notarize the actual deliverables. Hash and manifest signing must happen after those operations, since they can change installer bytes.

Example publisher command, using placeholder locations:

```sh
python3 rune/scripts/sign-update.py \
  --openssl /path/to/openssl-3 \
  --key /protected/signing/pickle-updates.pem \
  --version 1.0.0 \
  --channel stable \
  --notes "Release notes for this version." \
  --artifact aarch64-apple-darwin release/Pickle-1.0.0.dmg https://publisher.example/pickle/Pickle-1.0.0.dmg \
  --artifact x86_64-pc-windows-msvc release/Pickle-1.0.0.exe https://publisher.example/pickle/Pickle-1.0.0.exe \
  --output release/stable.json
```

The script prints only completion and the public verification key. It reads installers in bounded chunks, computes SHA-256/size, constructs canonical payload bytes, invokes OpenSSL signing, and writes the envelope through atomic replacement. It never prints or copies the private key. Temporary signing inputs contain the public payload and are removed after signing.

Verify that the resulting public key matches the trust root compiled into the application. Publish immutable installers first, then publish the signed manifest. URLs must return the actual bytes directly over HTTPS; redirects are refused. A GitHub release asset URL that redirects to another host is therefore not a working update URL without a publisher endpoint that serves the file directly.

Refresh the signed manifest before its expiry. Expiry enforcement depends on the user's system clock; clock errors produce a recovery message rather than disabling signature checks.

## Download and installation behavior

- The manifest is limited to 64 KiB, with a 24 KiB signed payload.
- Installers are limited to their signed size and a 512 MiB hard cap.
- HTTPS is mandatory; redirects, shared cookies, automatic retries, and proxies are disabled.
- A download uses an owner-local cache and a temporary file in that directory.
- Cancellation, interrupted transport, oversized data, or mismatched SHA-256 leaves no published installer.
- Atomic replacement occurs only after the final byte count and hash match the signed metadata.
- Cached files are rehashed before reuse, and symlink cache destinations are refused.
- The app may reveal the verified file for the user to inspect and install. Platform code signing remains a separate requirement; the manifest signature does not replace Developer ID, notarization, Authenticode, or installer validation.

This is a verified-download workflow, not an automatic in-place updater. Installation, app replacement, update rollback, and data-schema downgrade handling still belong to the selected installer/release process and must be tested on both operating systems before shipping.
