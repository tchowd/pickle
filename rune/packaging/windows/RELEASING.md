# Windows release preparation and finalization

The production candidate path separates compilation from access to the signing key. Existing `build-windows.ps1` and `build-identity.ps1` remain developer tools. The latter signs before invoking Cargo and must not be used as a production CI wrapper with a production key already available to dependency build scripts.

No script here imports, creates, exports, or trusts a certificate. The finalizer uses an existing certificate in `Cert:\CurrentUser\My`. It supports certificate-store signing only; managed Artifact Signing or an external signing service needs its own reviewed adapter. A self-signed development certificate is not a production credential.

## Unsigned build job

Check out the reviewed exact source commit in a clean working tree. Run this job without signing credentials, private keys, certificate import, or access to a signing service. Public update URL/key/channel, if configured, must be set before compilation. Their receipt entries are declarations, not independent proof of what was embedded.

From `rune`, on Windows with the pinned Rust toolchain, Python, and Windows SDK:

```powershell
./scripts/prepare-windows-release.ps1 `
  -PackageName $env:PICKLE_WINDOWS_PACKAGE_NAME `
  -Publisher $env:PICKLE_WINDOWS_PUBLISHER `
  -PublisherDisplayName $env:PICKLE_WINDOWS_PUBLISHER_DISPLAY_NAME `
  -IdentityVersion $env:PICKLE_WINDOWS_IDENTITY_VERSION `
  -ExpectedSourceCommit $env:GITHUB_SHA `
  -OutputDirectory "$env:RUNNER_TEMP/pickle-prepared" -Configuration release
```

This generates matching PE and unsigned sparse MSIX identity metadata, compiles the x64 app/host with static CRT policy, runs the existing PE/import and license gates, and creates `payload/` plus `prepared-receipt.json`. No installer or signing operation runs. Capture the printed receipt SHA-256 as a trusted build-job output. Transfer the prepared artifact with an authenticated CI artifact digest/provenance binding to the reviewed commit and workflow. The receipt seals file bytes and public metadata but cannot independently establish their source, review, environment, or embedded update key. A signing job must not accept an expected digest copied out of the same untrusted downloaded receipt.

## Isolated signing job

Use a fresh signing worker with the reviewed finalization scripts from the approved source commit. Authenticate the build artifact and its externally recorded receipt digest first. Supply the production's fixed package name, exact publisher subject, package family, app version and increasing four-part MSIX version. Package identity must remain stable across releases and certificate renewal; do not silently migrate it.

If the owner's signing mechanism uses a PFX, import it explicitly into `CurrentUser\My` without `-Exportable`, never into Root or TrustedPeople. Do not print its password or keep it in command arguments. Record every newly imported certificate/key for exact cleanup in `finally`; preserve pre-existing entries. Production trust must validate through the platform's existing CA trust. Missing trust is an error, not permission to import a root or peer certificate. Hardware-backed/managed credentials may require a different owner-supplied signing adapter.

```powershell
./scripts/finalize-windows-release.ps1 `
  -PreparedDirectory "$env:RUNNER_TEMP/pickle-prepared/payload" `
  -ReceiptPath "$env:RUNNER_TEMP/pickle-prepared/prepared-receipt.json" `
  -ExpectedReceiptSha256 $env:PICKLE_AUTHENTICATED_RECEIPT_SHA256 `
  -ExpectedSourceCommit $env:GITHUB_SHA `
  -ExpectedAppVersion $env:PICKLE_RELEASE_VERSION `
  -PackageName $env:PICKLE_WINDOWS_PACKAGE_NAME `
  -Publisher $env:PICKLE_WINDOWS_PUBLISHER `
  -IdentityVersion $env:PICKLE_WINDOWS_IDENTITY_VERSION `
  -ExpectedPackageFamily $env:PICKLE_WINDOWS_PACKAGE_FAMILY `
  -CertificateThumbprint $env:PICKLE_SIGN_CERT_THUMBPRINT `
  -OutputDirectory "$env:RUNNER_TEMP/pickle-finalized"
```

The finalizer statically validates the exact receipt, payload allowlist/hashes, x64 import policy, PE product version and embedded identity, unsigned MSIX manifest and registration metadata. It does not run Cargo, dependency build scripts, staged executables, or the installer. It signs app/host/MSIX with SHA-256 and an RFC3161 timestamp, refreshes the signed identity-package hash, compiles the reviewed NSIS source with a dedicated uninstaller signing hook, and signs the installer. All final PE/MSIX signatures are independently checked with SignTool's normal trust policy and mandatory timestamps, plus exact signer checks. The generated uninstaller is retained only as verification evidence and is never executed by the finalizer.

Final output is `PickleRune-setup.exe`, `PickleRune-windows.zip`, `release-report.json`, and selected inspection/signature evidence. Signing and packaging occur in a unique owned work directory beneath the fresh output directory. Candidates are promoted only after all signature, timestamp, report and 1-byte–512-MiB artifact-size gates pass; a promotion failure removes any newly promoted files and deletes work. Prepared input is preserved. Only the two explicitly named deliverables are release candidates; do not publish the entire output directory or a broad artifact glob. Hashes are recorded after platform signing and packaging. The Windows update artifact should be the identity-bearing NSIS installer, not the ZIP: extracting the ZIP does not register identity. Rename/copy candidates to immutable versioned release paths, then sign the updater manifest using the exact resulting bytes and the independently verified Ed25519 public key. See [UPDATES.md](../../scripts/UPDATES.md). GitHub asset redirects are refused by the app's downloader; the owner must supply a direct HTTPS feed/file endpoint.

Remove or disable the imported signing key/service access before any installer or staged app is executed. Run `test-windows-package.ps1 -Configuration release -ExpectedVersion ... -InstallerPath ...` on the finalized installer on a separate disposable acceptance worker. Existing hosted CI tests use an explicitly disposable certificate and machine peer trust solely in the guarded test harness; optimized CI now prepares before creating that test key, finalizes afterward, deletes its private key, then runs installed identity/OCR/native-window/lifecycle tests. These development identities and test-signed assets must never be published as production.

The release workflow should fail closed when owner identity, certificate, update public key/feed, immutable artifact destinations, or authenticated provenance are missing. It should upload reviewable candidates, not automatically publish or alter trust. Same-version repair is covered; real N→N+1 upgrade/downgrade, standard-user Windows client acceptance, publisher reputation, and rollback remain separate acceptance gates. No hosted-server smoke substitutes for that client matrix.

Portable validation: `pwsh -NoProfile -File scripts/test-windows-release.ps1`. Native signing/finalization and installed acceptance require the hosted Windows CI test. This documentation does not claim production credentials or a deployed feed exist.
