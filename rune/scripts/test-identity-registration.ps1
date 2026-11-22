# Pure mocked deployment fixtures; never registers packages or changes trust.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../packaging/windows/identity-registration.ps1')
$root = Join-Path ([IO.Path]::GetTempPath()) ('pickle-identity-fixture-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $root | Out-Null
$packagePath = Join-Path $root 'PickleIdentity.msix'
[IO.File]::WriteAllText($packagePath,'fixture package bytes')
$metadata = @{schema=1;name='Pickle.Rune.Test';publisher='CN=Pickle Test';family='Pickle.Rune.Test_123456789abcd';version='0.1.0.0';application='Pickle';sha256=(Get-FileHash -Algorithm SHA256 $packagePath).Hash.ToLowerInvariant()}
$metadataPath = Join-Path $root 'PickleIdentity.json'
$metadata | ConvertTo-Json | Set-Content $metadataPath
$script:packages = @()
$script:removed = @()
$script:addCount = 0
function Get-AppxPackage { param($Name) return $script:packages }
function Get-IdentityExternalPath($Package) { return $Package.ExternalPath }
function Remove-AppxPackage { param($Package) $script:removed += $Package; $script:packages = @($script:packages | Where-Object { $_.PackageFullName -ne $Package }) }
function Add-AppxPackage {
  param($Path,$ExternalLocation)
  $script:addCount++
  $script:packages = @([pscustomobject]@{Name=$metadata.name;Publisher=$metadata.publisher;PackageFamilyName=$metadata.family;PackageFullName='fixture-full-name';Version=$metadata.version;ExternalPath=$ExternalLocation})
}
function Assert-Refused([scriptblock]$Work) {
  $refused = $false
  try { & $Work } catch { $refused = $true }
  if (!$refused) { throw 'Expected a safe refusal.' }
}
Invoke-PickleIdentity Check $root $metadataPath
Invoke-PickleIdentity Register $root $metadataPath
if ($script:addCount -ne 1) { throw 'Registration was not requested.' }
Invoke-PickleIdentity Register $root $metadataPath
if ($script:addCount -ne 1) { throw 'Identical owned registration should be idempotent.' }
$owned = $script:packages[0]
foreach ($field in @('Publisher','PackageFamilyName','ExternalPath')) {
  $copy = $owned.PSObject.Copy()
  $copy.$field = 'different ownership'
  $script:packages = @($copy)
  Assert-Refused { Invoke-PickleIdentity Unregister $root $metadataPath }
  if ($script:removed.Count) { throw 'Unrelated registration was removed.' }
}
$script:packages = @($owned.PSObject.Copy())
$script:packages[0].Version = '0.2.0.0'
Assert-Refused { Invoke-PickleIdentity Check $root $metadataPath }
$script:packages = @()
[IO.File]::WriteAllText($packagePath,'tampered package')
Assert-Refused { Invoke-PickleIdentity Register $root $metadataPath }
if ($script:addCount -ne 1) { throw 'Tampered package reached deployment.' }
$script:packages = @($owned)
Invoke-PickleIdentity Unregister $root $metadataPath
if ($script:removed.Count -ne 1 -or $script:removed[0] -ne 'fixture-full-name') { throw 'Exact owned identity was not removed.' }
Invoke-PickleIdentity Unregister $root $metadataPath
if ($script:removed.Count -ne 1) { throw 'Missing identity must be a no-op.' }
Write-Host 'Identity fixtures passed: origin metadata, publisher/family/path ownership, idempotency, downgrade, checksum, exact removal. No OS deployment or trust calls.'
Remove-Item -LiteralPath $metadataPath,$packagePath
Remove-Item -LiteralPath $root
