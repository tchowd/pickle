param([Parameter(Mandatory)][string]$Package, [Parameter(Mandatory)][string]$ExpectedIdentity)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sdk.ps1')
$output = Join-Path ([IO.Path]::GetTempPath()) ('pickle-msix-inspection-' + [guid]::NewGuid().ToString('N'))
$makeappx = Find-WindowsSdkTool 'makeappx.exe'
& $makeappx unpack /p (Resolve-Path -LiteralPath $Package).Path /d $output
if ($LASTEXITCODE -ne 0) { throw 'MSIX unpack validation failed.' }
[xml]$manifest = Get-Content -Raw (Join-Path $output 'AppxManifest.xml')
if ($manifest.Package.Identity.Name -ne $ExpectedIdentity) { throw 'MSIX identity mismatch.' }
if ($manifest.Package.Applications.Application.Executable -ne 'PickleRune.exe') { throw 'MSIX app executable mismatch.' }
foreach ($name in @('PickleRune.exe','pickle-browser-host.exe','browser-extension/manifest.json','THIRD_PARTY_NOTICES.txt')) {
  if (!(Test-Path -LiteralPath (Join-Path $output $name))) { throw "MSIX missing $name." }
}
if (Test-Path (Join-Path $output 'Uninstall.exe')) { throw 'MSIX must not contain an NSIS uninstaller.' }
Add-Type -AssemblyName System.Drawing
foreach ($logo in @(@('StoreLogo.png',50), @('Square44x44Logo.png',44), @('Square150x150Logo.png',150))) {
  $image = [System.Drawing.Image]::FromFile((Join-Path $output ('Assets/' + $logo[0])))
  try { if ($image.Width -ne $logo[1] -or $image.Height -ne $logo[1]) { throw 'MSIX logo dimensions are invalid.' } } finally { $image.Dispose() }
}
Write-Host "MSIX structure passed. Inspection retained at $output. No package or certificates were installed."
