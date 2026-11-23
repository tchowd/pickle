# Portable retention/environment contracts. Windows adds pure HWND predicate checks without invoking user32.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-startup-smoke.ps1')
function Assert-Refused([scriptblock]$Operation) {
  $refused=$false
  try { & $Operation } catch { $refused=$true }
  if (!$refused) { throw 'Expected the startup-smoke data validator to refuse a change.' }
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('pickle-startup-fixture-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $root 'BrowserBridge') -Force | Out-Null
try {
  $file = Join-Path $root 'history.json'
  [IO.File]::WriteAllText($file,'fixed owned history fixture')
  $before = Get-PickleRetainedState $root
  foreach ($name in @('instance.lock','instance.json','window.lock','BrowserBridge/connection.json')) { [IO.File]::WriteAllText((Join-Path $root $name),'transient fixture') }
  Assert-PickleRetainedState $before (Get-PickleRetainedState $root)
  [IO.File]::WriteAllText($file,'changed owned history fixture')
  Assert-Refused { Assert-PickleRetainedState $before (Get-PickleRetainedState $root) }
  [IO.File]::WriteAllText($file,'fixed owned history fixture')
  $extra = Join-Path $root 'unexpected-settings.json'
  [IO.File]::WriteAllText($extra,'unexpected')
  Assert-Refused { Assert-PickleRetainedState $before (Get-PickleRetainedState $root) }
  Remove-Item -LiteralPath $extra,$file
  Assert-Refused { Assert-PickleRetainedState $before (Get-PickleRetainedState $root) }
  [IO.File]::WriteAllText($file,'fixed owned history fixture')
  $alias = Join-Path $root 'unsafe-link'
  New-Item -ItemType SymbolicLink -Path $alias -Target $file | Out-Null
  try { Assert-Refused { Get-PickleRetainedState $root } } finally { Remove-Item -LiteralPath $alias }
  $large = Join-Path $root 'oversized-owned-fixture'
  $stream = [IO.File]::Create($large)
  try { $stream.SetLength(64MB+1) } finally { $stream.Dispose() }
  try { Assert-Refused { Get-PickleRetainedState $root } } finally { Remove-Item -LiteralPath $large }
  $many = Join-Path $root 'bounded-inventory'
  New-Item -ItemType Directory -Path $many | Out-Null
  try {
    foreach ($index in 1..513) { [IO.File]::WriteAllText((Join-Path $many "$index.txt"),'') }
    Assert-Refused { Get-PickleRetainedState $root }
  } finally { Remove-Item -LiteralPath $many -Recurse }
  $backend = [Environment]::GetEnvironmentVariable('RUNE_GPU_BACKEND','Process')
  $adapter = [Environment]::GetEnvironmentVariable('WGPU_ADAPTER_NAME','Process')
  $start = New-PickleFailureProcessStart $file
  $second = New-PickleFailureProcessStart $file
  if ($start.FileName -cne $file -or $start.Arguments -cne '--sample' -or $start.UseShellExecute -or $start.EnvironmentVariables['RUNE_GPU_BACKEND'] -cne 'wgpu' -or $start.EnvironmentVariables['WGPU_ADAPTER_NAME'] -notmatch '^Pickle-CI-intentionally-missing-adapter-[0-9a-f]{32}$' -or $start.EnvironmentVariables['WGPU_ADAPTER_NAME'] -ceq $second.EnvironmentVariables['WGPU_ADAPTER_NAME']) { throw 'The failure child must have its own exact executable/arguments and unique impossible adapter.' }
  if ($backend -cne [Environment]::GetEnvironmentVariable('RUNE_GPU_BACKEND','Process') -or $adapter -cne [Environment]::GetEnvironmentVariable('WGPU_ADAPTER_NAME','Process')) { throw 'Failure setup changed the parent environment.' }
  if ($IsWindows) {
    Initialize-PickleStartupDialogProbe
    if (![PickleStartupDialogProbe]::Eligible(12,12,$true,'Pickle could not open the reader','#32770')) { throw 'Expected the exact owned native dialog predicate to match.' }
    foreach ($case in @(
      @(13,12,$true,'Pickle could not open the reader','#32770'),
      @(12,12,$false,'Pickle could not open the reader','#32770'),
      @(12,12,$true,'Pickle could not open the reader.','#32770'),
      @(12,12,$true,'pickle could not open the reader','#32770'),
      @(12,12,$true,'Pickle could not open the reader','Window')
    )) { if ([PickleStartupDialogProbe]::Eligible($case[0],$case[1],$case[2],$case[3],$case[4])) { throw 'A non-owned, hidden or incorrect native window passed the startup dialog predicate.' } }
    if (![PickleStartupDialogProbe]::EligibleButton(12,12,$true,$true,1,'Button',$true)) { throw 'The exact owned enabled IDOK button must be accepted.' }
    foreach ($case in @(
      @(13,12,$true,$true,1,'Button',$true),
      @(12,12,$false,$true,1,'Button',$true),
      @(12,12,$true,$false,1,'Button',$true),
      @(12,12,$true,$true,2,'Button',$true),
      @(12,12,$true,$true,1,'Static',$true),
      @(12,12,$true,$true,1,'Button',$false)
    )) { if ([PickleStartupDialogProbe]::EligibleButton($case[0],$case[1],$case[2],$case[3],$case[4],$case[5],$case[6])) { throw 'A wrong-process, hidden, disabled or incorrect native control passed the IDOK predicate.' } }
  }
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
Write-Host 'Startup smoke fixtures passed: retained-data changes/links/size/count refused, transient descriptors excluded, child environment isolated; no app launched.'
