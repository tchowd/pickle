param([ValidateSet('debug','release')][string]$Configuration = 'debug', [switch]$Installer)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sdk.ps1')
Set-Location (Join-Path $PSScriptRoot '..')
$buildArgs = @('build', '--workspace', '--locked')
if ($Configuration -eq 'release') { $buildArgs += '--release' }
& cargo @buildArgs
if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
$targetLine = (& rustc -vV | Where-Object { $_ -like 'host:*' })
$buildTarget = $targetLine.Substring(6).Trim()
$noticeArgs = @('packaging/generate-notices.py', '--target', $buildTarget, '--output', 'artifacts/notices-windows')
if ($Configuration -eq 'release') { $noticeArgs += '--strict' }
& python @noticeArgs
if ($LASTEXITCODE -ne 0) { throw 'Dependency notice inventory is incomplete; resolve the reported gaps before release.' }
$stage = Join-Path $PWD ('artifacts/windows-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage -Force | Out-Null
Copy-Item "target/$Configuration/pickle-app.exe" (Join-Path $stage 'PickleRune.exe')
if (!(Test-Path "target/$Configuration/pickle-browser-host.exe")) { throw 'The self-contained native browser host is missing.' }
Copy-Item "target/$Configuration/pickle-browser-host.exe" $stage
Copy-Item '../browser-extension' (Join-Path $stage 'browser-extension') -Recurse
Copy-Item 'packaging/assets/Pickle.ico' $stage
Copy-Item 'packaging/THIRD-PARTY-NOTICES.md' $stage
Copy-Item 'artifacts/notices-windows/THIRD_PARTY_NOTICES.txt' $stage
Copy-Item 'artifacts/notices-windows/dependency-inventory.json' $stage
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
