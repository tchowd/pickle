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
$aggregate=@($loop.Parent.Statements | Where-Object { $_.Extent.StartOffset -gt $loop.Extent.EndOffset -and $_ -is [Management.Automation.Language.IfStatementAst] -and $_.Clauses[0].Item1.Extent.Text -ceq '$extensionFailures.Count' })
if ($aggregate.Count -ne 1) { throw 'The package must fail after collecting safe setup failures.' }
$ordered=@($loop.Parent.Statements | Where-Object { $_.Extent.StartOffset -ge $loop.Extent.StartOffset -and $_.Extent.EndOffset -le $aggregate[0].Extent.EndOffset })
$actualPair=[scriptblock]::Create(($ordered.Extent.Text -join "`n"))
foreach ($scenario in @('both-pass','chrome-failed-cleanly','chrome-cleanup-unknown','chrome-delivery-failed-cleanly','formatted-copy-failed')) {
  & {
    param($ActualPair,[string]$Scenario)
    $testDirectory='owned-installed-package';$observed=[Collections.Generic.List[string]]::new();$deliveries=[Collections.Generic.List[string]]::new();$extensionFailures=[Collections.Generic.List[string]]::new()
    $application='owned-app';$supportRoot='owned-profile';$Configuration='debug';$copy=[pscustomobject]@{called=0;read=0;validated=0}
    $copyFails=$Scenario -ceq 'formatted-copy-failed'
    function Invoke-PickleExtensionCore([string]$PackageDirectory,[string]$Browser,[string]$EvidencePath) {
      if ($PackageDirectory -cne 'owned-installed-package' -or $EvidencePath -cne (Join-Path $PWD "artifacts/windows-extension-$Browser-native-smoke.json")) { throw 'The actual package setup call lost installed-payload ownership.' }
      $observed.Add($Browser)
      if ($Browser -ceq 'chrome' -and $Scenario -ceq 'chrome-cleanup-unknown') { throw 'owned cleanup unknown' }
      return !($Browser -ceq 'chrome' -and $Scenario -ceq 'chrome-failed-cleanly')
    }
    function Invoke-PickleExtensionDeliveryCore([string]$PackageDirectory,[string]$Browser,[string]$EvidencePath) {
      if ($PackageDirectory -cne 'owned-installed-package' -or $EvidencePath -cne (Join-Path $PWD "artifacts/windows-extension-$Browser-delivery-native-smoke.json")) { throw 'The delivery call lost its exact package/evidence boundaries.' }
      $deliveries.Add($Browser)
      return !($Browser -ceq 'chrome' -and $Scenario -ceq 'chrome-delivery-failed-cleanly')
    }
    function Invoke-PickleReadingCore($Application,$DataDirectory,$EvidencePath,$Configuration,[string]$Scenario,[switch]$RequireWindowCapture) {
      if (($observed -join ',') -cne 'chrome,edge' -or $copy.called -ne 0 -or $Application -cne 'owned-app' -or $DataDirectory -cne 'owned-profile' -or $Configuration -cne 'debug' -or $Scenario -cne 'formatted-copy' -or $RequireWindowCapture -or $EvidencePath -cne (Join-Path $PWD 'artifacts/unpackaged-debug-native-smoke-formatted-copy-reading.json')) { throw 'The native Copy workflow ran before independent browsers or with different arguments.' }
      $copy.called++
      if ($copyFails) { throw 'owned formatted Copy failed' }
    }
    function Read-PickleReadingJson($Path) {
      if ($copy.called -ne 1 -or $copy.read -ne 0 -or $Path -cne (Join-Path $PWD 'artifacts/unpackaged-debug-native-smoke-formatted-copy-reading.json')) { throw 'The actual Copy report was not read after its workflow.' }
      $copy.read++;return 'owned-copy-report'
    }
    function Assert-PicklePackageReadingEvidence($Evidence,$Required,$Configuration,[string]$Scenario) {
      if ($copy.read -ne 1 -or $copy.validated -ne 0 -or $Evidence -cne 'owned-copy-report' -or $Required -or $Configuration -cne 'debug' -or $Scenario -cne 'formatted-copy') { throw 'The actual Copy report was not required.' }
      $copy.validated++
    }
    $failed=$false;try { & $ActualPair } catch { $failed=$true }
    $expected=if ($Scenario -ceq 'chrome-cleanup-unknown') { 'chrome' } else { 'chrome,edge' }
    if (($observed -join ',') -cne $expected -or $failed -ne ($Scenario -cne 'both-pass')) { throw 'The actual package did not collect safe failures, stop on uncertain cleanup, or fail after a rejected setup.' }
    $expectedDelivery=if ($Scenario -ceq 'chrome-cleanup-unknown') { '' } else { 'chrome,edge' }
    if (($deliveries -join ',') -cne $expectedDelivery) { throw 'Delivery must follow each clean component outcome independently, and stop on unsafe cleanup.' }
    $expectedCopy=if ($Scenario -ceq 'chrome-cleanup-unknown') { 0 } else { 1 }
    $expectedReport=if ($copyFails) { 0 } else { $expectedCopy }
    if ($copy.called -ne $expectedCopy -or $copy.read -ne $expectedReport -or $copy.validated -ne $expectedReport) { throw 'Safe browser failures must allow Copy before aggregate refusal; unsafe cleanup must stop before Copy.' }
  } $actualPair $scenario
}
$python=Get-Command python -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if (!$python) { $python=Get-Command python3 -CommandType Application -ErrorAction Stop | Select-Object -First 1 }
& $python.Source (Join-Path $PSScriptRoot 'test_windows_extension_page_fixture.py')
if ($LASTEXITCODE -ne 0) { throw 'The non-forwarding owned proxy contract failed.' }
$valid=[ordered]@{schema=1;test='windows-stable-bundled-extension-load';scope='bundled-extension-load-only';browser='chrome';extension_id='abcdefghijklmnopabcdefghijklmnop';extension_version='0.1.0';browser_version='157.0.1.2';passed=$true;atomic_job_membership_verified=$true;owned_window_verified=$true;developer_mode_enabled=$true;load_unpacked_invoked=$true;folder_picker_owned=$true;folder_path_value_verified=$true;folder_picker_closed=$true;extension_loaded=$true;browser_job_drained=$true;full_delivery_verified=$false;native_host_registered=$false;native_host_executed=$false;authenticated_bridge_verified=$false;app_launched=$false;devtools_or_cdp_used=$false;extension_code_modified=$false}
$valid.extension_card_identity_verified=$true
$valid.extension_identity_proof='card';$valid.extension_details_identity_verified=$false
foreach ($key in @('owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified')) { $valid[$key]=$true }
$valid.navigation_shortcut_events=4;$valid.navigation_enter_events=2;$valid.navigation_confirm_shortcut_events=4;$valid.navigation_escape_events=2
$report=ConvertTo-PickleExtensionStageReport ([pscustomobject]$valid)
Assert-PickleExtensionStageReport $report 'chrome'
$unacceptedDeliveryProof=$valid | ConvertTo-Json | ConvertFrom-Json
$unacceptedDeliveryProof.extension_identity_proof='authenticated-delivery'
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $unacceptedDeliveryProof }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $unacceptedDeliveryProof 'chrome' }
foreach ($pair in @(@('failure','native-operation-failed'),@('error_kind','com'),@('error_hresult',0))) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member -NotePropertyName $pair[0] -NotePropertyValue $pair[1]
  $null=ConvertTo-PickleExtensionStageReport $bad
  Assert-ExtensionRefused { Assert-PickleExtensionStageReport $bad 'chrome' }
}
$stale=$valid | ConvertTo-Json | ConvertFrom-Json
$stale.passed=$false;$stale | Add-Member stage 'ordinary-navigation'
$stale | Add-Member failure 'native-operation-failed';$stale | Add-Member error_kind 'element-not-available';$stale | Add-Member error_hresult ([int]-2147220991)
$staleClean=ConvertTo-PickleExtensionStageReport $stale
if ($staleClean.error_kind -cne 'element-not-available' -or $staleClean.error_hresult -ne -2147220991) { throw 'Stale native element classification was lost.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $staleClean 'chrome' }
$stale.error_kind='ElementNotAvailableException with raw details'
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $stale }
$layout=[ordered]@{stage='complete';action_performed=$true;safely_verified=$true;before_verified=$true;set_window_pos_succeeded=$true;after_verified=$true;unavailable=$false;requested_outer_width=1280;requested_outer_height=800;before_client_width=988;before_client_height=692;after_client_width=1264;after_client_height=762;after_outer_width=1280;after_outer_height=800} | ConvertTo-Json | ConvertFrom-Json
$layoutClean=ConvertTo-PickleExtensionLayout $layout
if (!$layoutClean.safely_verified -or $layoutClean.after_outer_width -ne 1280) { throw 'The sole safely verified resize changed.' }
$recovered=$valid | ConvertTo-Json | ConvertFrom-Json;$recovered.browser='edge'
$recovered | Add-Member -NotePropertyName initial_developer_failure -NotePropertyValue 'developer-toggle-unavailable'
$recovered | Add-Member -NotePropertyName edge_layout_observation -NotePropertyValue $layout
Assert-PickleExtensionStageReport (ConvertTo-PickleExtensionStageReport $recovered) 'edge'
$menu=[pscustomobject][ordered]@{stage='complete';action_performed=$true;safely_verified=$true;before_verified=$true;discovery_complete=$true;candidate_count=1;control_verified=$true;invoke_completed=$true;after_verified=$true;unavailable=$false}
$menuRecovered=$recovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$menuRecovered.PSObject.Properties.Remove('edge_layout_observation');$menuRecovered | Add-Member edge_menu_observation $menu
Assert-PickleExtensionStageReport (ConvertTo-PickleExtensionStageReport $menuRecovered) 'edge'
$loadMenu=$menu | ConvertTo-Json | ConvertFrom-Json
$loadMenu | Add-Member prior_menu_verified $true;$loadMenu | Add-Member developer_mode_on_verified $true
$loadRecovered=$menuRecovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$loadRecovered | Add-Member stage 'browser-cleanup';$loadRecovered | Add-Member initial_load_failure 'load-unpacked-unavailable';$loadRecovered | Add-Member edge_load_menu_observation $loadMenu
$loadRecoveredClean=ConvertTo-PickleExtensionStageReport $loadRecovered
Assert-PickleExtensionStageReport $loadRecoveredClean 'edge'
if (!$loadRecoveredClean.edge_menu_observation.safely_verified -or $loadRecoveredClean.initial_developer_failure -cne 'developer-toggle-unavailable' -or $loadRecoveredClean.initial_load_failure -cne 'load-unpacked-unavailable') { throw 'The original menu proof and first Load failure must remain distinct.' }
foreach ($change in @(@('prior_menu_verified',$false),@('developer_mode_on_verified',$false),@('developer_mode_on_verified','true'))) {
  $bad=$loadMenu | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionMenu $bad -AfterLoad }
}
foreach ($field in @('prior_menu_verified','developer_mode_on_verified')) {
  $bad=$loadMenu | ConvertTo-Json | ConvertFrom-Json;$bad.PSObject.Properties.Remove($field)
  Assert-ExtensionRefused { ConvertTo-PickleExtensionMenu $bad -AfterLoad }
}
$bad=$loadMenu | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member menu_closed $true
Assert-ExtensionRefused { ConvertTo-PickleExtensionMenu $bad -AfterLoad }
foreach ($change in @(@('browser','chrome'),@('stage','developer-mode'),@('initial_load_failure','stage-deadline'),@('developer_mode_enabled',$false))) {
  $bad=$loadRecovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
}
foreach ($field in @('edge_menu_observation','edge_load_menu_observation','initial_load_failure')) {
  $bad=$loadRecovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.PSObject.Properties.Remove($field)
  Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
}
$partialLoadMenu=$loadMenu | ConvertTo-Json | ConvertFrom-Json
$partialLoadMenu.stage='before-invoke';$partialLoadMenu.safely_verified=$false;$partialLoadMenu.invoke_completed=$false;$partialLoadMenu.after_verified=$false;$partialLoadMenu.unavailable=$true
$failedLoadMenu=$loadRecovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$failedLoadMenu.stage='load-unpacked';$failedLoadMenu.passed=$false;$failedLoadMenu.edge_load_menu_observation=$partialLoadMenu;$failedLoadMenu | Add-Member failure 'edge-load-menu-unverified'
if (!(ConvertTo-PickleExtensionStageReport $failedLoadMenu).edge_load_menu_observation.action_performed) { throw 'An unverified second menu action must retain partial facts.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $failedLoadMenu 'edge' }
foreach ($change in @(@('stage','after-document'),@('action_performed',$false),@('safely_verified',$false),@('before_verified',$false),@('discovery_complete',$false),@('candidate_count',2),@('candidate_count','1'),@('control_verified',$false),@('invoke_completed',$false),@('after_verified',$false),@('unavailable',$true))) {
  $bad=$menu | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionMenu $bad }
}
foreach ($key in @($menu.PSObject.Properties.Name)) {
  $bad=$menu | ConvertTo-Json | ConvertFrom-Json;$bad.PSObject.Properties.Remove($key)
  Assert-ExtensionRefused { ConvertTo-PickleExtensionMenu $bad }
}
$bad=$menu | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member raw_name 'not-retained'
Assert-ExtensionRefused { ConvertTo-PickleExtensionMenu $bad }
# A deadline can expire after proving the control but before Invoke admission.
$partialMenu=$menu | ConvertTo-Json | ConvertFrom-Json
$partialMenu.stage='before-invoke';$partialMenu.action_performed=$false;$partialMenu.safely_verified=$false;$partialMenu.invoke_completed=$false;$partialMenu.after_verified=$false;$partialMenu.unavailable=$true
if (!(ConvertTo-PickleExtensionMenu $partialMenu).control_verified) { throw 'Pre-Invoke control proof was erased.' }
$partialMenu.action_performed=$true
$partialReceipt=$menuRecovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$partialReceipt.passed=$false;$partialReceipt.edge_menu_observation=$partialMenu
$partialReceipt | Add-Member failure 'edge-menu-unverified'
if (!(ConvertTo-PickleExtensionStageReport $partialReceipt).edge_menu_observation.action_performed) { throw 'Unsafe attempted menu action lost its partial facts.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $partialReceipt 'edge' }
$bad=$menuRecovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad | Add-Member edge_layout_observation $layout
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
$bad=$menuRecovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.browser='chrome'
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
foreach ($change in @(@('requested_outer_width',1024),@('after_outer_height',799),@('after_client_width',1281),@('action_performed',$false),@('safely_verified',$false),@('before_verified',$false),@('set_window_pos_succeeded',$false),@('after_verified',$false),@('unavailable',$true),@('stage','after-document'),@('before_client_width',0),@('after_outer_width','1280'))) {
  $bad=$layout | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionLayout $bad }
}
$unsafe=$layout | ConvertTo-Json | ConvertFrom-Json;$unsafe.stage='after-document';$unsafe.safely_verified=$false;$unsafe.after_verified=$false;$unsafe.unavailable=$true
$unsafeReceipt=$recovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$unsafeReceipt.passed=$false;$unsafeReceipt.edge_layout_observation=$unsafe
$unsafeReceipt | Add-Member -NotePropertyName failure -NotePropertyValue 'edge-layout-unverified'
$unsafeClean=ConvertTo-PickleExtensionStageReport $unsafeReceipt
if ($unsafeClean.edge_layout_observation.safely_verified -or $unsafeClean.initial_developer_failure -cne 'developer-toggle-unavailable' -or $unsafeClean.failure -cne 'edge-layout-unverified') { throw 'Unsafe attempted layout lost the original discovery or stop evidence.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $unsafeReceipt 'edge' }
$bad=$recovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.browser='chrome'
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
$bad=$recovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.initial_developer_failure='stage-deadline'
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
$bad=$recovered | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.PSObject.Properties.Remove('edge_layout_observation')
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
$bad=$layout | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member -NotePropertyName arbitrary_window_text -NotePropertyValue 'not-retained'
Assert-ExtensionRefused { ConvertTo-PickleExtensionLayout $bad }
# This exception is limited to safe control labels in the independently proved
# fresh hosted internal page; it never admits reading text, paths or values.
foreach ($label in @('Menu','Open navigation','Settings and more','Café (2)','More...')) {
  if (!(Test-PickleEdgeNavigationLabel $label)) { throw 'A safe fixture navigation label was rejected.' }
}
foreach ($label in @($null,'',' leading','trailing ','https://example.invalid','example.invalid','www.example.com','a@b.test','C:\owned','edge:extensions',"line`nnext",'a/b','a\b',"a`t b",('a'*81),'例え.テスト')) {
  if (Test-PickleEdgeNavigationLabel $label) { throw 'Unsafe fixture control text was admitted.' }
}
$navigationControl=[pscustomobject][ordered]@{role=50000;label='Menu';enabled=$true;offscreen=$false;bounds_flags=3;invoke_supported=$true;toggle_supported=$false;toggle_state=-1;expand_supported=$true;expand_state=0;ancestor_roles=@(50000,50026,50030);ancestors_truncated=$false}
$navigation=[pscustomobject][ordered]@{stage='complete';incomplete=$false;proof_verified=$true;nodes=12;controls=1;property_errors=0;truncated=$false;records=@($navigationControl);work_area_width=1024;work_area_height=768;window_dpi=96}
$navigationClean=ConvertTo-PickleEdgeNavigation $navigation
if (!$navigationClean.proof_verified -or $navigationClean.records[0].label -cne 'Menu') { throw 'Proved internal-page controls lost their bounded labels.' }
$prefix=$navigation | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$prefix.stage='before-document';$prefix.proof_verified=$false;$prefix.incomplete=$true;$prefix.controls=0;$prefix.records=@()
$null=ConvertTo-PickleEdgeNavigation $prefix
foreach ($change in @(@('stage','unknown'),@('proof_verified',$false),@('controls',0),@('truncated',$true),@('nodes',1025),@('window_dpi','96'),@('work_area_height',$null))) {
  $bad=$navigation | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleEdgeNavigation $bad }
}
foreach ($change in @(@('label','private.example'),@('label','C:\owned'),@('role',50020),@('bounds_flags',8),@('toggle_state',0),@('expand_state',4),@('ancestors_truncated',$true))) {
  $bad=$navigation | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.records[0].($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleEdgeNavigation $bad }
}
$bad=$navigation | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.records[0].ancestor_roles=@(50000,50026)
Assert-ExtensionRefused { ConvertTo-PickleEdgeNavigation $bad }
$bad=$prefix | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.controls=1;$bad.records=@($navigationControl)
Assert-ExtensionRefused { ConvertTo-PickleEdgeNavigation $bad }
$bad=$navigation | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad | Add-Member -NotePropertyName raw_window_text -NotePropertyValue 'not retained'
Assert-ExtensionRefused { ConvertTo-PickleEdgeNavigation $bad }
$failedNavigation=$valid | ConvertTo-Json | ConvertFrom-Json;$failedNavigation.browser='edge';$failedNavigation.passed=$false
$failedNavigation | Add-Member -NotePropertyName stage -NotePropertyValue 'developer-mode'
$failedNavigation | Add-Member -NotePropertyName failure -NotePropertyValue 'developer-toggle-unavailable'
$failedNavigation | Add-Member -NotePropertyName edge_navigation_observation -NotePropertyValue $navigation
$loadDiscovery=$failedNavigation | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$loadDiscovery.stage='load-unpacked';$loadDiscovery.failure='load-unpacked-unavailable'
$loadClean=ConvertTo-PickleExtensionStageReport $loadDiscovery
if ($loadClean.stage -cne 'load-unpacked' -or $loadClean.failure -cne 'load-unpacked-unavailable' -or !$loadClean.edge_navigation_observation.proof_verified) { throw 'The exact failed Load unpacked discovery lost its bounded page observation.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $loadClean 'edge' }
foreach ($change in @(@('browser','chrome'),@('passed',$true),@('stage','ordinary-navigation'),@('stage','folder-picker-path'),@('stage','loaded-extension-details'),@('failure','developer-toggle-unavailable'),@('failure','stage-deadline'),@('failure','native-operation-failed'))) {
  $bad=$loadDiscovery | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
}
$null=ConvertTo-PickleExtensionStageReport $failedNavigation
$identity=[pscustomobject]@{stage='complete';incomplete=$false;proof_verified=$true;nodes=5;property_errors=0;title_exact_count=1;version_exact_count=0;id_syntax_count=1;extension_id_group_count=0;known_error_count=0;unclassified_text_count=2;matching_count=2;truncated=$false;records=@(
  [pscustomobject]@{role=50020;title_exact=$true;version_exact=$false;id_syntax=0;extension_id_group=$false;known_error_exact=$false;offscreen=$false;bounds_flags=3;ancestor_roles=@(50020,50026,50030);ancestors_truncated=$false},
  [pscustomobject]@{role=50020;title_exact=$false;version_exact=$false;id_syntax=2;extension_id_group=$false;known_error_exact=$false;offscreen=$false;bounds_flags=3;ancestor_roles=@(50020,50026,50030);ancestors_truncated=$false})}
$identityClean=ConvertTo-PickleEdgeIdentityObservation $identity
if ($identityClean.title_exact_count -ne 1 -or $identityClean.id_syntax_count -ne 1 -or $identityClean.extension_id_group_count -ne 0) { throw 'Independent closed labels were lost when the assumed ID group was absent.' }
foreach ($change in @(@('stage','unknown'),@('incomplete',$true),@('proof_verified',$false),@('nodes',257),@('nodes',1),@('nodes','5'),@('matching_count',1),@('title_exact_count',0),@('id_syntax_count',0),@('truncated',$true),@('property_errors',1))) {
  $bad=$identity | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleEdgeIdentityObservation $bad }
}
foreach ($field in @('name','path','id','pixels')) {
  $bad=$identity | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.records[0] | Add-Member -NotePropertyName $field -NotePropertyValue 'not retained'
  Assert-ExtensionRefused { ConvertTo-PickleEdgeIdentityObservation $bad }
}
foreach ($change in @(@('role',49999),@('id_syntax',3),@('bounds_flags',8),@('title_exact','true'),@('version_exact',$true))) {
  $bad=$identity | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.records[0].($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleEdgeIdentityObservation $bad }
}
$bad=$identity | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.records[0].ancestor_roles=@(50026,50030)
Assert-ExtensionRefused { ConvertTo-PickleEdgeIdentityObservation $bad }
$bad=$identity | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.records[0].ancestor_roles=@(50020,50026)
Assert-ExtensionRefused { ConvertTo-PickleEdgeIdentityObservation $bad }
$identityPartial=$identity | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$identityPartial.stage='traversal';$identityPartial.incomplete=$true;$identityPartial.proof_verified=$false;$identityPartial.property_errors=1
$null=ConvertTo-PickleEdgeIdentityObservation $identityPartial
$zero=[pscustomobject]@{schema=1;test='windows-stable-bundled-extension-load';scope='bundled-extension-load-only';browser='edge';passed=$false;full_delivery_verified=$false;stage='loaded-extension-details';failure='detail-candidate-unavailable';folder_picker_closed=$true;candidate_extension_id_count=0;edge_identity_observation=$identity}
$zeroClean=ConvertTo-PickleExtensionStageReport $zero
if ($zeroClean.failure -cne 'detail-candidate-unavailable' -or $zeroClean.passed -or !$zeroClean.edge_identity_observation.proof_verified) { throw 'Read-only identity diagnostics changed the original failure.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $zeroClean 'edge' }
foreach ($change in @(@('browser','chrome'),@('passed',$true),@('stage','developer-mode'),@('failure','detail-identity-unavailable'),@('candidate_extension_id_count',1),@('candidate_extension_id_count',0.0),@('folder_picker_closed',$false))) {
  $bad=$zero | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
}
$fullZero=$zero | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$fullZero.test='windows-stable-extension-reference-delivery';$fullZero.scope='extension-native-reference-delivery';$fullZero.stage='delivery-candidate';$fullZero.failure='loaded-extension-id-unavailable'
$fullZero | Add-Member bundled_extension_version '0.1.0'
$fullClean=ConvertTo-PickleExtensionDriverScalars $fullZero $true
if ($fullClean.failure -cne 'loaded-extension-id-unavailable' -or $fullClean.bundled_extension_version -cne '0.1.0') { throw 'The full delivery observation changed its mode or original failure.' }
Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $fullZero }
foreach ($change in @(@('browser','chrome'),@('passed',$true),@('stage','browser-cleanup'))) {
  $bad=$failedNavigation | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
}
$successNavigation=$valid | ConvertTo-Json | ConvertFrom-Json;$successNavigation | Add-Member -NotePropertyName edge_navigation_observation -NotePropertyValue $navigation
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $successNavigation 'chrome' }
$detail=$valid | ConvertTo-Json | ConvertFrom-Json;$detail.extension_identity_proof='detail-view';$detail.extension_card_identity_verified=$false;$detail.extension_details_identity_verified=$true
$detail | Add-Member -NotePropertyName owned_extension_folder_verified -NotePropertyValue $true
$detail | Add-Member -NotePropertyName detail_identity_stage -NotePropertyValue 'verified'
Assert-PickleExtensionStageReport (ConvertTo-PickleExtensionStageReport $detail) 'chrome'
foreach ($change in @(@('extension_identity_proof','manager'),@('extension_card_identity_verified',$true),@('extension_details_identity_verified',$false),@('owned_extension_folder_verified',$false),@('detail_identity_stage','path'))) {
  $bad=$detail | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { Assert-PickleExtensionStageReport $bad 'chrome' }
}
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
  title_records=@([ordered]@{flags=3;known_error_flags=0;bounds_nonempty=$false;bounds_intersect_window=$false;bounds_clipped=$false;control_type_id=50020;offscreen=$false;automation_id_flags=4;ancestor_role_ids=@(50020,50026,50032);ancestor_automation_id_flags=@(4,2,1)})
  developer_candidate_count=1;developer_records_truncated=$false;toggle_count=1;checkbox_count=0;button_count=0
  developer_records=@([ordered]@{name_flags=0;help_flags=0;labeled_by_flags=15;labeled_by_bound=$true;control_type_id=50002;enabled=$true;offscreen=$false;toggle_supported=$true;toggle_state=1;invoke_supported=$false})
  owned_window_count=1;window_records_truncated=$false
  window_records=@([ordered]@{class_kind=2;original_window=$false;owner_is_original=$true;root_is_original=$false;foreground=$true;known_error_count=1;scan_nodes=4;scan_truncated=$false;scan_unavailable=$false})
}
$failureObserved=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
$clean=ConvertTo-PickleExtensionFailureObservation $failureObserved
if ($clean.title_records[0].flags -ne 3 -or $clean.title_records[0].ancestor_role_ids.Count -ne 3 -or $clean.title_records[0].automation_id_flags -ne 4 -or ($clean.title_records[0].ancestor_automation_id_flags -join ',') -cne '4,2,1' -or $clean.developer_records[0].labeled_by_flags -ne 15 -or $clean.developer_records[0].name_flags -ne 0 -or $clean.window_records[0].known_error_count -ne 1) { throw 'Closed scalar mapping observations were changed.' }
$partial=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json
$partial.incomplete=$true;$partial.owned_window_count=0;$partial.window_records=@()
$partialClean=ConvertTo-PickleExtensionFailureObservation $partial
if (!$partialClean.incomplete -or $partialClean.title_records[0].flags -ne 3 -or $partialClean.developer_records[0].labeled_by_flags -ne 15 -or $partialClean.window_records.Count -ne 0) { throw 'Completed primary records were lost from a partial failure observation.' }
$diagnosed=$failure | ConvertTo-Json | ConvertFrom-Json
$diagnosed.stage='developer-mode';$diagnosed.failure='developer-toggle-unavailable'
$diagnosed | Add-Member -NotePropertyName failure_observation -NotePropertyValue $failureObserved
$null=ConvertTo-PickleExtensionStageReport $diagnosed
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $diagnosed 'edge' }
$detailFailure=$diagnosed | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$detailFailure.stage='loaded-extension-details';$detailFailure.failure='detail-identity-unavailable'
$detailFailure | Add-Member -NotePropertyName detail_identity_stage -NotePropertyValue 'path'
$detailFailureClean=ConvertTo-PickleExtensionStageReport $detailFailure
if ($detailFailureClean.detail_identity_stage -cne 'path' -or $null -eq $detailFailureClean.failure_observation -or $detailFailureClean.failure -cne 'detail-identity-unavailable') { throw 'New detail refusal lost its bounded phase or existing failure diagnostics.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $detailFailure 'edge' }
$policyRecords=@(foreach ($hive in @('HKCU','HKLM')) { foreach ($view in @(32,64)) { foreach ($policy in @('ExtensionDeveloperModeSettings','DeveloperToolsAvailability')) { [ordered]@{hive=$hive;view=$view;policy=$policy;state='absent'} } } })
$policies=[ordered]@{effective_policy_claimed=$false;incomplete=$false;records=$policyRecords} | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$policyClean=ConvertTo-PickleExtensionEdgePolicies $policies
if ($policyClean.records.Count -ne 8 -or $policyClean.effective_policy_claimed) { throw 'Closed registry presence was changed or interpreted as effective policy.' }
foreach ($state in @('absent','known-0','known-1','known-2','unexpected-type','unexpected-value','unavailable')) {
  $mapped=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$mapped.records[1].state=$state
  if ((ConvertTo-PickleExtensionEdgePolicies $mapped).records[1].state -cne $state) { throw 'A closed Edge policy category was lost.' }
}
foreach ($change in @(@('effective_policy_claimed',$true),@('effective_policy_claimed','false'),@('incomplete',1),@('records',@()),@('records',@($policies.records[0])))) {
  $bad=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionEdgePolicies $bad }
}
foreach ($change in @(@('hive','HKCR'),@('view',128),@('view','32'),@('view',32.0),@('policy','UnreviewedPolicy'),@('state','enabled'),@('state',0),@('state','known-2'))) {
  $bad=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$bad.records[0].($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionEdgePolicies $bad }
}
$bad=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$bad.records[7]=$bad.records[0]
Assert-ExtensionRefused { ConvertTo-PickleExtensionEdgePolicies $bad }
$bad=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$bad.records=@($bad.records)+@($bad.records[0])
Assert-ExtensionRefused { ConvertTo-PickleExtensionEdgePolicies $bad }
$bad=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$bad.records[0] | Add-Member -NotePropertyName raw_value -NotePropertyValue 'not-retained'
Assert-ExtensionRefused { ConvertTo-PickleExtensionEdgePolicies $bad }
$bad=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$bad | Add-Member -NotePropertyName registry_path -NotePropertyValue 'not-retained'
Assert-ExtensionRefused { ConvertTo-PickleExtensionEdgePolicies $bad }
$partialPolicies=$policies | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$partialPolicies.incomplete=$true
foreach ($record in $partialPolicies.records) { $record.state='unavailable' }
$null=ConvertTo-PickleExtensionEdgePolicies $partialPolicies
$policyFailure=$diagnosed | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$policyFailure | Add-Member -NotePropertyName edge_policy_observation -NotePropertyValue $partialPolicies
$cleanPolicyFailure=ConvertTo-PickleExtensionStageReport $policyFailure
if ($cleanPolicyFailure.failure -cne $diagnosed.failure -or !$cleanPolicyFailure.edge_policy_observation.incomplete) { throw 'Optional Edge presence observations replaced the primary failure.' }
Assert-ExtensionRefused { Assert-PickleExtensionStageReport $policyFailure 'edge' }
foreach ($change in @(@('passed',$true),@('browser','chrome'),@('stage','ordinary-navigation'))) {
  $bad=$policyFailure | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionStageReport $bad }
  if ($change[0] -ceq 'passed') { Assert-ExtensionRefused { Assert-PickleExtensionStageReport $bad 'edge' } }
}
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
foreach ($change in @(@('automation_id_flags',0),@('automation_id_flags',3),@('automation_id_flags',65),@('automation_id_flags',129),@('automation_id_flags',513),@('automation_id_flags',2),@('automation_id_flags','4'),@('ancestor_automation_id_flags',@(4,2)),@('ancestor_automation_id_flags',@(4,3,1)),@('ancestor_automation_id_flags',@(4,'card',1)),@('ancestor_automation_id_flags',@(4,2,1.0)))) {
  $bad=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json;$bad.title_records[0].($change[0])=$change[1]
  Assert-ExtensionRefused { ConvertTo-PickleExtensionFailureObservation $bad }
}
foreach ($flags in @(1,2,4,8,16,32,64,128,256,512)) {
  $mapped=$failureObserved | ConvertTo-Json -Depth 7 | ConvertFrom-Json;$mapped.title_records[0].automation_id_flags=$flags;$mapped.title_records[0].ancestor_automation_id_flags[0]=$flags
  $mappedClean=ConvertTo-PickleExtensionFailureObservation $mapped
  if ($mappedClean.title_records[0].automation_id_flags -ne $flags) { throw 'A closed author-ID category was lost.' }
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
    foreach ($mode in @('--self-test','--details-self-test','--layout-self-test','--edge-navigation-self-test')) {
      $child=Start-PickleReadingChild $exe @($mode)
      try { if (!$child.WaitForExit(10000) -or $child.ExitCode -ne 0) { throw 'Native fixture non-UI predicates failed.' } }
      finally { if (!$child.HasExited) { $child.Kill();$child.WaitForExit(10000) | Out-Null };$child.Dispose() }
    }
  } else {
    Assert-ExtensionRefused { Invoke-PickleExtensionSetupStage $package 'chrome' (Join-Path $root 'must-not-exist.json') }
    if (Test-Path -LiteralPath (Join-Path $root 'must-not-exist.json')) { throw 'Non-Windows acceptance touched native evidence.' }
  }
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
Write-Output 'PASS extension setup scope, refusal, inventory, non-forwarding proxy and native-free predicates.'
