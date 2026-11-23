# Finalizing a Mac release

`scripts/finalize-macos-release.py` prepares an **existing, production-signed** Pickle app for ZIP distribution. It copies the input to a new output directory, notarizes the copy through an existing Keychain profile, staples it, and verifies the final archive. It does not build, sign, install, launch, or publish an app; configure credentials; import certificates; change permissions; or alter the input bundle.

The existing `build-macos.sh release` command builds optimized code but still defaults to ad hoc signing. Optimization alone is not a production release. Its `PICKLE_SIGN_IDENTITY` input enables signing of the nested browser host and then the app with hardened runtime and a secure timestamp. The release owner must select the intended **Developer ID Application** identity. Never automatically choose an identity merely because it exists on a machine. [Apple describes the appropriate identity, signing order, timestamp, and runtime options.](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac)

## Owner inputs and prerequisites

Before an actual finalization, the release owner must approve the exact bundle and the upload to Apple's notary service, and supply:

- An already signed app and nested `pickle-browser-host`, both signed by the expected Developer ID Application team. Ad hoc, Apple Development, Mac App Store, wrong-team, missing-runtime, and missing-timestamp signatures are rejected.
- The expected ten-character Team ID, numeric `major.minor.patch` version, and exact architecture (`arm64` or `x86_64`). Both executables must contain exactly that architecture. Universal binaries are deliberately unsupported by this first finalizer; use separate thin artifacts and do not claim Intel support from an Apple Silicon-only build.
- An existing, owner-configured `notarytool` Keychain profile. The finalizer accepts the profile name, never an Apple ID, password, private key, or certificate. It does not create or unlock a Keychain. A locked or unavailable profile prevents unattended completion; prepare access before invoking the finalizer.
- macOS 14 or later, Python 3.10 or later, and a selected Apple developer toolchain that provides `notarytool` and `stapler`. `syspolicy_check` is required; there is no bypass when a policy tool is absent.
- An output path that does not exist, with an existing parent directory. The output must be outside the input app.

Choose the release version and build number before signing. The current `Info.plist` template contains fixed version/build values; keep `CFBundleShortVersionString` consistent with Cargo's package version, and assign a build number appropriate for the release sequence. The finalizer verifies the expected plist version and build-number syntax, but cannot determine whether a build number is newer than a previously shipped release. Run the existing package inspection separately to verify the executable's `--version`, notices, payload, and loader dependencies.

No extra entitlements are added. Existing claims are inspected and an enabled `com.apple.security.get-task-allow` is rejected. Required entitlements and the behavior of the actual hardened-runtime build still need release review and native feature tests; notarization is not a functional test. [Apple's notarization prerequisites include valid Developer ID signatures, hardened runtime, secure timestamps, and removal of debugger attachment.](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)

## Explicit finalization

### Sign a prepared copy in an isolated environment

Complete compilation and the existing package inspections in a separate unsigned build job, without production signing-key access. Authenticate that job's exact commit and transferred artifact digest before making signing credentials available. Do **not** run `build-macos.sh` in the credential-bearing signing stage: it always invokes Cargo before packaging, even when `PICKLE_SIGN_IDENTITY` is set. Merely unsetting that variable does not remove a dependency build script's access to an already available Keychain identity.

After the owner has selected the existing identity and authenticated the prepared bytes, sign only a fresh copy. The following inputs are explicit owner-provided paths and a public signing-identity selector; no secret is passed in command arguments:

```sh
set -euo pipefail
: "${PICKLE_UNSIGNED_BUNDLE:?authenticated prepared .app path required}"
: "${PICKLE_SIGNED_BUNDLE:?new staging .app path required}"
: "${PICKLE_SIGN_IDENTITY:?owner-selected Developer ID Application identity required}"
test ! -e "$PICKLE_SIGNED_BUNDLE"
test ! -L "$PICKLE_SIGNED_BUNDLE"
/usr/bin/ditto "$PICKLE_UNSIGNED_BUNDLE" "$PICKLE_SIGNED_BUNDLE"
/usr/bin/codesign --force --options runtime --timestamp \
  --sign "$PICKLE_SIGN_IDENTITY" \
  "$PICKLE_SIGNED_BUNDLE/Contents/MacOS/pickle-browser-host"
/usr/bin/codesign --force --options runtime --timestamp \
  --sign "$PICKLE_SIGN_IDENTITY" "$PICKLE_SIGNED_BUNDLE"
/usr/bin/codesign --verify --deep --strict --all-architectures "$PICKLE_SIGNED_BUNDLE"
```

These commands do not compile dependencies or execute the received app. They do not import, unlock, select, or create a Keychain identity on the owner's behalf. An existing controlled signing environment must already provide authorized access. Keep the prepared source copy intact, do not deep-sign, and do not add entitlement exceptions without explicit native-feature review. The finalizer below independently enforces the expected Team ID, Developer ID requirement, runtime/timestamp and architecture before upload. Verification/acceptance that executes the candidate belongs in a separate environment after signing access has been removed.

From the repository root, after the owner has prepared and approved the production-signed bundle:

```sh
python3 rune/scripts/finalize-macos-release.py \
  --bundle "/owner/staging/Pickle Rune.app" \
  --output "/owner/releases/pickle-1.2.3-arm64" \
  --team-id ABCDEFGHIJ \
  --version 1.2.3 \
  --architecture arm64 \
  --keychain-profile "owner-notarization-profile"
```

These are placeholder owner paths and identity values. This command uploads software to Apple; it is not a dry run. The profile must already be configured by the owner. The default wait is 1,800 seconds; `--wait-seconds` accepts 60–3,600 seconds. If processing times out, Apple's processing may continue. Inspect the submission before deciding whether to resubmit. There is no automatic retry, credential-setup, or publication step.

The finalizer performs these gates in order:

1. Inspect the expected app identity/version and the two required executables. Reject symlinks, special files, extra Mach-O code, missing code, and nonexecutable payloads before copying. This product currently bundles no symlinked frameworks; adding one requires revisiting this explicit payload policy.
2. Stage a copy under a temporary directory in the new output directory using `ditto`. Verify both code signatures against Apple's trust anchor, the Developer ID issuer/application certificate extensions, and the expected team. The outer signature must also identify `com.pickle.reader.rune`. Inspect runtime flags, secure timestamps, entitlements, and architecture; verify nested integrity with `codesign --verify --deep --strict --all-architectures`. The finalizer never performs deep signing. [Apple documents the Developer ID requirement terms.](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements)
3. Create a submission ZIP and run `notarytool submit` with the existing Keychain profile, bounded wait, and JSON response. Require the exact `Accepted` status and a matching accepted submission log. Submission identifiers are normalized as UUIDs, including Apple's upper/lower-case spellings.
4. Staple and validate the ticket on the copied `.app`. Recreate the distribution ZIP **after** stapling. Require a nonempty ZIP no larger than 512 MiB, matching the update downloader and manifest signer. A ZIP cannot itself be stapled; Apple recommends stapling its app contents and then rebuilding the ZIP. [Apple describes submission, log review, and ZIP stapling.](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)
5. Extract the final ZIP into a fresh temporary directory. Recheck its exact top-level app, layout, version, signatures, architecture, and stapled ticket. Require both `syspolicy_check distribution` and `spctl --assess --type execute` on that extracted copy. [Apple recommends system-policy assessment and fresh-machine testing for trusted execution.](https://developer.apple.com/forums/thread/706442)
6. Only after every gate passes, expose the copied app, final ZIP, SHA-256 checksum, and a passing release report. The report's binary hashes come from the final extracted archive. No installed app is replaced or launched.

## Outputs, failure handling, and evidence limits

A successful new output directory contains:

```text
Pickle Rune.app/
PickleRune-1.2.3-macos-arm64.zip
SHA256SUMS
macos-release-report.json
notarization-submission.json
notarization-log.json
```

The report records the expected version/team/architecture, native signature checks, Apple's accepted submission ID, final ticket/policy checks, and the ZIP checksum/size. It expressly records that clean-machine testing, source review, and build reproducibility were not attested. It is evidence about the owner-supplied signed bytes and current host policy, not evidence that those bytes came from a reviewed commit or that the app works on a clean customer Mac.

On failure, temporary app copies, raw tool output, intermediate ZIPs, and extraction directories are removed. No final ZIP/app, checksum, or passing report is published. If Apple returned a usable submission ID, bounded submission metadata and a sanitized issue log can remain for investigation. The retained log excludes unknown fields and contains at most 32 issues with bounded field lengths; counts disclose omissions. It can contain Apple's diagnostic descriptions and paths inside the submitted product. Authentication command output is not echoed. If submission fails before a usable response, no ID may be available locally; the owner can inspect the existing profile's submission history separately.

The output directory is never reused or overwritten, including after a failed attempt. Preserve any useful notary evidence, choose a new output path for another attempt, and do not treat an accepted submission alone as a completed release.

Before shipping, independently test the exact final ZIP on fresh Mac environments for the supported OS/architecture combinations. Download through a browser, unpack normally, verify first-launch Gatekeeper behavior, and check offline first launch with the stapled ticket. Exercise selection, conventional shortcut, modifier gesture, OCR, source audio, Keychain authorization, browser-host registration, and upgrade/manual replacement while preserving data. The finalizer does not grant those permissions, reset Gatekeeper caches, disable quarantine, establish clean-machine evidence, or prove native feature behavior. Keep any outstanding native failures open even if notarization succeeds.

Only then provide the **final ZIP** to `scripts/sign-update.py`, using the owner-managed update key and HTTPS publisher endpoint described in `scripts/UPDATES.md`. Stapling changes deliverable bytes, so signing update metadata from the submission ZIP or a prestapled artifact would be incorrect. The finalizer never signs update metadata or uploads an artifact to a publisher.

## Portable fixture tests

```sh
python3 rune/scripts/test_finalize_macos_release.py
```

The tests inject fake command responses and fixture ZIPs. They cover trust/runtime/timestamp/architecture rejection, exact notarization acceptance, submission matching, bounded log retention, ordering, final-ticket survival, untouched inputs, and cleanup on failure. They execute no Apple signing/notary/Gatekeeper commands and require no release credentials. A passing fixture suite is not a real Developer ID or notarization result.
