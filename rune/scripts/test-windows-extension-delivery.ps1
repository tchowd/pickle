# Portable closed-contract/refusal tests. No browser, app, registry or vault.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-extension-delivery.ps1')
function Assert-DeliveryRefused([scriptblock]$Operation) { $refused=$false;try { & $Operation } catch { $refused=$true };if (!$refused) { throw 'Expected native-delivery safety refusal.' } }
function New-DeliveryRegistryBaseline {
  $rows=[Collections.Generic.List[object]]::new()
  foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)) {
    foreach ($leaf in Get-PickleExtensionRegistryPaths) {
      $parts=$leaf.Split([char]92)
      for ($length=1;$length -le $parts.Length;$length++) { $rows.Add([pscustomobject]@{View=$view;Path=($parts[0..($length-1)] -join '\');Existed=$false}) }
    }
  }
  return ,$rows
}
foreach ($name in @('windows-extension-delivery.ps1','invoke-windows-extension-delivery.ps1','test-windows-extension-delivery.ps1','windows-extension-smoke.ps1','test-windows-package.ps1')) {
  $tokens=$null;$errors=$null;$null=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $name),[ref]$tokens,[ref]$errors)
  if ($errors.Count) { throw 'A delivery script did not parse.' }
}
$trueFlags=@('passed','full_delivery_verified','atomic_job_membership_verified','owned_window_verified','owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified','developer_mode_enabled','load_unpacked_invoked','folder_picker_owned','folder_path_value_verified','folder_picker_closed','extension_card_identity_verified','extension_loaded','browser_job_drained','browser_launch_attempted','app_launched','initial_compose_ready','native_host_registered','owned_registration_verified','owned_article_verified','context_preview_revealed','accepted_article_verified','native_host_route_verified','authenticated_bridge_verified','ordinary_disconnect_verified','ordinary_quit_verified','connect_attempted','disconnect_attempted')
$falseFlags=@('devtools_or_cdp_used','extension_code_modified','native_host_process_start_observed','inference_action_invoked','bridge_token_read','native_window_pixels_verified','selected_text_capture_verified','visible_url_verified','browser_not_started_cleanup_verified')
$valid=[ordered]@{schema=1;test='windows-stable-extension-reference-delivery';scope='extension-native-reference-delivery';browser='chrome';stage='browser-cleanup';application_pid=42;delivery_shortcut_events=6;extension_id='abcdefghijklmnopabcdefghijklmnop';bundled_extension_version='0.1.0';navigation_shortcut_events=4;navigation_enter_events=2;navigation_confirm_shortcut_events=4;navigation_escape_events=2;extension_identity_proof='authenticated-delivery'}
# Actual accepted authenticated text is the proof, not candidate/list details.
$trueFlags=@($trueFlags | Where-Object { $_ -cne 'extension_card_identity_verified' })
$falseFlags+=@('extension_card_identity_verified','extension_details_identity_verified','owned_extension_folder_verified','extension_name_found','extension_version_found')
foreach ($key in $trueFlags) { $valid[$key]=$true };foreach ($key in $falseFlags) { $valid[$key]=$false }
$noticeFlags=@('connect_notice_observed','disconnect_notice_observed','disconnect_no_match_notice_observed')
foreach ($key in $noticeFlags) { $valid[$key]=$false }
$valid.reader_reveal=[ordered]@{stage='complete';reader_observed=$true;source_label_observed=$true;control_verified=$true;focused_control_verified=$true;home_events=2;page_down_events=2;context_visible=$true;preview_visible=$true;scope_verified=$true;polls=3;unavailable=$false}
$valid=$valid | ConvertTo-Json -Depth 5 | ConvertFrom-Json
Assert-PickleExtensionDeliveryDriver $valid 'chrome'
$alreadyVisible=$valid | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$alreadyVisible.reader_reveal.home_events=0;$alreadyVisible.reader_reveal.page_down_events=0;$alreadyVisible.reader_reveal.control_verified=$false;$alreadyVisible.reader_reveal.focused_control_verified=$false
Assert-PickleExtensionDeliveryDriver $alreadyVisible 'chrome'
foreach ($change in @(@('stage','unknown'),@('reader_observed',$false),@('source_label_observed',$false),@('context_visible',$false),@('preview_visible',$false),@('scope_verified',$false),@('focused_control_verified',$false),@('control_verified',$false),@('unavailable',$true),@('home_events',4),@('home_events',0),@('page_down_events',13),@('page_down_events',1),@('polls',151),@('polls',0),@('polls','3'),@('reader_observed','true'))) {
  $bad=$valid.reader_reveal | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-DeliveryRefused { ConvertTo-PickleDeliveryReaderReveal $bad }
}
$bad=$valid.reader_reveal | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member text 'untrusted context must never be retained'
Assert-DeliveryRefused { ConvertTo-PickleDeliveryReaderReveal $bad }
$bad=$valid.reader_reveal | ConvertTo-Json | ConvertFrom-Json;$bad.PSObject.Properties.Remove('scope_verified')
Assert-DeliveryRefused { ConvertTo-PickleDeliveryReaderReveal $bad }
$routeOnly=$valid | ConvertTo-Json -Depth 5 | ConvertFrom-Json
$routeOnly.reader_reveal.stage='scroll';$routeOnly.reader_reveal.context_visible=$false;$routeOnly.reader_reveal.preview_visible=$false;$routeOnly.reader_reveal.scope_verified=$false;$routeOnly.reader_reveal.unavailable=$true
$null=ConvertTo-PickleExtensionDeliveryDriver $routeOnly
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $routeOnly 'chrome' }
# Even completed reveal/route flags cannot substitute for exact article and
# authenticated admission, which remain required by the actual success gate.
$routeOnly=$valid | ConvertTo-Json -Depth 5 | ConvertFrom-Json;$routeOnly.accepted_article_verified=$false;$routeOnly.authenticated_bridge_verified=$false
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $routeOnly 'chrome' }
$chromeObservation=[pscustomobject]@{stage='complete';incomplete=$false;unavailable=$false;before_article_verified=$true;after_article_verified=$true;before_nodes=10;after_nodes=10;nodes=9;controls=5;matching_count=1;extensions_exact_count=1;extension_title_exact_count=0;unknown_control_count=4;property_errors=0;truncated=$false;records=@(
  [pscustomobject]@{role=50000;extensions_exact=$true;extension_title_exact=$false;invoke_supported=$true;toggle_supported=$false;toggle_state=-1;enabled=$true;offscreen=$false;bounds_flags=3;ancestor_roles=@(50000,50033,50032);ancestors_truncated=$false})}
$null=ConvertTo-PickleChromeActionObservation $chromeObservation
foreach ($change in @(@('stage','unknown'),@('incomplete',$true),@('unavailable',$true),@('before_article_verified',$false),@('after_article_verified',$false),@('before_nodes',0),@('nodes',513),@('controls',6),@('matching_count',2),@('unknown_control_count',3),@('extensions_exact_count','1'),@('property_errors',1),@('truncated',$true))) {
  $bad=$chromeObservation | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-DeliveryRefused { ConvertTo-PickleChromeActionObservation $bad }
}
foreach ($field in @('name','id','url','path','pixels')) {
  $bad=$chromeObservation | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.records[0] | Add-Member -NotePropertyName $field -NotePropertyValue 'not retained'
  Assert-DeliveryRefused { ConvertTo-PickleChromeActionObservation $bad }
}
foreach ($change in @(@('role',50020),@('toggle_state',0),@('extension_title_exact',$true),@('invoke_supported','true'),@('bounds_flags',8))) {
  $bad=$chromeObservation | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.records[0].($change[0])=$change[1]
  Assert-DeliveryRefused { ConvertTo-PickleChromeActionObservation $bad }
}
$bad=$chromeObservation | ConvertTo-Json -Depth 6 | ConvertFrom-Json;$bad.records[0].ancestor_roles=@(50000,50030,50032)
Assert-DeliveryRefused { ConvertTo-PickleChromeActionObservation $bad }
$unavailableChrome=$chromeObservation | ConvertTo-Json -Depth 6 | ConvertFrom-Json
$unavailableChrome.stage='before-article';$unavailableChrome.incomplete=$true;$unavailableChrome.unavailable=$true;$unavailableChrome.before_article_verified=$false;$unavailableChrome.after_article_verified=$false;$unavailableChrome.records=@()
foreach ($field in @('after_nodes','nodes','controls','matching_count','extensions_exact_count','extension_title_exact_count','unknown_control_count')) { $unavailableChrome.$field=0 }
$null=ConvertTo-PickleChromeActionObservation $unavailableChrome
$failedChrome=$valid | ConvertTo-Json -Depth 7 | ConvertFrom-Json
$failedChrome.passed=$false;$failedChrome.full_delivery_verified=$false;$failedChrome.stage='delivery-context';$failedChrome | Add-Member failure 'delivery-context-unavailable'
$failedChrome | Add-Member owned_article_verified $true -Force;$failedChrome | Add-Member delivery_shortcut_events 6 -Force
$failedChrome.reader_reveal=[pscustomobject]@{stage='route';reader_observed=$false;source_label_observed=$false;control_verified=$false;focused_control_verified=$false;home_events=0;page_down_events=0;context_visible=$false;preview_visible=$false;scope_verified=$false;polls=42;unavailable=$true}
$failedChrome | Add-Member chrome_action_observation $chromeObservation
$classifiedChrome=ConvertTo-PickleExtensionDeliveryDriver $failedChrome
if ($classifiedChrome.failure -cne 'delivery-context-unavailable' -or !$classifiedChrome.chrome_action_observation.after_article_verified -or $classifiedChrome.passed) { throw 'Chrome discovery changed the original failed delivery outcome.' }
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $classifiedChrome 'chrome' }
foreach ($change in @(@('browser','edge'),@('passed',$true),@('stage','delivery-connect'),@('failure','delivery-connect-unavailable'),@('native_host_registered',$false),@('owned_registration_verified',$false),@('owned_article_verified',$false),@('delivery_shortcut_events',5))) {
  $bad=$failedChrome | ConvertTo-Json -Depth 9 | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-DeliveryRefused { ConvertTo-PickleExtensionDeliveryDriver $bad }
}
$bad=$failedChrome | ConvertTo-Json -Depth 9 | ConvertFrom-Json;$bad.reader_reveal.reader_observed=$true
Assert-DeliveryRefused { ConvertTo-PickleExtensionDeliveryDriver $bad }
Assert-DeliveryRefused { ConvertTo-PickleExtensionStageReport $failedChrome }
# Complete product-written proof is sufficient without any UI status notice.
$noticeOnly=$valid | ConvertTo-Json | ConvertFrom-Json;$noticeOnly.connect_notice_observed=$true
foreach ($key in @('native_host_registered','owned_registration_verified')) {
  $bad=$noticeOnly | ConvertTo-Json | ConvertFrom-Json;$bad.$key=$false
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
foreach ($key in $noticeFlags) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.$key='true'
  Assert-DeliveryRefused { ConvertTo-PickleExtensionDeliveryDriver $bad }
}
Assert-DeliveryRefused { ConvertTo-PickleExtensionStageReport $valid }
Assert-DeliveryRefused { Assert-PickleExtensionStageReport $valid 'chrome' }
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $valid 'edge' }
$partial=$valid | ConvertTo-Json | ConvertFrom-Json
$partial.passed=$false;$partial.full_delivery_verified=$false;$partial.stage='delivery-disconnect'
$partial | Add-Member failure 'delivery-disconnect-unavailable'
$retained=ConvertTo-PickleExtensionDeliveryDriver $partial
if (!$retained.accepted_article_verified -or !$retained.authenticated_bridge_verified -or !$retained.extension_loaded -or $retained.extension_identity_proof -cne 'authenticated-delivery') { throw 'A later cleanup failure erased the already accepted authenticated route.' }
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $retained 'chrome' }
foreach ($proof in @('card','detail-view','candidate','')) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.extension_identity_proof=$proof
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
$bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member detail_identity_stage 'verified'
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
foreach ($key in $trueFlags) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.$key=$false
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
foreach ($key in $falseFlags) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.$key=$true
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
foreach ($change in @(@('schema','1'),@('passed','true'),@('delivery_shortcut_events',5),@('delivery_shortcut_events','6'),@('application_pid',0),@('navigation_enter_events',1),@('extension_id','not-a-public-id'),@('bundled_extension_version','0.1.1'),@('scope','bundled-extension-load-only'))) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
foreach ($key in @('extension_version','raw_source','native_host_executed','delivery_cleanup_unavailable','error_kind','error_hresult','failure')) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member -NotePropertyName $key -NotePropertyValue $false
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
$state=[pscustomobject]@{total_requests=0;show_count=0;chat_count=0;source_verified=$false;context_verified=$false;window_capture_required=$false;marker_in_context=$false;rejected=$false;scenario='simplify';rejection_stage=$null;rejection_ocr=$null}
$none=ConvertTo-PickleExtensionZeroInference $state
if ($none.chat_count -ne 0 -or $none.total_requests -ne 0) { throw 'No requests were changed into inference evidence.' }
$metadata=$state | ConvertTo-Json | ConvertFrom-Json;$metadata.total_requests=1;$metadata.show_count=1
$metadataClean=ConvertTo-PickleExtensionZeroInference $metadata
if ($metadataClean.total_requests -ne 1 -or $metadataClean.show_count -ne 1 -or $metadataClean.chat_count -ne 0) { throw 'Metadata discovery was conflated with inference.' }
foreach ($change in @(@('chat_count',1),@('total_requests',1),@('show_count',1),@('total_requests','0'),@('rejected',$true),@('source_verified',$true),@('rejection_stage','chat-fields'),@('window_capture_required',$true))) {
  $bad=$state | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-DeliveryRefused { ConvertTo-PickleExtensionZeroInference $bad }
}
$bad=$state | ConvertTo-Json | ConvertFrom-Json;$bad.PSObject.Properties.Remove('rejection_stage');$bad | Add-Member -NotePropertyName raw_request -NotePropertyValue 'must-not-persist'
Assert-DeliveryRefused { ConvertTo-PickleExtensionZeroInference $bad }
if (!(Test-PickleExtensionRemoveNewKey $false $true 0 0) -or (Test-PickleExtensionRemoveNewKey $true $true 0 0) -or (Test-PickleExtensionRemoveNewKey $false $false 0 0) -or (Test-PickleExtensionRemoveNewKey $true $true 3 2)) { throw 'Only previously absent empty registration keys can be removed.' }
Assert-DeliveryRefused { Test-PickleExtensionRemoveNewKey $false $true 1 0 }
Assert-DeliveryRefused { Test-PickleExtensionRemoveNewKey $false $true 0 1 }
Assert-DeliveryRefused { Test-PickleExtensionRemoveNewKey $false $true -1 0 }
$cleanupFlags=@('owned_processes_stopped','profile_restored','registry_restored','archives_unchanged','bundled_extension_unchanged','lease_released','bridge_descriptor_removed')
$outer=[ordered]@{schema=1;test='windows-stable-extension-article-delivery';scope='extension-native-reference-delivery';browser='chrome';passed=$true;full_delivery_verified=$true;driver=$valid;application_pid=42;ordinary_exit=$true;application_forced_stop=$false;cleanup_stage='complete';inference=$none;proxy=[pscustomobject]@{total_requests=1;pages_served=1;rejected_requests=0;forwarded_requests=0}}
foreach ($key in $cleanupFlags) { $outer[$key]=$true }
$outer=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json
Assert-PickleExtensionDeliveryReport $outer 'chrome'
foreach ($key in $cleanupFlags) {
  $bad=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.$key=$false
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryCleanup $bad 'chrome' }
}
$failed=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$failed.passed=$false;$failed.full_delivery_verified=$false;$failed.driver.passed=$false;$failed.driver.full_delivery_verified=$false
Assert-PickleExtensionDeliveryCleanup $failed 'chrome'
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryReport $failed 'chrome' }
$bad=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.inference.chat_count=1
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryReport $bad 'chrome' }
$bad=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.driver.browser_job_drained=$false
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryCleanup $bad 'chrome' }
$bad=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.application_pid=43
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryCleanup $bad 'chrome' }
$bad=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.ordinary_exit=$false
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryReport $bad 'chrome' }
$never=[pscustomobject]@{schema=1;test='windows-stable-extension-reference-delivery';scope='extension-native-reference-delivery';browser='chrome';passed=$false;full_delivery_verified=$false;application_pid=42;app_launched=$true;stage='delivery-app-ready';browser_launch_attempted=$false;browser_not_started_cleanup_verified=$true;browser_job_drained=$false;failure='delivery-app-window-ambiguous';delivery_cleanup_unavailable=$true;application_visible_root_windows=2;application_eligible_reader_windows=2;connect_attempted=$false;disconnect_attempted=$false;connect_notice_observed=$false;disconnect_notice_observed=$false;disconnect_no_match_notice_observed=$false}
Assert-PickleExtensionBrowserCleanup $never
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $never 'chrome' }
foreach ($change in @(@('browser_launch_attempted',$true),@('browser_job_drained',$true),@('browser_not_started_cleanup_verified',$false),@('browser_launch_attempted','false'),@('passed',$true),@('full_delivery_verified',$true),@('stage','launch-contained-browser'),@('application_visible_root_windows',257),@('application_eligible_reader_windows',-1))) {
  $bad=$never | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-DeliveryRefused { Assert-PickleExtensionBrowserCleanup $bad }
}
$bad=$never | ConvertTo-Json | ConvertFrom-Json;$bad | Add-Member atomic_job_membership_verified $true
Assert-DeliveryRefused { Assert-PickleExtensionBrowserCleanup $bad }
foreach ($key in @('browser_launch_attempted','browser_not_started_cleanup_verified','browser_job_drained')) {
  $bad=$never | ConvertTo-Json | ConvertFrom-Json;$bad.PSObject.Properties.Remove($key)
  Assert-DeliveryRefused { Assert-PickleExtensionBrowserCleanup $bad }
}
$bad=$never | ConvertTo-Json | ConvertFrom-Json;$bad.application_eligible_reader_windows=3
Assert-DeliveryRefused { Assert-PickleExtensionBrowserCleanup $bad }
$bad=$never | ConvertTo-Json | ConvertFrom-Json;$bad.PSObject.Properties.Remove('application_visible_root_windows')
Assert-DeliveryRefused { Assert-PickleExtensionBrowserCleanup $bad }
# Certified not-started permits cleanup proof, never a full delivery pass. The
# initial app failure and forced-vs-ordinary distinction remain intact.
$beforeBrowser=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$beforeBrowser.passed=$false;$beforeBrowser.full_delivery_verified=$false;$beforeBrowser.ordinary_exit=$false;$beforeBrowser.application_forced_stop=$true;$beforeBrowser.driver=$never
Assert-PickleExtensionDeliveryCleanup $beforeBrowser 'chrome'
if ($beforeBrowser.driver.failure -cne 'delivery-app-window-ambiguous' -or $beforeBrowser.driver.browser_job_drained) { throw 'Pre-launch cleanup changed the primary failure or fabricated a job drain.' }
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryReport $beforeBrowser 'chrome' }
$beforeBrowser.ordinary_exit=$true;Assert-DeliveryRefused { Assert-PickleExtensionDeliveryCleanup $beforeBrowser 'chrome' }
$bad=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.application_forced_stop=$true
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryReport $bad 'chrome' }
foreach ($stage in @('host-absence','registry-state','registry-baseline','private arbitrary message')) {
  $bad=$outer | ConvertTo-Json -Depth 8 | ConvertFrom-Json;$bad.cleanup_stage=$stage
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryCleanup $bad 'chrome' }
}
& {
  $mock=[pscustomobject]@{states=@{};calls=0;unreadable=$false}
  function Get-PickleExtensionRegistryLeafState($Hive,$View,$Path) {
    $mock.calls++;if ($mock.unreadable) { throw 'Owned registry read refused.' }
    $key="$Hive|$View|$Path"
    if ($mock.states.ContainsKey($key)) { return $mock.states[$key] }
    return [pscustomobject]@{Present=$false;Values=0;Subkeys=0}
  }
  $baseline=New-DeliveryRegistryBaseline
  Assert-PickleExtensionPostExitRegistryState $baseline 'chrome'
  if ($mock.calls -ne 12) { throw 'The post-exit proof did not inspect every fixed hive, view and browser leaf.' }
  $chrome='SOFTWARE\Google\Chrome\NativeMessagingHosts\com.pickle.reader'
  $edge='SOFTWARE\Microsoft\Edge\NativeMessagingHosts\com.pickle.reader'
  foreach ($browser in @('chrome','edge')) {
    $leaf=if ($browser -ceq 'chrome') { $chrome } else { $edge };$mock.states=@{}
    foreach ($view in @('Registry32','Registry64')) { $mock.states["CurrentUser|$view|$leaf"]=[pscustomobject]@{Present=$true;Values=0;Subkeys=0} }
    Assert-PickleExtensionPostExitRegistryState $baseline $browser
    # Aliased views and leaves already absent after a previous exact removal
    # remain valid, without treating another browser or hive as owned.
    $mock.states.Remove("CurrentUser|Registry32|$leaf")
    Assert-PickleExtensionPostExitRegistryState $baseline $browser
  }
  foreach ($key in @("LocalMachine|Registry32|$chrome","CurrentUser|Registry64|$edge")) {
    $mock.states=@{};$mock.states[$key]=[pscustomobject]@{Present=$true;Values=0;Subkeys=0}
    Assert-DeliveryRefused { Assert-PickleExtensionPostExitRegistryState $baseline 'chrome' }
  }
  foreach ($state in @(
    [pscustomobject]@{Present=$true;Values=1;Subkeys=0},[pscustomobject]@{Present=$true;Values=0;Subkeys=1},
    [pscustomobject]@{Present='true';Values=0;Subkeys=0},[pscustomobject]@{Present=$false;Values=1;Subkeys=0},
    [pscustomobject]@{Present=$true;Values=-1;Subkeys=0},[pscustomobject]@{Present=$true;Values=0;Subkeys=0;Unknown=$false}
  )) {
    $mock.states=@{};$mock.states["CurrentUser|Registry32|$chrome"]=$state
    Assert-DeliveryRefused { Assert-PickleExtensionPostExitRegistryState $baseline 'chrome' }
  }
  $mock.states=@{};$mock.unreadable=$true
  Assert-DeliveryRefused { Assert-PickleExtensionPostExitRegistryState $baseline 'chrome' }
  $mock.unreadable=$false
  Assert-DeliveryRefused { Assert-PickleExtensionPostExitRegistryState $null 'chrome' }
  foreach ($mutation in @('missing','duplicate','conflicting','wrong-view','wrong-path','nonbool','unknown')) {
    $bad=New-DeliveryRegistryBaseline;$index=0
    for ($i=0;$i -lt $bad.Count;$i++) { if ($bad[$i].Path -ceq $chrome) { $index=$i;break } }
    switch ($mutation) {
      'missing' { $bad.RemoveAt($index) }
      'duplicate' { $bad.Add($bad[$index]) }
      'conflicting' { $bad[$index].Existed=$true }
      'wrong-view' { $bad[$index].View=[Microsoft.Win32.RegistryView]::Default }
      'wrong-path' { $bad[$index].Path=$edge }
      'nonbool' { $bad[$index].Existed='false' }
      'unknown' { $bad[$index] | Add-Member Unexpected $false }
    }
    Assert-DeliveryRefused { Assert-PickleExtensionPostExitRegistryState $bad 'chrome' }
  }
}
# Execute the original strict preflight and final-after-baseline guard calls.
# Empty baseline skips native writes here; the unchanged final guard must run.
& {
  $strict=[pscustomobject]@{calls=0}
  function Assert-PickleExtensionRegistryAbsent { $strict.calls++;throw 'Strict key absence refused.' }
  Assert-DeliveryRefused { Get-PickleExtensionRegistrationBaseline }
  if ($strict.calls -ne 1) { throw 'Strict registration preflight was bypassed.' }
  Assert-DeliveryRefused { Restore-PickleExtensionRegistrationBaseline @() }
  if ($strict.calls -ne 2) { throw 'Strict final key absence after baseline restoration was bypassed.' }
}
$owned=Join-Path ([IO.Path]::GetTempPath()) ('pickle-final-host-proof-'+[guid]::NewGuid().ToString('N'))
if ($owned.StartsWith('/var/') -and (Test-Path -LiteralPath '/private/var')) { $owned='/private'+$owned }
New-Item -ItemType Directory -Path $owned | Out-Null
try {
  Assert-PickleExtensionFinalHostAbsent $owned $true $true
  Assert-DeliveryRefused { Assert-PickleExtensionFinalHostAbsent $owned $false $true }
  Assert-DeliveryRefused { Assert-PickleExtensionFinalHostAbsent $owned $true $false }
  $native=Join-Path $owned 'NativeHost';[IO.File]::WriteAllText($native,'owned wrong type')
  Assert-DeliveryRefused { Assert-PickleExtensionFinalHostAbsent $owned $true $true }
  Remove-Item -LiteralPath $native;New-Item -ItemType Directory -Path $native | Out-Null
  Assert-PickleExtensionFinalHostAbsent $owned $true $true
  & {
    param($Directory,$Native)
    $calls=[pscustomobject]@{registry=0;restore=0;denyRegistry=$false}
    function Get-PickleExtensionRegistryLeafState { $calls.registry++;if ($calls.denyRegistry) { throw 'Owned registry proof refused.' };return [pscustomobject]@{Present=$false;Values=0;Subkeys=0} }
    $baseline=New-DeliveryRegistryBaseline
    $receipt=[pscustomobject]@{cleanup_stage='not-started'}
    Assert-PickleExtensionPostExitAbsence $receipt $Directory $true $true $baseline 'chrome'
    if ($calls.registry -ne 12 -or $receipt.cleanup_stage -cne 'registry-state') { throw 'The post-exit registry state guard was skipped.' }
    # An earlier provisional/no-match result cannot cover a late Connect write.
    [IO.File]::WriteAllText((Join-Path $Native 'late-owned-host'),'owned bytes')
    Assert-DeliveryRefused { Assert-PickleExtensionPostExitAbsence $receipt $Directory $true $true $baseline 'chrome';$calls.restore++ }
    if ($calls.restore -ne 0 -or $calls.registry -ne 12 -or $receipt.cleanup_stage -cne 'host-absence') { throw 'Late host reappearance reached restoration.' }
    Remove-Item -LiteralPath (Join-Path $Native 'late-owned-host')
    $calls.denyRegistry=$true
    Assert-DeliveryRefused { Assert-PickleExtensionPostExitAbsence $receipt $Directory $true $true $baseline 'chrome';$calls.restore++ }
    if ($calls.restore -ne 0 -or $receipt.cleanup_stage -cne 'registry-state') { throw 'Registry state failure reached restoration.' }
  } $owned $native
  & {
    param($Directory)
    function Get-PickleExtensionStrictAttributes { throw 'Unreadable owned path.' }
    Assert-DeliveryRefused { Assert-PickleExtensionFinalHostAbsent $Directory $true $true }
  } $owned
} finally { Remove-Item -LiteralPath $owned -Recurse -Force }
# Entry rejects nonhosted/nonWindows invocation before containment or startup.
if (!$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
  Assert-DeliveryRefused { & (Join-Path $PSScriptRoot 'invoke-windows-extension-delivery.ps1') -PackageDirectory 'not-opened' -Browser chrome -EvidencePath 'not-opened' }
}
if ($IsWindows) {
  $work=Join-Path ([IO.Path]::GetTempPath()) ('pickle-delivery-pure-'+[guid]::NewGuid().ToString('N'));New-Item -ItemType Directory -Path $work | Out-Null
  try {
    $fixture=Build-PickleExtensionFixture $work
    & $fixture '--delivery-self-test'
    if ($LASTEXITCODE -ne 0) { throw 'Compiled native-free delivery predicates failed.' }
  } finally { Remove-Item -LiteralPath $work -Recurse -Force }
}
Write-Output 'PASS separate native-delivery scope, causal refusal, metadata/inference separation and empty-key cleanup predicates.'
