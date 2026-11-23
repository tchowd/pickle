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
foreach ($key in @('owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified')) { $valid[$key]=$true }
$valid.navigation_shortcut_events=4;$valid.navigation_enter_events=2;$valid.navigation_confirm_shortcut_events=4;$valid.navigation_escape_events=2
$report=ConvertTo-PickleExtensionStageReport ([pscustomobject]$valid)
Assert-PickleExtensionStageReport $report 'chrome'
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $report 'edge' }
$wrongCard=$valid | ConvertTo-Json | ConvertFrom-Json;$wrongCard.extension_card_identity_verified=$false
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $wrongCard 'chrome' }
foreach ($change in @(@('address_focus_ancestry_verified',$false),@('owned_browser_foreground_verified',$false),@('navigation_value_verified',$false),@('extensions_document_verified',$false),@('committed_internal_uri_verified',$false),@('navigation_enter_events',1),@('navigation_confirm_shortcut_events',3),@('navigation_escape_events',2.0))) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { Assert-PickleExtensionStageReport $bad 'chrome' }
}
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
$observation=[ordered]@{nodes=294;property_errors=0;document_count=1;extensions_label_exact_count=1;extensions_document_exact_count=1;welcome_document_known_count=0;new_tab_document_known_count=0;address_edit_known_name_count=1;value_pattern_count=1;internal_uri_exact_count=1;internal_uri_without_slash_count=1;candidate_count=1;candidates_truncated=$false;legacy_observed=$false;candidates=@($candidate)}
$observed=$observation | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$clean=ConvertTo-PickleExtensionDeveloperObservation $observed
if ($clean.candidates.Count -ne 1 -or !$clean.candidates[0].invoke_supported -or $clean.candidates[0].toggle_supported -or $clean.legacy_observed) { throw 'Observed metadata was changed by sanitization.' }
$withObservation=$failure | ConvertTo-Json | ConvertFrom-Json
$withObservation | Add-Member -NotePropertyName developer_observation -NotePropertyValue $observed
$null=ConvertTo-PickleExtensionStageReport $withObservation
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $withObservation 'edge' }
$navigationFailure=$withObservation | ConvertTo-Json -Depth 6 | ConvertFrom-Json
$navigationFailure.stage='ordinary-navigation';$navigationFailure.failure='owned-address-bar-unavailable'
$navigationFailure | Add-Member -NotePropertyName page_observation_before_navigation -NotePropertyValue $observed
$navigationFailure | Add-Member -NotePropertyName navigation_failure_observation -NotePropertyValue $observed
$null=ConvertTo-PickleExtensionStageReport $navigationFailure
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $navigationFailure 'edge' }
$failureObserved=[ordered]@{
  incomplete=$false;nodes=20;property_errors=0;title_candidate_count=1;title_records_truncated=$false
  title_records=@([ordered]@{flags=3;known_error_flags=0;bounds_nonempty=$false;bounds_intersect_window=$false;bounds_clipped=$false;control_type_id=50020;offscreen=$false;ancestor_role_ids=@(50020,50026,50032)})
  developer_candidate_count=1;developer_records_truncated=$false;toggle_count=1;checkbox_count=0;button_count=0
  developer_records=@([ordered]@{name_flags=0;help_flags=0;labeled_by_flags=15;labeled_by_bound=$true;control_type_id=50002;enabled=$true;offscreen=$false;toggle_supported=$true;toggle_state=1;invoke_supported=$false})
  owned_window_count=1;window_records_truncated=$false
  window_records=@([ordered]@{class_kind=2;original_window=$false;owner_is_original=$true;root_is_original=$false;foreground=$true;known_error_count=1;scan_nodes=4;scan_truncated=$false;scan_unavailable=$false})
}
$failureObserved=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
$clean=ConvertTo-PickleExtensionFailureObservation $failureObserved
if ($clean.title_records[0].flags -ne 3 -or $clean.title_records[0].ancestor_role_ids.Count -ne 3 -or $clean.developer_records[0].labeled_by_flags -ne 15 -or $clean.developer_records[0].name_flags -ne 0 -or $clean.window_records[0].known_error_count -ne 1) { throw 'Closed scalar mapping observations were changed.' }
$partial=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
$partial.incomplete=$true;$partial.owned_window_count=0;$partial.window_records=@()
$partialClean=ConvertTo-PickleExtensionFailureObservation $partial
if (!$partialClean.incomplete -or $partialClean.title_records[0].flags -ne 3 -or $partialClean.developer_records[0].labeled_by_flags -ne 15 -or $partialClean.window_records.Count -ne 0) { throw 'Completed primary records were lost from a partial failure observation.' }
$diagnosed=$failure | ConvertTo-Json | ConvertFrom-Json
$diagnosed.stage='developer-mode';$diagnosed.failure='developer-toggle-unavailable'
$diagnosed | Add-Member -NotePropertyName failure_observation -NotePropertyValue $failureObserved
$null=ConvertTo-PickleExtensionStageReport $diagnosed
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $diagnosed 'edge' }
$partialReport=$diagnosed | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$partialReport.failure_observation=$partial
$null=ConvertTo-PickleExtensionStageReport $partialReport
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $partialReport 'edge' }
$bad=$valid | ConvertTo-Json | ConvertFrom-Json
$bad | Add-Member -NotePropertyName failure_observation -NotePropertyValue $failureObserved
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $bad 'chrome' }
foreach ($change in @(@('incomplete','true'),@('nodes',1025),@('owned_window_count',65),@('title_candidate_count',21),@('title_records_truncated',$true),@('property_errors','0'))) {
  $bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
}
foreach ($group in @('title','developer','window')) {
  $bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
  $bad.($group+'_records')[0] | Add-Member -NotePropertyName arbitrary_text -NotePropertyValue 'must-not-be-published'
  Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
  $bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
  $bad.($group+'_records')=@(1..9 | ForEach-Object { $failureObserved.($group+'_records')[0] })
  Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
}
foreach ($change in @(@('flags',0),@('known_error_flags',32),@('known_error_flags',1),@('bounds_nonempty',$true),@('bounds_clipped',$true),@('bounds_intersect_window','true'),@('control_type_id',49999),@('offscreen','false'),@('ancestor_role_ids',@(50020,50026,50032,50032,50032,50032,50032,50032,50032)))) {
  $bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json;$bad.title_records[0].($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
}
$knownError=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
$knownError.title_records[0].flags=16;$knownError.title_records[0].known_error_flags=16
$knownError.title_records[0].bounds_nonempty=$true;$knownError.title_records[0].bounds_intersect_window=$true;$knownError.title_records[0].bounds_clipped=$true
$knownClean=ConvertTo-PickleExtensionFailureObservation $knownError
if ($knownClean.title_records[0].known_error_flags -ne 16 -or !$knownClean.title_records[0].bounds_intersect_window -or !$knownClean.title_records[0].bounds_clipped) { throw 'Exact known error and bounded rectangle flags were lost.' }
$capture=[pscustomobject]@{status='captured';owned_before=$true;owned_after=$true;child_started=$true;child_stopped=$true;width=2;height=3;png_bytes=24}
$null=ConvertTo-PickleExtensionWindowCapture $capture
foreach ($change in @(@('status','private text'),@('owned_before',$false),@('owned_after',$false),@('child_started',$false),@('child_stopped',$false),@('width',4097),@('height',2161),@('png_bytes',4194305),@('png_bytes',23),@('height',3.0),@('owned_after',1))) {
  $bad=$capture | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionWindowCapture $bad }
}
foreach ($status in @('unavailable','timed-out','helper-failed','ownership-rejected','cleanup-failed')) {
  $bad=$capture | ConvertTo-Json | ConvertFrom-Json;$bad.status=$status
  Assert-ExtensionRefused { ConvertTo-PickleExtensionWindowCapture $bad }
  $bad.width=0;$bad.height=0;$bad.png_bytes=0;$bad.child_stopped=($status -cne 'cleanup-failed')
  $null=ConvertTo-PickleExtensionWindowCapture $bad
}
$bad=$capture | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member -NotePropertyName raw_title -NotePropertyValue 'must-not-persist'
Assert-ExtensionRefused { ConvertTo-PickleExtensionWindowCapture $bad }
$withCapture=$diagnosed | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$withCapture | Add-Member -NotePropertyName window_capture -NotePropertyValue $capture
$null=ConvertTo-PickleExtensionStageReport $withCapture
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $withCapture 'edge' }
$withCapture.passed=$true
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $withCapture }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $withCapture 'edge' }
foreach ($change in @(@('name_flags',1),@('labeled_by_bound',$false),@('toggle_state',-1),@('toggle_supported',$false),@('enabled',1))) {
  $bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json;$bad.developer_records[0].($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
}
foreach ($change in @(@('class_kind',3),@('known_error_count',5),@('scan_nodes',129),@('foreground',1))) {
  $bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json;$bad.window_records[0].($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
}
$bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
$bad | Add-Member -NotePropertyName private_url -NotePropertyValue 'must-not-be-published'
Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
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
$cleanFailure.driver.browser_job_drained=$true
$unreaped=[pscustomobject]@{status='cleanup-failed';owned_before=$true;owned_after=$false;child_started=$true;child_stopped=$false;width=0;height=0;png_bytes=0}
$cleanFailure.driver | Add-Member -NotePropertyName stage -NotePropertyValue 'developer-mode'
$cleanFailure.driver | Add-Member -NotePropertyName failure -NotePropertyValue 'developer-toggle-unavailable'
$cleanFailure.driver | Add-Member -NotePropertyName window_capture -NotePropertyValue $unreaped
Assert-ExtensionRefused { Assert-PickleExtensionCleanupReport $cleanFailure 'chrome' }
$cleanFailure.driver.window_capture.status='timed-out';$cleanFailure.driver.window_capture.child_stopped=$true
Assert-PickleExtensionCleanupReport $cleanFailure 'chrome'
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
  # Owned synthetic PNG header exercises the actual bounded copier; it is never
  # treated as native capture evidence, decoded or displayed on this host.
  $captureWork=Join-Path $root 'capture';New-Item -ItemType Directory -Path $captureWork | Out-Null
  $png=Join-Path $captureWork 'owned-window.png';$captureEvidence=Join-Path $root 'diagnostic.json'
  $header=[byte[]]@(137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,2,0,0,0,3)
  [IO.File]::WriteAllBytes($png,$header)
  if (!(Copy-PickleExtensionWindowCapture $capture $captureWork $captureEvidence) -or [Convert]::ToBase64String([IO.File]::ReadAllBytes([IO.Path]::ChangeExtension($captureEvidence,'.png'))) -cne [Convert]::ToBase64String($header)) { throw 'The bounded fixed-name diagnostic copy lost its bytes.' }
  Assert-ExtensionRefused { Copy-PickleExtensionWindowCapture $capture $captureWork $captureEvidence }
  $freshEvidence=Join-Path $root 'refused.json'
  $header[19]=4;[IO.File]::WriteAllBytes($png,$header)
  Assert-ExtensionRefused { Copy-PickleExtensionWindowCapture $capture $captureWork $freshEvidence }
  if (Test-Path -LiteralPath ([IO.Path]::ChangeExtension($freshEvidence,'.png'))) { throw 'A dimension mismatch produced an artifact.' }
  $header[19]=2;$header[0]=0;[IO.File]::WriteAllBytes($png,$header)
  Assert-ExtensionRefused { Copy-PickleExtensionWindowCapture $capture $captureWork $freshEvidence }
  $header[0]=137;[IO.File]::WriteAllBytes($png,($header+[byte]0))
  Assert-ExtensionRefused { Copy-PickleExtensionWindowCapture $capture $captureWork $freshEvidence }
  $unavailable=$capture | ConvertTo-Json | ConvertFrom-Json;$unavailable.status='unavailable';$unavailable.width=0;$unavailable.height=0;$unavailable.png_bytes=0
  if (Copy-PickleExtensionWindowCapture $unavailable $captureWork $freshEvidence) { throw 'Unavailable capture fabricated an artifact.' }
  if (Test-Path -LiteralPath ([IO.Path]::ChangeExtension($freshEvidence,'.png'))) { throw 'A refused capture produced an artifact.' }
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
