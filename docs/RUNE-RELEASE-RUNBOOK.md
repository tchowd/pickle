# Pickle Rune release runbook

This is the operational path from a reviewed source commit to Mac and Windows deliverables. It complements the [implementation status](RUNE-IMPLEMENTATION-STATUS.md), [native evidence](RUNE-NATIVE-ACCEPTANCE.md), and [full migration plan](RUNE-CROSS-PLATFORM-PLAN.md). The finalization tools are separate from ordinary development packaging. Their presence does not establish successful production signing, notarization, publishing, or device acceptance.

## Release inputs and ownership

Record these public inputs together before preparing a candidate:

| Input | Required decision or evidence |
|---|---|
| Source | An exact reviewed commit and its passing build/test run. Do not use a branch name as immutable provenance. |
| Version | Cargo/application version, Mac bundle version, and Windows four-part identity version. Document the identity-version mapping and require monotonic upgrades. |
| Support | The exact tested OS versions, architectures, source applications, browser versions and known feature differences. The proposed initial matrix is Apple Silicon macOS 14+ and Windows 11 x64; hosted latest-OS checks do not prove that matrix. |
| Mac publisher | Authorized Developer ID Application identity, exact Team ID, and an existing notarization keychain profile in the controlled signing environment. |
| Windows publisher | Authorized existing code-signing certificate, expected thumbprint and exact subject, permanent package name/family, and publisher display name. Preserve these across updates. |
| Update trust | Publisher-owned Ed25519 public key, stable/beta channel, and direct HTTPS manifest/artifact URLs. Both application builds must use the same chosen trust root and channel. |
| Publication | Owner of immutable artifact hosting, manifest replacement, expiry monitoring, key recovery and support. A redirecting GitHub release asset URL does not satisfy the current update client. |

Private keys, certificate export passwords, notarization credentials and signing-service tokens belong in the publisher's controlled credential system. Do not put them in source, release notes, commands in a chat, test fixtures or uploaded evidence. The finalizers use existing signing facilities; they do not create a publisher identity, import trust or grant new permissions.

An existing identity on a developer machine is not evidence that its owner authorized this product's release. Confirm the intended publisher before using it. No production identity or update domain is selected by this runbook.

## 1. Prepare without production signing credentials

Use a clean build environment at the exact reviewed commit. Keep production signing credentials unavailable to dependency compilation and build scripts. Build jobs and credential-bearing signing jobs should use separate isolated environments; changing environment variables within a shared runner is not a security boundary against a background process left by a dependency.

1. Run the repository checks and both native CI package profiles. Preserve logs, dependency lockfile, pinned Rune/toolchain revisions and target-specific notice inventories.
2. Set the public update configuration consistently before compiling both platforms. An omitted feed leaves update checking unavailable; that is a release-scope decision, not a completed updater deployment.
3. Build the Mac release bundle without a production identity. Prepare the Windows unsigned identity-bearing stage with the public permanent package identity and matching PE manifest.
4. Record the exact source commit, target, versions, public update configuration, file inventory and SHA-256 values. Retain the build service's authenticated artifact/provenance information separately.
5. Transfer only the selected prepared artifacts and public evidence into the signing environment. Do not rebuild dependencies there.

A receipt delivered alongside an artifact can detect accidental changes against its listed hashes. It does not independently prove who built the artifact, what source was reviewed, or which configuration was compiled. Verify the authenticated build-run identity and expected artifact digest outside the received receipt before making a signing key available.

Never substitute a later bundle, an old `Previous.app`, the newest staging directory, or an artifact from another commit merely because its filename matches. The currently tested local development app and each hosted CI bundle have separate identities in the native record.

## 2. Finalize platform artifacts

Follow the platform procedures and exact tool arguments:

- [Mac signing and notarization](../rune/packaging/macos/RELEASING.md): sign the selected prepared bundle in the controlled environment, then finalize a copy with explicit expected publisher/version/architecture. Require accepted notarization, a stapled ticket, final archive extraction/reverification and Gatekeeper assessment before using its final checksum.
- [Windows preparation and finalization](../rune/packaging/windows/RELEASING.md): prepare without the certificate, then sign the verified stage with the expected existing publisher. Require independent signature, signer and timestamp checks for the application, browser host, identity package, generated uninstaller and final installer. Preserve the exact package identity and stable installation path.

Finalizers must not invoke Cargo or execute received application binaries while the production signing capability is available. Review the packaging scripts and tools executed in that environment as part of the release commit. Use fresh output locations and preserve the unsigned/prepared input for diagnosis.

Do not treat a successful submission, timestamp request or signing command as completion. The corresponding verification and finalization report must succeed. On failure, retain bounded diagnostic evidence and leave the candidate unpublished; do not hash or upload a partial output under the final release filename.

The disposable hosted Windows certificate used by CI is test-only. Its clearly named artifacts are evidence of the signing/install protocol, not a production publisher identity or a user release. Never import that certificate on a user machine to make an artifact appear trusted.

## 3. Test the finalized candidate

Run acceptance against the exact finalized bytes and record their hashes, source commit, publisher and test environment. Keep the [native checklist](RUNE-NATIVE-ACCEPTANCE.md) and plan's release gates open until the corresponding evidence exists.

- Install and first launch on supported clean standard-user systems without development tools. Verify Gatekeeper/publisher behavior and installed runtime loading.
- Exercise select → invoke → choose → read → follow-up → close/reopen in supported native editors and browsers, including actual keyboard layouts and focus behavior.
- Test denied/revoked capture permissions, protected/excluded sources, pause/cancel, stale context and destination-scoped consent. Real source-window OCR is separate from generated-image OCR.
- Validate browser installation/update/disconnect, supported captions, source-app audio/transcription, persistence/import/restart, accessibility and the declared display matrix.
- Install over the previous supported version; exercise running-app refusal, interrupted/failed updates, rollback and retained-data behavior. Same-version repair is not cross-version upgrade evidence.
- Measure optimized startup, latency, idle resource use and repeated-session memory against declared budgets. Native window responsiveness probes alone do not establish these budgets.

The user declined provisioning a Windows 11 machine or VM during this work. Do not replace that decision with an automatically provisioned environment or silently count hosted Windows Server tests as Windows 11 acceptance. The missing device evidence remains visible for the release owner.

## 4. Sign update metadata after finalization

Use [the update signing procedure](../rune/scripts/UPDATES.md) on the final platform artifacts after signing, notarization, stapling and archive creation have finished changing their bytes.

The signer requires the expected public key compiled into the application. It derives the selected private key's public key and fails before signing if they differ. Its output path must be separate from the key and all installers. A manifest signature authenticates the download metadata; it does not replace platform signing or make an untested installer safe to publish.

After removing signing-key access, use each finalized candidate's `--verify-update-manifest FILE --from-version VERSION` command. It uses that binary's compiled trust root/channel/target and requires the signed version to match its Cargo version. It performs no network, storage, lease or UI initialization. See the update procedure for the exact invocation and output boundaries. Compare its reported size/hash with the independent final artifact report; the command verifies metadata and does not hash the installer itself. Check version ordering, validity window and user-visible release notes. Preserve the exact signed manifest bytes with the final artifact records. A public deterministic test key must never become the release trust root.

## 5. Publish as an explicit owner action

Publication is the final step after the exact candidate, reports, acceptance gaps and known issues are reviewable. This runbook does not automatically create a release, change a repository's default branch, modify certificate trust or upload software to an update host.

1. Approve the declared support scope and remaining known issues against the full plan. Do not infer approval from green CI.
2. Publish immutable finalized installers/archives first. Confirm that each intended HTTPS URL serves the exact signed size/hash directly, without redirects.
3. Publish the signed manifest last. Confirm that the live response matches the reviewed bytes and verify a check/download with the actual installed client.
4. Archive source/build provenance, signature/notarization reports, notice inventories, final checksums, signed manifest and acceptance records. Keep private credentials out of this archive.
5. Monitor availability and manifest expiry. For a failed rollout, stop offering the bad candidate and follow the documented rollback/data compatibility procedure; never disable signature or expiry checks as a recovery shortcut.

The current client offers explicit verified downloads and lets the user reveal the installer. It does not install or replace the running app automatically. Recovery instructions and manual installation acceptance must match that behavior.
