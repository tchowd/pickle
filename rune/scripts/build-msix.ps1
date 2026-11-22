param(
  [Parameter(Mandatory)][string]$StageDirectory,
  [string]$PackageName = $env:PICKLE_MSIX_IDENTITY,
  [string]$Publisher = $env:PICKLE_MSIX_PUBLISHER,
  [string]$PublisherDisplayName = $env:PICKLE_MSIX_PUBLISHER_DISPLAY_NAME,
  [string]$Version = '0.1.0.0',
  [ValidateSet('x64','arm64')][string]$Architecture = 'x64',
  [string]$CertificateThumbprint = $env:PICKLE_SIGN_CERT_THUMBPRINT,
  [switch]$UnsignedDevelopment
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sdk.ps1')
if (!$IsWindows) { throw 'MSIX packaging requires Windows and the Windows SDK.' }
if ($PackageName -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]{1,48}[A-Za-z0-9]$') { throw 'Supply an explicit 3-50 character MSIX package identity.' }
if (!$Publisher -or $Publisher -notmatch '^CN=' -or !$PublisherDisplayName) { throw 'Supply the exact certificate subject publisher and a publisher display name.' }
if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$' -or @($Version.Split('.') | Where-Object { [uint64]$_ -gt 65535 }).Count) { throw 'MSIX version requires four integers from 0 to 65535.' }
if ($UnsignedDevelopment -and $CertificateThumbprint) { throw 'Choose either explicit unsigned development or signing.' }
if (!$UnsignedDevelopment -and $CertificateThumbprint -notmatch '^[a-fA-F0-9]{40}$') { throw 'Release MSIX requires an existing signing certificate thumbprint; use -UnsignedDevelopment only for CI inspection.' }
$stage = (Resolve-Path -LiteralPath $StageDirectory).Path
foreach ($name in @('PickleRune.exe','pickle-browser-host.exe','Pickle.ico','THIRD_PARTY_NOTICES.txt','dependency-inventory.json','browser-extension')) {
  if (!(Test-Path -LiteralPath (Join-Path $stage $name))) { throw "Windows staging payload is missing $name." }
}
# An identity with the wrong architecture can pack successfully yet fail activation.
foreach ($name in @('PickleRune.exe','pickle-browser-host.exe')) {
  $stream = [IO.File]::OpenRead((Join-Path $stage $name))
  $reader = [IO.BinaryReader]::new($stream)
  try {
    if ($reader.ReadUInt16() -ne 0x5a4d) { throw "Invalid PE executable: $name" }
    $stream.Position = 0x3c
    $offset = $reader.ReadUInt32()
    if ($offset -gt $stream.Length - 6) { throw "Invalid PE header: $name" }
    $stream.Position = $offset
    if ($reader.ReadUInt32() -ne 0x00004550) { throw "Invalid PE signature: $name" }
    $machine = $reader.ReadUInt16()
    $expected = if ($Architecture -eq 'x64') { 0x8664 } else { 0xaa64 }
    if ($machine -ne $expected) { throw "Staged $name does not match MSIX architecture $Architecture." }
  } finally { $reader.Dispose(); $stream.Dispose() }
}
# Never package an NSIS uninstaller or unknown files from a pre-existing installation.
$root = Split-Path $PSScriptRoot -Parent
$artifacts = Join-Path $root 'artifacts'
$payload = Join-Path $artifacts ('msix-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $payload 'Assets') -Force | Out-Null
foreach ($name in @('PickleRune.exe','pickle-browser-host.exe','Pickle.ico','THIRD-PARTY-NOTICES.md','THIRD_PARTY_NOTICES.txt','dependency-inventory.json','browser-extension')) {
  Copy-Item -LiteralPath (Join-Path $stage $name) -Destination $payload -Recurse
}
# Unqualified, exact-size logos avoid a MakePRI/resource indexing dependency.
Add-Type -AssemblyName System.Drawing
$source = [System.Drawing.Image]::FromFile((Join-Path $root 'packaging/assets/Pickle-preview.png'))
try {
  foreach ($logo in @(@('StoreLogo.png',50), @('Square44x44Logo.png',44), @('Square150x150Logo.png',150))) {
    $bitmap = [System.Drawing.Bitmap]::new([int]$logo[1], [int]$logo[1])
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
      $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
      $graphics.DrawImage($source,0,0,[int]$logo[1],[int]$logo[1])
      $bitmap.Save((Join-Path $payload ('Assets/' + $logo[0])), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
  }
} finally { $source.Dispose() }
[xml]$manifest = Get-Content -Raw (Join-Path $root 'packaging/windows/AppxManifest.xml')
$manifest.Package.Identity.SetAttribute('Name',$PackageName)
$manifest.Package.Identity.SetAttribute('Publisher',$Publisher)
$manifest.Package.Identity.SetAttribute('Version',$Version)
$manifest.Package.Identity.SetAttribute('ProcessorArchitecture',$Architecture)
$manifest.Package.Properties.PublisherDisplayName = $PublisherDisplayName
$manifest.Save((Join-Path $payload 'AppxManifest.xml'))
$output = Join-Path $artifacts ("PickleRune-$Architecture.msix")
$makeappx = Find-WindowsSdkTool 'makeappx.exe'
& $makeappx pack /d $payload /p $output /o
if ($LASTEXITCODE -ne 0) { throw 'Windows SDK rejected the MSIX payload or manifest.' }
if (!$UnsignedDevelopment) {
  $signtool = Find-WindowsSdkTool 'signtool.exe'
  & $signtool sign /sha1 $CertificateThumbprint /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $output
  if ($LASTEXITCODE -ne 0) { throw 'MSIX signing failed. Check that Publisher exactly matches the certificate subject.' }
  & $signtool verify /pa /v $output
  if ($LASTEXITCODE -ne 0) { throw 'MSIX signature verification failed.' }
}
Write-Host "MSIX artifact: $output"
if ($UnsignedDevelopment) { Write-Host 'UNSIGNED DEVELOPMENT ONLY: SDK inspection artifact; this script does not install or trust certificates.' }
