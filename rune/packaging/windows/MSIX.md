# Windows identity and release packaging

The ordinary ZIP and NSIS build is an unpackaged development path. It supports selected text and other available desktop integration, but it does not establish the package identity required by [Windows.Media.Ocr](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr?view=winrt-26100). Full OCR validation therefore requires an installed MSIX build on Windows 11. App capability checks report unavailable identity honestly.

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

Release gates still require a clean Windows 11 machine: verify trust/install/upgrade/uninstall, packaged activation, identity and real OCR against a known image, selected text, capture, audio, shortcuts, and GPU rendering. MSVC dynamic CRT availability is unverified; the package currently does not bundle VCRedist or declare a VCLibs framework dependency. Audit actual binary dependencies and supply the supported runtime before release. Do not change `crt-static` globally without separately targeting application builds and checking proc-macro/build-tool compilation.

Native browser hosts are currently registered externally under the user's browser NativeMessagingHosts keys. MSIX removal does not run the NSIS uninstaller and no automatic MSIX removal hook is implemented. Before removing the package, use Settings → Disconnect for each browser (or the exact installed app's `--disconnect-browsers` maintenance command while the app is closed). After an MSIX update changes its versioned WindowsApps path, reconnect the browser integration; stable activation across MSIX updates is a remaining release gate. The cleanup command checks exact installation ownership and preserves registrations owned by another version or installation. User data and credentials are retained; package-managed app data behavior must also be checked during native installation testing.
