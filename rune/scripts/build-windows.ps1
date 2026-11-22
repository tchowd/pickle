param(
  [ValidateSet('debug','release')][string]$Configuration = 'debug',
  [switch]$Installer,
  [string]$IdentityPackage
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sdk.ps1')
Set-Location (Join-Path $PSScriptRoot '..')
$buildTarget = 'x86_64-pc-windows-msvc'
# Explicit --target is essential even on an x64 host: Cargo must keep host proc
# macros/build scripts separate from target crates compiled with a static CRT.
# An explicit output root also keeps staging correct if CARGO_TARGET_DIR is set.
$buildRoot = Join-Path $PWD 'target/windows-package'
$buildArgs = @('build', '--workspace', '--locked', '--target', $buildTarget, '--target-dir', $buildRoot)
if ($Configuration -eq 'release') { $buildArgs += '--release' }
$priorRustFlags = [Environment]::GetEnvironmentVariable('RUSTFLAGS', 'Process')
$priorEncodedFlags = [Environment]::GetEnvironmentVariable('CARGO_ENCODED_RUSTFLAGS', 'Process')
try {
  # Respect Cargo's encoded-flags precedence and preserve the caller's arguments.
  # When no environment flags are supplied, use Cargo's array merge so existing
  # build.rustflags arrays are retained instead of silently shadowing config.
  if ($null -ne $priorEncodedFlags) {
    $separator = [string][char]31
    $addition = '-C' + $separator + 'target-feature=+crt-static'
    $env:CARGO_ENCODED_RUSTFLAGS = if ($priorEncodedFlags.Length) { $priorEncodedFlags + $separator + $addition } else { $addition }
  } elseif ($null -ne $priorRustFlags) {
    $env:RUSTFLAGS = $priorRustFlags + ' -C target-feature=+crt-static'
  } else {
    $buildArgs += @('--config', 'build.rustflags=["-C","target-feature=+crt-static"]')
  }
  & cargo @buildArgs
  if ($LASTEXITCODE -ne 0) { throw 'Static-CRT Rust build failed. If Cargo rustflags use a string or target-specific config, supply all intended flags through RUSTFLAGS or CARGO_ENCODED_RUSTFLAGS and retry.' }
} finally {
  [Environment]::SetEnvironmentVariable('RUSTFLAGS', $priorRustFlags, 'Process')
  [Environment]::SetEnvironmentVariable('CARGO_ENCODED_RUSTFLAGS', $priorEncodedFlags, 'Process')
}
$noticeArgs = @('packaging/generate-notices.py', '--target', $buildTarget, '--output', 'artifacts/notices-windows')
if ($Configuration -eq 'release') { $noticeArgs += '--strict' }
& python @noticeArgs
if ($LASTEXITCODE -ne 0) { throw 'Dependency notice inventory is incomplete; resolve the reported gaps before release.' }
$stage = Join-Path $PWD ('artifacts/windows-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage -Force | Out-Null
$binaries = Join-Path $buildRoot "$buildTarget/$Configuration"
Copy-Item (Join-Path $binaries 'pickle-app.exe') (Join-Path $stage 'PickleRune.exe')
if (!(Test-Path (Join-Path $binaries 'pickle-browser-host.exe'))) { throw 'The self-contained native browser host is missing.' }
Copy-Item (Join-Path $binaries 'pickle-browser-host.exe') $stage
Copy-Item '../browser-extension' (Join-Path $stage 'browser-extension') -Recurse
Copy-Item 'packaging/assets/Pickle.ico' $stage
Copy-Item 'packaging/THIRD-PARTY-NOTICES.md' $stage
Copy-Item 'artifacts/notices-windows/THIRD_PARTY_NOTICES.txt' $stage
Copy-Item 'artifacts/notices-windows/dependency-inventory.json' $stage
if ($IdentityPackage) {
  $identitySource = (Resolve-Path -LiteralPath $IdentityPackage).Path
  $identityMetadata = Join-Path (Split-Path $identitySource -Parent) 'PickleIdentity.json'
  if (!(Test-Path -LiteralPath $identitySource -PathType Leaf) -or !(Test-Path -LiteralPath $identityMetadata -PathType Leaf)) { throw 'Identity packaging requires an existing package file and sibling PickleIdentity.json.' }
  Copy-Item -LiteralPath $identitySource -Destination (Join-Path $stage 'PickleIdentity.msix')
  Copy-Item -LiteralPath $identityMetadata -Destination (Join-Path $stage 'PickleIdentity.json')
  Copy-Item -LiteralPath 'packaging/windows/identity-registration.ps1' -Destination $stage
  Copy-Item -LiteralPath 'packaging/assets/Pickle-preview.png' -Destination $stage
}
# Check actual loader dependencies, not just compiler flags. A foreign library or
# target-specific Cargo override must not quietly add a runtime prerequisite.
& python packaging/test_windows_runtime.py
if ($LASTEXITCODE -ne 0) { throw 'Windows runtime verification fixtures failed.' }
& python packaging/verify-windows-runtime.py --directory $stage --architecture x64 --output artifacts/windows-runtime-report.json
if ($LASTEXITCODE -ne 0) { throw 'Packaged runtime policy failed. Build every application/native dependency with a static CRT; preserve custom flags through RUSTFLAGS or CARGO_ENCODED_RUSTFLAGS.' }
if ($env:PICKLE_SIGN_CERT_THUMBPRINT) {
  if ($env:PICKLE_SIGN_CERT_THUMBPRINT -notmatch '^[0-9a-fA-F]{40}$') { throw 'The signing certificate thumbprint must contain 40 hexadecimal characters.' }
  $signtool = Find-WindowsSdkTool 'signtool.exe'
  foreach ($binary in Get-ChildItem $stage -Filter '*.exe') {
    & $signtool sign /sha1 $env:PICKLE_SIGN_CERT_THUMBPRINT /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $binary.FullName
    if ($LASTEXITCODE -ne 0) { throw 'Windows code signing failed.' }
  }
}
Compress-Archive -Path "$stage/*" -DestinationPath (Join-Path $PWD 'artifacts/PickleRune-windows.zip') -Force
if ($Installer) {
  $makensis = (Get-Command makensis.exe -ErrorAction SilentlyContinue).Source
  if (!$makensis) { $makensis = "${env:ProgramFiles(x86)}/NSIS/makensis.exe" }
  if (!(Test-Path $makensis)) { throw 'NSIS is required to produce the installer. Install NSIS and retry.' }
  $metadata = (& cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json)
  if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the application version.' }
  $appVersion = ($metadata.packages | Where-Object { $_.name -eq 'pickle-app' }).version
  if (!$appVersion) { throw 'Could not determine the installer version.' }
  $nsisArgs = @("/DPICKLE_STAGE=$stage", "/DPICKLE_OUTPUT=$PWD/artifacts/PickleRune-setup.exe", "/DPICKLE_VERSION=$appVersion")
  if ($env:PICKLE_SIGN_CERT_THUMBPRINT) { $nsisArgs += @("/DPICKLE_SIGN_CERT_THUMBPRINT=$env:PICKLE_SIGN_CERT_THUMBPRINT", "/DPICKLE_SIGNTOOL=$signtool") }
  $nsisArgs += 'packaging/windows/PickleRune.nsi'
  & $makensis @nsisArgs
  if ($LASTEXITCODE -ne 0) { throw 'Windows installer build failed.' }
  if ($env:PICKLE_SIGN_CERT_THUMBPRINT) {
    & $signtool sign /sha1 $env:PICKLE_SIGN_CERT_THUMBPRINT /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 'artifacts/PickleRune-setup.exe'
    if ($LASTEXITCODE -ne 0) { throw 'Installer signing failed.' }
  }
}
Write-Host "Windows staging directory: $stage"
Write-Host 'Unsigned artifacts are development builds. Validate a clean Windows installation before release.'
