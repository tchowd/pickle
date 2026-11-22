# Windows identity and release packaging

The ordinary ZIP and NSIS build is an unpackaged development path. It supports selected text and other available desktop integration, but it does not establish the package identity required by [Windows.Media.Ocr](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr?view=winrt-26100). Full OCR validation requires a registered package identity on Windows 11. App capability checks report unavailable identity honestly.

The preferred complete installer path is **NSIS plus a signed identity package with an external location**. This preserves the stable executable location and lets the same uninstaller disconnect owned browser hosts and unregister the exact owned identity before deleting application files. [Microsoft documents this identity model](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps). The optional full MSIX below is a separate distribution path.

Build the signed NSIS identity distribution from `rune` on Windows, using an existing trusted code-signing certificate:

```powershell
./scripts/build-identity.ps1 -Configuration release -Installer `
  -PackageName 'YOUR.PRODUCTION.IDENTITY' -Publisher 'CN=YOUR EXACT CERTIFICATE SUBJECT' `
  -PublisherDisplayName 'YOUR PUBLISHER' -Version '0.1.0.0' `
  -CertificateThumbprint 'YOUR EXISTING CERTIFICATE THUMBPRINT'
```

The wrapper creates a tiny signed `PickleIdentity.msix`, derives its expected family using Windows' identity API, and generates matching PE manifest metadata before building and signing the app. NSIS checks existing registrations before copying the payload, refuses a conflicting publisher/family/external location or downgrade, and registers with `Add-AppxPackage -ExternalLocation`. Registration failure stops installation before a new uninstall entry is written. Repeating the exact already registered version at the same location is a no-op. Upgrades must use an increasing four-part package version and the same identity/publisher. Do not switch an existing installation between identity-bearing and ordinary builds as an upgrade strategy.

During uninstall, browser cleanup runs while identity is available; the helper then removes only the current user's package with matching name, publisher, family and external path. A failed ownership check stops deletion. The installer never imports certificates or changes machine trust. Production machines must already trust the production certificate. A CI development certificate may be created/trusted only by an explicitly isolated runner test, with runner cleanup outside these product scripts. The pure helper fixtures (`scripts/test-identity-registration.ps1`) mock all deployment calls and do not change OS state. Native installation, repair, upgrade, rollback behavior, identity/OCR and final cleanup still require Windows validation.

`AppxManifest.xml` is the template for a full-trust desktop MSIX. `build-msix.ps1` requires an explicit package identity, publisher certificate subject, and publisher display name. It uses Windows SDK MakeAppx validation and discovers SDK tools even outside a developer shell. It copies only the staged payload allowlist and produces exact-size logos from the project's existing artwork. The script never creates a certificate, installs one, changes trust, installs the package, or launches the app.

Build and inspect an **unsigned CI artifact**, from `rune` on Windows:

```powershell
./scripts/build-windows.ps1 -Configuration debug
$stage = (Get-ChildItem artifacts/windows-* -Directory | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
./scripts/build-msix.ps1 -StageDirectory $stage -PackageName 'Pickle.Rune.CI' -Publisher 'CN=Pickle CI Development' -PublisherDisplayName 'Pickle CI Development' -Version '0.1.0.0' -UnsignedDevelopment
./scripts/test-msix-package.ps1 -Package artifacts/PickleRune-x64.msix -ExpectedIdentity 'Pickle.Rune.CI'
```

This inspection does not prove installation, runtime identity, OCR, microphone access, GPU behavior, or upgrade handling. An unsigned artifact is not a user release. [Microsoft's packaging guide](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-manual-conversion) describes package identity and certificate requirements.

For release, stage a release build, pass the production package identity and exact publisher, and omit `-UnsignedDevelopment`. Supply `-CertificateThumbprint` for an existing trusted signing certificate in the user's certificate store. The script signs and verifies the package. The same certificate may sign the EXEs in `build-windows.ps1` through `PICKLE_SIGN_CERT_THUMBPRINT`. Signing credentials and production identity are deliberately not committed. The four-part MSIX version is a separate explicit input and must increase for upgrades; keep it aligned with the app/release manifest version.

Release gates still require a clean Windows 11 machine: verify trust/install/upgrade/uninstall, packaged activation, identity and real OCR against a known image, selected text, capture, audio, shortcuts, and GPU rendering. The Windows build scopes static CRT compilation to an explicit target and audits ordinary and delayed PE imports; the standalone MSIX builder applies the same staged runtime audit. Native clean-machine loading remains required. Do not change `crt-static` globally without separately targeting application builds and checking proc-macro/build-tool compilation.

Native browser hosts are registered externally under the user's browser NativeMessagingHosts keys. Full MSIX removal does not run the NSIS uninstaller and no automatic full-MSIX removal hook is implemented. Before removing a full MSIX package, use Settings → Disconnect for each browser (or the exact installed app's `--disconnect-browsers` maintenance command while the app is closed). Full MSIX connections record the OS-derived AUMID, which is independent of version and executable location; the copied host activates it through `IApplicationActivationManager` with fixed background arguments, bounded waiting, and no stale-path fallback. Newer versions can clean up a connection belonging to the same exact OS identity. Sparse NSIS identity connections retain direct activation and exact ownership of their stable executable path. Legacy unpackaged and Mac registrations remain readable. Extension-origin checks and authenticated loopback relaying apply in each case. Actual full-MSIX activation across updates remains an OS test gate. User data and credentials are retained; package-managed app data behavior must also be checked during native installation testing.
