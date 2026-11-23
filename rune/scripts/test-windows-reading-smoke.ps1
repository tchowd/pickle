# Portable profile/HTTP fixtures; Windows additionally compiles/runs the non-UI self-test.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-reading-smoke.ps1')
function Assert-ReadingRefused([scriptblock]$Operation) {
  $refused=$false;try { & $Operation } catch { $refused=$true }
  if (!$refused) { throw 'Expected reading-fixture safety refusal.' }
}
$python=Get-Command python -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if (!$python) { $python=Get-Command python3 -CommandType Application -ErrorAction Stop | Select-Object -First 1 }
& $python.Source (Join-Path $PSScriptRoot 'test_windows_reading_provider.py')
if ($LASTEXITCODE -ne 0) { throw 'Fixed provider HTTP fixtures failed.' }
$root=Join-Path ([IO.Path]::GetTempPath()) ('pickle-reading-tests-'+[guid]::NewGuid().ToString('N'))
if ($root.StartsWith('/var/') -and (Test-Path -LiteralPath '/private/var')) { $root='/private'+$root }
New-Item -ItemType Directory -Path $root | Out-Null
try {
  if ((ConvertTo-PickleReadingRelative ('BrowserBridge'+[char]92+'connection.json')) -cne 'BrowserBridge/connection.json') { throw 'Windows profile separators must normalize before control exclusions.' }
  $data=Join-Path $root 'data';$backup=Join-Path $root 'backup'
  New-Item -ItemType Directory -Path (Join-Path $data 'BrowserBridge') -Force | Out-Null
  $settings=Join-Path $data 'settings.json';$history=Join-Path $data 'history.json'
  $original=[Text.UTF8Encoding]::new($true).GetBytes('Original Café profile bytes.')
  [IO.File]::WriteAllBytes($settings,$original)
  [IO.File]::WriteAllText($history,'Fixed owned history bytes.')
  foreach ($name in @('instance.lock','instance.json','window.lock','store.lock','BrowserBridge/connection.json')) { [IO.File]::WriteAllText((Join-Path $data $name),'transient owned fixture') }
  $snapshot=Save-PickleReadingProfile $data $backup
  if ($snapshot.Hashes.Count -ne 2) { throw 'Transient synchronization records entered the private profile backup.' }
  [IO.File]::WriteAllText($settings,'Temporary synthetic configuration.')
  [IO.File]::WriteAllText((Join-Path $data 'window.json'),'Test window geometry.')
  Assert-ReadingRefused { Restore-PickleReadingProfile $snapshot { $false } }
  if ([IO.File]::ReadAllText($settings) -cne 'Temporary synthetic configuration.') { throw 'A live-lease restoration guard mutated profile bytes.' }
  Restore-PickleReadingProfile $snapshot { $true }
  if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($settings)) -cne [Convert]::ToBase64String($original) -or (Test-Path -LiteralPath (Join-Path $data 'window.json')) -or !(Test-Path -LiteralPath (Join-Path $data 'instance.lock'))) { throw 'Restoration failed to preserve original bytes or synchronization inodes.' }
  $unknown=Join-Path $data 'unrelated-new-file.txt'
  [IO.File]::WriteAllText($unknown,'Another actor must be preserved.')
  [IO.File]::WriteAllText($settings,'Temporary synthetic configuration.')
  Assert-ReadingRefused { Restore-PickleReadingProfile $snapshot { $true } }
  if (!(Test-Path -LiteralPath $unknown) -or [IO.File]::ReadAllText($settings) -cne 'Temporary synthetic configuration.') { throw 'Unknown-file preflight partially mutated a profile.' }
  Remove-Item -LiteralPath $unknown
  [IO.File]::WriteAllText((Join-Path $backup 'history.json'),'Changed private backup fixture.')
  Assert-ReadingRefused { Restore-PickleReadingProfile $snapshot { $true } }
  if ([IO.File]::ReadAllText($settings) -cne 'Temporary synthetic configuration.') { throw 'Backup hash refusal partially restored profile bytes.' }
  $link=Join-Path $root 'alias'
  New-Item -ItemType SymbolicLink -Path $link -Target $data | Out-Null
  try { Assert-ReadingRefused { Get-PickleReadingFiles $link } } finally { Remove-Item -LiteralPath $link }
  Assert-ReadingRefused { Save-PickleReadingProfile $data (Join-Path $data 'backup-inside-profile') }
  $large=Join-Path $data 'oversized-fixture'
  $stream=[IO.File]::Create($large)
  try { $stream.SetLength(64MB+1) } finally { $stream.Dispose() }
  try { Assert-ReadingRefused { Get-PickleReadingFiles $data };Assert-ReadingRefused { Read-PickleReadingBytes $large 64MB } } finally { Remove-Item -LiteralPath $large }
  $many=Join-Path $data 'many';New-Item -ItemType Directory -Path $many | Out-Null
  try { foreach ($n in 1..513) { [IO.File]::WriteAllText((Join-Path $many "$n.txt"),'') };Assert-ReadingRefused { Get-PickleReadingFiles $data } }
  finally { Remove-Item -LiteralPath $many -Recurse }
  if ($IsWindows) {
    $executable=Build-PickleReadingFixture $root
    $selfTest=Start-PickleReadingChild $executable @('--self-test')
    try { if (!$selfTest.WaitForExit(10000) -or $selfTest.ExitCode -ne 0) { throw 'Owned native fixture non-UI predicates failed.' } }
    finally { if (!$selfTest.HasExited) { $selfTest.Kill();$selfTest.WaitForExit(10000) | Out-Null };$selfTest.Dispose() }
  } else {
    Assert-ReadingRefused { Assert-PickleInstalledReadingWorkflow '/absent-app' '/absent-profile' '/absent-evidence' 'debug' }
  }
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
Write-Host 'Reading fixtures passed: bounded HTTP/source contract, guarded byte-exact restoration, tamper/link/count/size refusals; no native app or GUI launched.'
