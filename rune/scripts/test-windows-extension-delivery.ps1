# Portable closed-contract/refusal tests. No browser, app, registry or vault.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-extension-delivery.ps1')
function Assert-DeliveryRefused([scriptblock]$Operation) { $refused=$false;try { & $Operation } catch { $refused=$true };if (!$refused) { throw 'Expected native-delivery safety refusal.' } }
foreach ($name in @('windows-extension-delivery.ps1','invoke-windows-extension-delivery.ps1','test-windows-extension-delivery.ps1','windows-extension-smoke.ps1','test-windows-package.ps1')) {
  $tokens=$null;$errors=$null;$null=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $name),[ref]$tokens,[ref]$errors)
  if ($errors.Count) { throw 'A delivery script did not parse.' }
}
$trueFlags=@('passed','full_delivery_verified','atomic_job_membership_verified','owned_window_verified','owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified','developer_mode_enabled','load_unpacked_invoked','folder_picker_owned','folder_path_value_verified','folder_picker_closed','extension_card_identity_verified','extension_loaded','browser_job_drained','app_launched','initial_compose_ready','native_host_registered','owned_registration_verified','owned_article_verified','context_preview_revealed','accepted_article_verified','native_host_route_verified','authenticated_bridge_verified','ordinary_disconnect_verified','ordinary_quit_verified')
$falseFlags=@('devtools_or_cdp_used','extension_code_modified','native_host_process_start_observed','inference_action_invoked','bridge_token_read','native_window_pixels_verified','selected_text_capture_verified','visible_url_verified')
$valid=[ordered]@{schema=1;test='windows-stable-extension-reference-delivery';scope='extension-native-reference-delivery';browser='chrome';stage='browser-cleanup';application_pid=42;delivery_shortcut_events=6;extension_id='abcdefghijklmnopabcdefghijklmnop';extension_version='0.1.0';navigation_shortcut_events=4;navigation_enter_events=2;navigation_confirm_shortcut_events=4;navigation_escape_events=2}
foreach ($key in $trueFlags) { $valid[$key]=$true };foreach ($key in $falseFlags) { $valid[$key]=$false }
$valid=$valid | ConvertTo-Json -Depth 5 | ConvertFrom-Json
Assert-PickleExtensionDeliveryDriver $valid 'chrome'
Assert-DeliveryRefused { ConvertTo-PickleExtensionStageReport $valid }
Assert-DeliveryRefused { Assert-PickleExtensionStageReport $valid 'chrome' }
Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $valid 'edge' }
foreach ($key in $trueFlags) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.$key=$false
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
foreach ($key in $falseFlags) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.$key=$true
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
foreach ($change in @(@('schema','1'),@('passed','true'),@('delivery_shortcut_events',5),@('delivery_shortcut_events','6'),@('application_pid',0),@('navigation_enter_events',1),@('extension_id','not-a-public-id'),@('extension_version','0.1.1'),@('scope','bundled-extension-load-only'))) {
  $bad=$valid | ConvertTo-Json | ConvertFrom-Json;$bad.($change[0])=$change[1]
  Assert-DeliveryRefused { Assert-PickleExtensionDeliveryDriver $bad 'chrome' }
}
foreach ($key in @('raw_source','native_host_executed','delivery_cleanup_unavailable')) {
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
$outer=[ordered]@{schema=1;test='windows-stable-extension-article-delivery';scope='extension-native-reference-delivery';browser='chrome';passed=$true;full_delivery_verified=$true;driver=$valid;application_pid=42;ordinary_exit=$true;inference=$none;proxy=[pscustomobject]@{total_requests=1;pages_served=1;rejected_requests=0;forwarded_requests=0}}
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
