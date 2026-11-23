# Portable public integrity/identity validators only: no signing, SDK, certificate, or staged executable runs.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-release-common.ps1')
$root=Join-Path ([IO.Path]::GetTempPath()) ('pickle-release-fixture-'+[guid]::NewGuid().ToString('N'))
# macOS exposes its normal temporary directory through /var -> /private/var.
# Use the physical fixture location so the production no-junction policy remains strict.
if ($root.StartsWith('/var/') -and (Test-Path -LiteralPath '/private/var')) { $root='/private'+$root }
New-Item -ItemType Directory -Path (Join-Path $root 'browser-extension') -Force | Out-Null
$required=@('PickleRune.exe','pickle-browser-host.exe','PickleIdentity.msix','PickleIdentity.json','identity-registration.ps1','Pickle.ico','Pickle-preview.png','THIRD-PARTY-NOTICES.md','THIRD_PARTY_NOTICES.txt','dependency-inventory.json','browser-extension/manifest.json')
foreach ($name in $required) { [IO.File]::WriteAllText((Join-Path $root $name),'public inert fixture '+$name) }
$commit='a'*40
$receipt=[pscustomobject]@{schema=1;source_commit=$commit;app_version='0.1.0';target='x86_64-pc-windows-msvc';identity=[pscustomobject]@{name='Pickle.Release.Test';publisher='CN=Pickle Test';family='Pickle.Release.Test_123456789abcd';version='0.1.0.0';application='Pickle'};files=@(Get-ReleaseInventory $root)}
function Assert-Refused([scriptblock]$Operation) { $refused=$false;try { & $Operation } catch { $refused=$true };if (!$refused) { throw 'Expected release validation refusal.' } }
Assert-PreparedRelease $root $receipt $commit '0.1.0' 'Pickle.Release.Test' 'CN=Pickle Test' '0.1.0.0'
Assert-Refused { Assert-PreparedRelease $root $receipt ('b'*40) '0.1.0' 'Pickle.Release.Test' 'CN=Pickle Test' '0.1.0.0' }
Assert-Refused { Assert-PreparedRelease $root $receipt $commit '0.2.0' 'Pickle.Release.Test' 'CN=Pickle Test' '0.1.0.0' }
Assert-Refused { Assert-PreparedRelease $root $receipt $commit '0.1.0' 'Other.Package' 'CN=Pickle Test' '0.1.0.0' }
Assert-Refused { Assert-PreparedRelease $root $receipt $commit '0.1.0' 'Pickle.Release.Test' 'CN=Other Publisher' '0.1.0.0' }
foreach ($path in @('../outside','browser-extension/../outside','/absolute','C:/absolute','browser-extension//file')) { Assert-Refused { Assert-ReleasePath $root $path } }
$alias=$root+'-alias'
New-Item -ItemType SymbolicLink -Path $alias -Target $root | Out-Null
try {
  Assert-Refused { Assert-ReleaseAncestors (Join-Path $alias 'browser-extension/manifest.json') }
  Assert-Refused { Assert-ReleaseAncestors (Join-Path $alias 'new-final-output') -MayBeMissing }
  Assert-Refused { Get-ReleaseInventory (Join-Path $alias 'browser-extension') }
} finally { Remove-Item -LiteralPath $alias -Force }
Assert-ReleaseArtifactSize 1
Assert-ReleaseArtifactSize 512MB
Assert-Refused { Assert-ReleaseArtifactSize 0 }
Assert-Refused { Assert-ReleaseArtifactSize (512MB+1) }
$promotion=Join-Path $root 'promotion';New-Item -ItemType Directory -Path $promotion | Out-Null
$work=Join-Path $promotion '.owned-work';New-Item -ItemType Directory -Path $work | Out-Null
[IO.File]::WriteAllText((Join-Path $work 'PickleRune-setup.exe'),'inert signed candidate')
[IO.File]::WriteAllText((Join-Path $work 'release-report.json'),'public report')
$inputHash=Get-ReleaseSha256 (Join-Path $root 'PickleRune.exe')
Assert-Refused { Publish-ReleaseFiles $work $promotion @('PickleRune-setup.exe','release-report.json') { param($source,$target) [IO.File]::Move($source,$target);if ($target.EndsWith('release-report.json')) { throw 'Injected promotion failure after creating destination' } } }
if ((Test-Path -LiteralPath $work) -or (Test-Path -LiteralPath (Join-Path $promotion 'PickleRune-setup.exe')) -or (Test-Path -LiteralPath (Join-Path $promotion 'release-report.json'))) { throw 'Failed promotion left candidate-looking output.' }
if ((Get-ReleaseSha256 (Join-Path $root 'PickleRune.exe')) -cne $inputHash) { throw 'Failed promotion changed prepared input.' }
New-Item -ItemType Directory -Path $work | Out-Null
[IO.File]::WriteAllText((Join-Path $work 'PickleRune-setup.exe'),'inert signed candidate')
[IO.File]::WriteAllText((Join-Path $work 'release-report.json'),'public report')
Publish-ReleaseFiles $work $promotion @('PickleRune-setup.exe','release-report.json')
if (!(Test-Path -LiteralPath (Join-Path $promotion 'PickleRune-setup.exe')) -or !(Test-Path -LiteralPath (Join-Path $promotion 'release-report.json')) -or (Test-Path -LiteralPath $work)) { throw 'Successful promotion did not publish the exact gated output.' }
if ((Get-ReleaseSha256 (Join-Path $root 'PickleRune.exe')) -cne $inputHash) { throw 'Successful promotion changed prepared input.' }
Remove-Item -LiteralPath $promotion -Recurse
[IO.File]::AppendAllText((Join-Path $root 'PickleRune.exe'),'tampered')
Assert-Refused { Assert-PreparedRelease $root $receipt $commit '0.1.0' 'Pickle.Release.Test' 'CN=Pickle Test' '0.1.0.0' }
$receipt.files=@(Get-ReleaseInventory $root)
[IO.File]::WriteAllText((Join-Path $root 'unexpected.exe'),'extra')
Assert-Refused { Assert-PreparedRelease $root $receipt $commit '0.1.0' 'Pickle.Release.Test' 'CN=Pickle Test' '0.1.0.0' }
$receipt.files=@(Get-ReleaseInventory $root)
Assert-Refused { Assert-PreparedRelease $root $receipt $commit '0.1.0' 'Pickle.Release.Test' 'CN=Pickle Test' '0.1.0.0' }
foreach ($file in Get-ChildItem $root -Recurse -File) { Remove-Item -LiteralPath $file.FullName }
Remove-Item -LiteralPath (Join-Path $root 'browser-extension'),$root
Write-Host 'Release fixtures passed: identity/integrity, traversal/tampering/allowlist, size bounds, promotion rollback and preserved input. No signing/SDK/certificate operations.'
