# Portable invariants only; Windows additionally compiles/runs --self-test,
# which opens no browser/window. Actual native automation has a separate entry.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-extension-smoke.ps1')
function Assert-ExtensionRefused([scriptblock]$Operation) {
  $refused=$false;try { & $Operation } catch { $refused=$true }
  if (!$refused) { throw 'Expected extension-fixture safety refusal.' }
}
foreach ($name in @('windows-extension-smoke.ps1','invoke-windows-extension-smoke.ps1','test-windows-extension-smoke.ps1')) {
  $tokens=$null;$errors=$null
  $null=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $name),[ref]$tokens,[ref]$errors)
  if ($errors.Count) { throw 'An extension acceptance script did not parse.' }
}
$registryPaths=@(Get-PickleExtensionRegistryPaths)
$expectedPaths=@('SOFTWARE\Google\Chrome\NativeMessagingHosts\com.pickle.reader','SOFTWARE\Microsoft\Edge\NativeMessagingHosts\com.pickle.reader','SOFTWARE\Chromium\NativeMessagingHosts\com.pickle.reader')
if ($registryPaths.Count -ne 3 -or @($registryPaths | Where-Object { $_ -cnotin $expectedPaths }).Count) { throw 'Only exact Pickle registrations may block this setup stage; unrelated hosts are preserved.' }
foreach ($case in @(@('debug',$false,'Core',$true,'true','github-hosted',$true),@('release',$false,'Core',$true,'true','github-hosted',$false),@('debug',$true,'Core',$true,'true','github-hosted',$false),@('debug',$false,'Desktop',$true,'true','github-hosted',$false),@('debug',$false,'Core',$false,'true','github-hosted',$false),@('debug',$false,'Core',$true,'false','github-hosted',$false),@('debug',$false,'Core',$true,'true','self-hosted',$false))) {
  if ((Test-PickleExtensionSetupGate $case[0] $case[1] $case[2] $case[3] $case[4] $case[5]) -ne $case[6]) { throw 'The package setup gate broadened beyond unsigned debug hosted Windows Core.' }
}
# Execute the actual package foreach against a non-UI delegate. This verifies
# both sequential calls and the exact installed directory/evidence arguments.
$packageTokens=$null;$packageErrors=$null
$packageAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'test-windows-package.ps1'),[ref]$packageTokens,[ref]$packageErrors)
if ($packageErrors.Count) { throw 'The package script did not parse.' }
$calls=@($packageAst.FindAll({param($n) $n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -ceq 'Invoke-PickleExtensionCore'},$true))
if ($calls.Count -ne 1) { throw 'The installed package lost its extension setup delegation.' }
$loop=$calls[0].Parent
while ($null -ne $loop -and $loop -isnot [Management.Automation.Language.ForEachStatementAst]) { $loop=$loop.Parent }
if ($null -eq $loop) { throw 'The package must run each browser through a separate delegate.' }
$following=@($loop.Parent.Statements | Where-Object { $_.Extent.StartOffset -gt $loop.Extent.EndOffset } | Select-Object -First 1)
if ($following.Count -ne 1 -or $following[0] -isnot [Management.Automation.Language.IfStatementAst]) { throw 'The package must fail after collecting safe setup failures.' }
$actualPair=[scriptblock]::Create($loop.Extent.Text+"`n"+$following[0].Extent.Text)
foreach ($scenario in @('both-pass','chrome-failed-cleanly','chrome-cleanup-unknown')) {
  & {
    param($ActualPair,[string]$Scenario)
    $testDirectory='owned-installed-package';$observed=[Collections.Generic.List[string]]::new();$extensionFailures=[Collections.Generic.List[string]]::new()
    function Invoke-PickleExtensionCore([string]$PackageDirectory,[string]$Browser,[string]$EvidencePath) {
      if ($PackageDirectory -cne 'owned-installed-package' -or $EvidencePath -cne (Join-Path $PWD "artifacts/windows-extension-$Browser-native-smoke.json")) { throw 'The actual package setup call lost installed-payload ownership.' }
      $observed.Add($Browser)
      if ($Browser -ceq 'chrome' -and $Scenario -ceq 'chrome-cleanup-unknown') { throw 'owned cleanup unknown' }
      return !($Browser -ceq 'chrome' -and $Scenario -ceq 'chrome-failed-cleanly')
    }
    $failed=$false;try { & $ActualPair } catch { $failed=$true }
    $expected=if ($Scenario -ceq 'chrome-cleanup-unknown') { 'chrome' } else { 'chrome,edge' }
    if (($observed -join ',') -cne $expected -or $failed -ne ($Scenario -cne 'both-pass')) { throw 'The actual package did not collect safe failures, stop on uncertain cleanup, or fail after a rejected setup.' }
  } $actualPair $scenario
}
$python=Get-Command python -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if (!$python) { $python=Get-Command python3 -CommandType Application -ErrorAction Stop | Select-Object -First 1 }
& $python.Source (Join-Path $PSScriptRoot 'test_windows_extension_page_fixture.py')
if ($LASTEXITCODE -ne 0) { throw 'The non-forwarding owned proxy contract failed.' }
$valid=[ordered]@{schema=1;test='windows-stable-bundled-extension-load';scope='bundled-extension-load-only';browser='chrome';extension_id='abcdefghijklmnopabcdefghijklmnop';extension_version='0.1.0';browser_version='157.0.1.2';passed=$true;atomic_job_membership_verified=$true;owned_window_verified=$true;developer_mode_enabled=$true;load_unpacked_invoked=$true;folder_picker_owned=$true;folder_path_value_verified=$true;folder_picker_closed=$true;extension_loaded=$true;browser_job_drained=$true;full_delivery_verified=$false;native_host_registered=$false;native_host_executed=$false;authenticated_bridge_verified=$false;app_launched=$false;devtools_or_cdp_used=$false;extension_code_modified=$false}
$valid.extension_card_identity_verified=$true
$report=ConvertTo-PickleExtensionStageReport ([pscustomobject]$valid)
Assert-PickleExtensionStageReport $report 'chrome'
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $report 'edge' }
$wrongCard=$valid | ConvertTo-Json | ConvertFrom-Json;$wrongCard.extension_card_identity_verified=$false
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $wrongCard 'chrome' }
foreach ($change in @(@('schema',1.0),@('schema','1'),@('passed',$false),@('atomic_job_membership_verified',$false),@('owned_window_verified',$false),@('developer_mode_enabled',$false),@('load_unpacked_invoked',$false),@('folder_picker_owned',$false),@('folder_path_value_verified',$false),@('folder_picker_closed',$false),@('extension_loaded',$false),@('browser_job_drained',$false),@('full_delivery_verified',$true),@('native_host_registered',$true),@('authenticated_bridge_verified',$true),@('devtools_or_cdp_used',$true),@('extension_code_modified',$true),@('extension_id','qbcdefghijklmnopabcdefghijklmnop'),@('extension_version','0.1.1'),@('passed','true'))) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { Assert-PickleExtensionStageReport $bad 'chrome' }
}
$unexpected=$valid | ConvertTo-Json | ConvertFrom-Json
$unexpected | Add-Member -NotePropertyName arbitrary_window_text -NotePropertyValue 'private-unexpected-text'
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $unexpected }
$failure=[pscustomobject]@{schema=1;test='windows-stable-bundled-extension-load';scope='bundled-extension-load-only';browser='edge';passed=$false;stage='owned-folder-picker';failure='owned-folder-picker-unavailable';last_owned_picker_windows=0;browser_job_drained=$true}
$null=ConvertTo-PickleExtensionStageReport $failure
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $failure 'edge' }
$candidate=[ordered]@{label_exact=$true;label_casefold=$true;label_trim_casefold=$true;label_known_substring=$true;control_type_id=50000;enabled=$true;offscreen=$false;toggle_supported=$false;toggle_state=-1;invoke_supported=$true}
$observation=[ordered]@{nodes=294;property_errors=0;document_count=1;extensions_label_exact_count=1;value_pattern_count=1;internal_uri_exact_count=1;internal_uri_without_slash_count=1;candidate_count=1;candidates_truncated=$false;legacy_observed=$false;candidates=@($candidate)}
$observed=$observation | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$clean=ConvertTo-PickleExtensionDeveloperObservation $observed
if ($clean.candidates.Count -ne 1 -or !$clean.candidates[0].invoke_supported -or $clean.candidates[0].toggle_supported -or $clean.legacy_observed) { throw 'Observed metadata was changed by sanitization.' }
$withObservation=$failure | ConvertTo-Json | ConvertFrom-Json
$withObservation | Add-Member -NotePropertyName developer_observation -NotePropertyValue $observed
$null=ConvertTo-PickleExtensionStageReport $withObservation
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $withObservation 'edge' }
foreach ($change in @(@('nodes',1025),@('candidate_count',17),@('legacy_observed','true'),@('legacy_observed',$true))) {
  $bad=$observation | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionDeveloperObservation $bad }
}
$bad=$observation | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$bad.candidates[0] | Add-Member -NotePropertyName name -NotePropertyValue 'must-not-be-published'
Assert-ExtensionRefused { ConvertTo-PickleExtensionDeveloperObservation $bad }
$bad=$observation | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$bad | Add-Member -NotePropertyName url -NotePropertyValue 'https://private.invalid/'
Assert-ExtensionRefused { ConvertTo-PickleExtensionDeveloperObservation $bad }
$bad=$observation | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$bad.candidates=@(1..17 | ForEach-Object { $observed.candidates[0] });$bad.candidate_count=17;$bad.candidates_truncated=$true
Assert-ExtensionRefused { ConvertTo-PickleExtensionDeveloperObservation $bad }
$state=[pscustomobject]@{total_requests=3;pages_served=1;rejected_requests=2;forwarded_requests=0}
$null=ConvertTo-PickleExtensionProxyState $state
foreach ($change in @(@('forwarded_requests',1),@('total_requests',4),@('pages_served',1.0),@('rejected_requests',-1))) {
  $bad=$state | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionProxyState $bad }
}
$outer=[ordered]@{schema=1;test='windows-stable-extension-setup-stage';scope='bundled-extension-load-only';browser='chrome';passed=$true;app_profile_unchanged=$true;owned_processes_stopped=$true;temporary_profile_removed=$true;bundled_extension_unchanged=$true;full_delivery_verified=$false;app_launched=$false;app_profile_touched=$false;native_host_registered=$false;native_host_executed=$false;authenticated_bridge_verified=$false;manifest_sha256=('A'*64);driver=$valid;server=$state}
Assert-PickleExtensionSetupReport ($outer | ConvertTo-Json -Depth 6 | ConvertFrom-Json) 'chrome'
foreach ($change in @(@('passed',$false),@('owned_processes_stopped',$false),@('temporary_profile_removed',$false),@('app_profile_unchanged',$false),@('bundled_extension_unchanged',$false),@('full_delivery_verified',$true),@('app_launched',$true),@('native_host_registered',$true),@('schema','1'))) {
  $bad=$outer | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { Assert-PickleExtensionSetupReport $bad 'chrome' }
}
$cleanFailure=$outer | ConvertTo-Json -Depth 6 | ConvertFrom-Json
$cleanFailure.passed=$false;$cleanFailure.driver.passed=$false;$cleanFailure.driver.extension_loaded=$false
Assert-PickleExtensionCleanupReport $cleanFailure 'chrome'
Assert-ExtensionRefused { Assert-PickleExtensionSetupReport $cleanFailure 'chrome' }
$cleanFailure.driver.native_host_executed=$true
Assert-ExtensionRefused { Assert-PickleExtensionCleanupReport $cleanFailure 'chrome' }
$cleanFailure.driver.native_host_executed=$false
$cleanFailure.driver.browser_job_drained=$false
Assert-ExtensionRefused { Assert-PickleExtensionCleanupReport $cleanFailure 'chrome' }
$start=New-PickleExtensionCoreStart 'C:\Program Files\PowerShell\7\pwsh.exe' 'C:\owned script\entry.ps1' 'C:\owned package\' 'edge' 'C:\owned evidence\report.json'
$expected=@('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File','C:\owned script\entry.ps1','-PackageDirectory','C:\owned package\','-Browser','edge','-EvidencePath','C:\owned evidence\report.json')
if ($start.UseShellExecute -or $start.FileName -cne 'C:\Program Files\PowerShell\7\pwsh.exe' -or ($start.ArgumentList -join [char]0) -cne ($expected -join [char]0)) { throw 'The dedicated child lost its exact executable or structured argument boundaries.' }
# Real owned non-UI processes exercise success, nonzero status and forced bounded
# stop. They are not browser/native job semantics acceptance on this host.
foreach ($mode in @('exit-zero','exit-nonzero','timeout')) {
  $code=if ($mode -ceq 'timeout') { 'import time; time.sleep(30)' } elseif ($mode -ceq 'exit-nonzero') { 'import sys; sys.exit(7)' } else { 'import sys; sys.exit(0)' }
  $child=Start-PickleReadingChild $python.Source @('-c',$code)
  try {
    if ($mode -ceq 'timeout') {
      Assert-ExtensionRefused { Wait-PickleExtensionCore $child 500 }
      if (!$child.HasExited) { throw 'The timed-out retained owner remained alive.' }
    } else {
      $status=Wait-PickleExtensionCore $child 5000
      if ($status -ne $(if ($mode -ceq 'exit-nonzero') { 7 } else { 0 })) { throw 'The retained owner exit code was lost.' }
    }
  } finally { if (!$child.HasExited) { $child.Kill();$child.WaitForExit(10000) | Out-Null };$child.Dispose() }
}
$root=Join-Path ([IO.Path]::GetTempPath()) ('pickle-extension-tests-'+[guid]::NewGuid().ToString('N'))
if ($root.StartsWith('/var/') -and (Test-Path -LiteralPath '/private/var')) { $root='/private'+$root }
New-Item -ItemType Directory -Path $root | Out-Null
try {
  $package=Join-Path $root 'package';New-Item -ItemType Directory -Path $package | Out-Null
  foreach ($name in @('PickleRune.exe','pickle-browser-host.exe')) { [IO.File]::WriteAllText((Join-Path $package $name),'owned-not-executable') }
  Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../../browser-extension') -Destination (Join-Path $package 'browser-extension') -Recurse
  $bundle=Get-PickleBundledExtension $package
  $before=Get-PickleExtensionFingerprint $bundle.Directory
  Assert-PickleExtensionFingerprint $before (Get-PickleExtensionFingerprint $bundle.Directory)
  [IO.File]::AppendAllText((Join-Path $bundle.Directory 'background.js'),'changed')
  Assert-ExtensionRefused { Assert-PickleExtensionFingerprint $before (Get-PickleExtensionFingerprint $bundle.Directory) }
  [IO.File]::WriteAllText((Join-Path $bundle.Directory 'extra.txt'),'unexpected')
  Assert-ExtensionRefused { Get-PickleBundledExtension $package }
  $support=Join-Path $root 'support';New-Item -ItemType Directory -Path $support | Out-Null
  Assert-PickleExtensionHostFilesAbsent $support
  [IO.File]::WriteAllText((Join-Path $support 'instance.json'),'owned control bytes')
  $controls=Get-PickleExtensionFingerprint $support
  [IO.File]::WriteAllText((Join-Path $support 'instance.json'),'changed')
  Assert-ExtensionRefused { Assert-PickleExtensionFingerprint $controls (Get-PickleExtensionFingerprint $support) }
  $hostDir=Join-Path $support 'NativeHost';New-Item -ItemType Directory -Path $hostDir | Out-Null
  [IO.File]::WriteAllText((Join-Path $hostDir 'existing.json'),'do not replace')
  Assert-ExtensionRefused { Assert-PickleExtensionHostFilesAbsent $support }
  if ([IO.File]::ReadAllText((Join-Path $hostDir 'existing.json')) -cne 'do not replace') { throw 'Foreign native-host data changed.' }
  if ($IsWindows) {
    $exe=Build-PickleExtensionFixture $root
    $child=Start-PickleReadingChild $exe @('--self-test')
    try { if (!$child.WaitForExit(10000) -or $child.ExitCode -ne 0) { throw 'Native fixture non-UI predicates failed.' } }
    finally { if (!$child.HasExited) { $child.Kill();$child.WaitForExit(10000) | Out-Null };$child.Dispose() }
  } else {
    Assert-ExtensionRefused { Invoke-PickleExtensionSetupStage $package 'chrome' (Join-Path $root 'must-not-exist.json') }
    if (Test-Path -LiteralPath (Join-Path $root 'must-not-exist.json')) { throw 'Non-Windows acceptance touched native evidence.' }
  }
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
Write-Output 'PASS extension setup scope, refusal, inventory, non-forwarding proxy and native-free predicates.'
