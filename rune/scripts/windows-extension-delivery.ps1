# Separate hosted-only native delivery contract. Never weakens load-only scope.
. (Join-Path $PSScriptRoot 'windows-extension-smoke.ps1')

function ConvertTo-PickleDeliveryReaderReveal($Observation) {
  $bools=@('reader_observed','source_label_observed','control_verified','focused_control_verified','context_visible','preview_visible','scope_verified','unavailable')
  $keys=@('stage','home_events','page_down_events','polls')+$bools
  if (@($Observation.PSObject.Properties).Count -ne $keys.Count -or @($Observation.PSObject.Properties.Name | Where-Object { $_ -cnotin $keys }).Count -or $Observation.stage -isnot [string] -or $Observation.stage -cnotin @('route','control','scroll','complete')) { throw 'The reader reveal observation had unknown fields or stage.' }
  $clean=[ordered]@{stage=$Observation.stage}
  foreach ($key in $bools) { if ($Observation.$key -isnot [bool]) { throw 'A reader reveal flag was not boolean.' };$clean[$key]=$Observation.$key }
  foreach ($key in @('home_events','page_down_events','polls')) {
    $value=$Observation.$key
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt 0 -or $value -gt 150) { throw 'A reader reveal count was invalid.' }
    $clean[$key]=$value
  }
  if ($clean.home_events -notin @(0,2) -or $clean.page_down_events -gt 12 -or $clean.page_down_events%2 -ne 0 -or ($clean.page_down_events -gt 0 -and $clean.home_events -ne 2)) { throw 'Reader reveal exceeded its ordinary key action bound.' }
  if (($clean.control_verified -and (!$clean.reader_observed -or !$clean.source_label_observed)) -or ($clean.focused_control_verified -and !$clean.control_verified) -or ($clean.home_events -gt 0 -and !$clean.focused_control_verified)) { throw 'Reader keys were not bound to the owned reader control.' }
  if (($clean.source_label_observed -and !$clean.reader_observed) -or (($clean.scope_verified -or $clean.context_visible -or $clean.preview_visible) -and !$clean.reader_observed)) { throw 'Reader observations lacked their route proof.' }
  if ($clean.stage -ceq 'complete' -and (!$clean.reader_observed -or !$clean.source_label_observed -or !$clean.context_visible -or !$clean.preview_visible -or !$clean.scope_verified -or $clean.unavailable -or $clean.polls -eq 0)) { throw 'Reader reveal did not complete its route and visibility proof.' }
  return [pscustomobject]$clean
}
function ConvertTo-PickleChromeActionObservation($Observation) {
  $flags=@('incomplete','unavailable','before_article_verified','after_article_verified','truncated')
  $counts=@('before_nodes','after_nodes','nodes','controls','matching_count','extensions_exact_count','extension_title_exact_count','unknown_control_count','property_errors')
  $keys=@('stage','records')+$flags+$counts
  if (@($Observation.PSObject.Properties).Count -ne $keys.Count -or @($Observation.PSObject.Properties.Name | Where-Object { $_ -cnotin $keys }).Count -or $Observation.stage -isnot [string] -or $Observation.stage -cnotin @('before-ownership','before-article','controls','after-article','complete')) { throw 'The Chrome action observation had unknown fields or stage.' }
  $clean=[ordered]@{stage=$Observation.stage}
  foreach ($key in $flags) { if ($Observation.$key -isnot [bool]) { throw 'A Chrome action proof flag was not boolean.' };$clean[$key]=$Observation.$key }
  foreach ($key in $counts) { $value=$Observation.$key;if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt 0 -or $value -gt 512) { throw 'A Chrome action count exceeded its bound.' };$clean[$key]=$value }
  $records=@($Observation.records)
  if ($clean.matching_count -ne $clean.extensions_exact_count+$clean.extension_title_exact_count -or $clean.controls -ne $clean.matching_count+$clean.unknown_control_count -or $clean.controls -gt $clean.nodes -or $clean.nodes -gt $clean.before_nodes -or $records.Count -ne [Math]::Min(16,$clean.matching_count) -or $clean.truncated -ne ($clean.matching_count -gt 16)) { throw 'Chrome action counts disagreed.' }
  if ((!$clean.before_article_verified -and ($clean.nodes -ne 0 -or $clean.controls -ne 0)) -or ($clean.after_article_verified -and !$clean.before_article_verified)) { throw 'Chrome labels were inspected before exact article proof.' }
  if (($clean.stage -ceq 'complete' -or !$clean.incomplete -or !$clean.unavailable -or $clean.after_article_verified) -and ($clean.stage -cne 'complete' -or $clean.incomplete -or $clean.unavailable -or !$clean.before_article_verified -or !$clean.after_article_verified -or $clean.before_nodes -eq 0 -or $clean.after_nodes -eq 0 -or $clean.property_errors -ne 0)) { throw 'A partial Chrome action observation claimed completion.' }
  $recordKeys=@('role','extensions_exact','extension_title_exact','invoke_supported','toggle_supported','toggle_state','enabled','offscreen','bounds_flags','ancestor_roles','ancestors_truncated')
  $rows=@(foreach ($record in $records) {
    if (@($record.PSObject.Properties).Count -ne $recordKeys.Count -or @($record.PSObject.Properties.Name | Where-Object { $_ -cnotin $recordKeys }).Count) { throw 'A Chrome action record had unknown fields.' }
    $row=[ordered]@{}
    foreach ($key in @('extensions_exact','extension_title_exact','invoke_supported','toggle_supported','enabled','offscreen','ancestors_truncated')) { if ($record.$key -isnot [bool]) { throw 'A Chrome action record flag was not boolean.' };$row[$key]=$record.$key }
    if ($row.extensions_exact -eq $row.extension_title_exact) { throw 'A Chrome action record lacked one exact fixed label.' }
    foreach ($key in @('role','toggle_state','bounds_flags')) { $value=$record.$key;if (($value -isnot [int] -and $value -isnot [long])) { throw 'A Chrome action scalar was not an integer.' };$row[$key]=$value }
    if ($row.role -notin @(50000,50009,50011,50031) -or $row.bounds_flags -lt 0 -or $row.bounds_flags -gt 7 -or ($row.toggle_supported -and $row.toggle_state -notin @(0,1,2)) -or (!$row.toggle_supported -and $row.toggle_state -ne -1)) { throw 'A Chrome action scalar was outside its closed range.' }
    $ancestors=@($record.ancestor_roles)
    if ($ancestors.Count -lt 1 -or $ancestors.Count -gt 8 -or $ancestors[0] -ne $row.role) { throw 'Chrome action ancestry was invalid.' }
    foreach ($role in $ancestors) { if (($role -isnot [int] -and $role -isnot [long]) -or $role -lt 50000 -or $role -gt 50040 -or $role -eq 50030) { throw 'Chrome action ancestry entered document content or had an unknown role.' } }
    $row.ancestor_roles=$ancestors;[pscustomobject]$row
  })
  foreach ($pair in @(@('extensions_exact_count','extensions_exact'),@('extension_title_exact_count','extension_title_exact'))) {
    $seen=@($rows | Where-Object { $_.($pair[1]) }).Count
    if ($seen -gt $clean[$pair[0]] -or (!$clean.truncated -and $seen -ne $clean[$pair[0]])) { throw 'Chrome fixed-label counts disagreed with records.' }
  }
  $clean.records=$rows;return [pscustomobject]$clean
}
function ConvertTo-PickleExtensionDeliveryDriver($Report) {
  # Delivery-only observations are closed separately; all other fields
  # still pass through the unchanged common scalar allowlist.
  $scalars=[ordered]@{};$reveal=$null;$chromeAction=$null
  foreach ($field in $Report.PSObject.Properties) {
    if ($field.Name -ceq 'reader_reveal') { $reveal=ConvertTo-PickleDeliveryReaderReveal $field.Value }
    elseif ($field.Name -ceq 'chrome_action_observation') {
      if ($Report.browser -cne 'chrome' -or $Report.passed -isnot [bool] -or $Report.passed -or $Report.stage -cne 'delivery-context' -or $Report.failure -cne 'delivery-context-unavailable' -or $Report.native_host_registered -isnot [bool] -or !$Report.native_host_registered -or $Report.owned_registration_verified -isnot [bool] -or !$Report.owned_registration_verified -or $Report.owned_article_verified -isnot [bool] -or !$Report.owned_article_verified -or ($Report.delivery_shortcut_events -isnot [int] -and $Report.delivery_shortcut_events -isnot [long]) -or $Report.delivery_shortcut_events -ne 6 -or $Report.reader_reveal.reader_observed -isnot [bool] -or $Report.reader_reveal.reader_observed) { throw 'Chrome action discovery requires the exact failed full-delivery boundary.' }
      $chromeAction=ConvertTo-PickleChromeActionObservation $field.Value
    }
    else { $scalars[$field.Name]=$field.Value }
  }
  $clean=ConvertTo-PickleExtensionDriverScalars ([pscustomobject]$scalars) $true
  if ($null -ne $reveal) { $clean | Add-Member -NotePropertyName reader_reveal -NotePropertyValue $reveal }
  if ($null -ne $chromeAction) { $clean | Add-Member -NotePropertyName chrome_action_observation -NotePropertyValue $chromeAction }
  if (($clean.schema -isnot [int] -and $clean.schema -isnot [long]) -or $clean.schema -ne 1 -or $clean.test -cne 'windows-stable-extension-reference-delivery' -or $clean.scope -cne 'extension-native-reference-delivery' -or $clean.browser -cnotin @('chrome','edge') -or $clean.passed -isnot [bool]) { throw 'The delivery driver had the wrong closed scope.' }
  if (@($clean.PSObject.Properties.Name) -ccontains 'native_host_executed') { throw 'Delivery cannot invent direct native-host process observation.' }
  foreach ($key in @('browser_launch_attempted','browser_not_started_cleanup_verified','browser_job_drained','connect_attempted','disconnect_attempted','connect_notice_observed','disconnect_notice_observed','disconnect_no_match_notice_observed')) {
    if ($clean.$key -isnot [bool]) { throw 'Delivery omitted its actual browser lifetime state.' }
  }
  foreach ($key in @('application_visible_root_windows','application_eligible_reader_windows')) {
    if ($key -cin @($clean.PSObject.Properties.Name) -and ($clean.$key -lt 0 -or $clean.$key -gt 256)) { throw 'Delivery reader window counts exceeded their bound.' }
  }
  $windowCounts=@($clean.PSObject.Properties.Name | Where-Object { $_ -cin @('application_visible_root_windows','application_eligible_reader_windows') }).Count
  if ($windowCounts -eq 1 -or ($windowCounts -eq 2 -and $clean.application_eligible_reader_windows -gt $clean.application_visible_root_windows)) { throw 'Delivery reader window counts were inconsistent.' }
  return $clean
}
function Assert-PickleExtensionBrowserCleanup($Report) {
  $r=ConvertTo-PickleExtensionDeliveryDriver $Report
  if ($r.browser_not_started_cleanup_verified) {
    if ($r.browser_launch_attempted -or $r.browser_job_drained -or $r.connect_attempted -or $r.disconnect_attempted -or $r.passed -or $r.full_delivery_verified -isnot [bool] -or $r.full_delivery_verified -or $r.stage -cnotin @('arguments','delivery-app-ready') -or $r.atomic_job_membership_verified -eq $true -or $r.owned_window_verified -eq $true -or $r.native_host_registered -eq $true) { throw 'A browser constructor attempt or accepted delivery cannot claim not-started cleanup.' }
  } elseif (!$r.browser_launch_attempted -or !$r.browser_job_drained) { throw 'The attempted browser job did not prove complete termination.' }
}
function Assert-PickleExtensionDeliveryDriver($Report,[string]$Browser) {
  $r=ConvertTo-PickleExtensionDeliveryDriver $Report
  if ($r.browser -cne $Browser) { throw 'The delivery browser identity changed.' }
  if (!$r.browser_launch_attempted -or $r.browser_not_started_cleanup_verified) { throw 'Actual delivery requires a launched browser and real job drain.' }
  if (!$r.connect_attempted -or !$r.disconnect_attempted) { throw 'Actual delivery requires the ordinary Connect and Disconnect actions.' }
  if ($null -eq $r.reader_reveal -or $r.reader_reveal.stage -cne 'complete') { throw 'The reader context reveal was not verified.' }
  foreach ($key in @('passed','full_delivery_verified','atomic_job_membership_verified','owned_window_verified','owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified','developer_mode_enabled','load_unpacked_invoked','folder_picker_owned','folder_path_value_verified','folder_picker_closed','extension_loaded','browser_job_drained','app_launched','initial_compose_ready','native_host_registered','owned_registration_verified','owned_article_verified','context_preview_revealed','accepted_article_verified','native_host_route_verified','authenticated_bridge_verified','ordinary_disconnect_verified','ordinary_quit_verified')) {
    if ($r.$key -isnot [bool] -or !$r.$key) { throw 'The actual extension delivery chain did not complete.' }
  }
  if ($r.extension_identity_proof -cne 'authenticated-delivery' -or @($r.PSObject.Properties.Name | Where-Object { $_ -cin @('detail_identity_stage','extension_version') }).Count) { throw 'Full delivery requires actual authenticated admission, not list or details identity.' }
  foreach ($key in @('extension_card_identity_verified','extension_details_identity_verified','owned_extension_folder_verified','extension_name_found','extension_version_found')) {
    if ($r.$key -isnot [bool] -or $r.$key) { throw 'Authenticated delivery cannot invent card, detail or folder identity proof.' }
  }
  Assert-PickleExtensionRecoveredLayout $r
  foreach ($pair in @(@('navigation_shortcut_events',4),@('navigation_enter_events',2),@('navigation_confirm_shortcut_events',4),@('navigation_escape_events',2))) { if (($r.($pair[0]) -isnot [int] -and $r.($pair[0]) -isnot [long]) -or $r.($pair[0]) -ne $pair[1]) { throw 'The original ordinary browser navigation was incomplete.' } }
  foreach ($key in @('devtools_or_cdp_used','extension_code_modified','native_host_process_start_observed','inference_action_invoked','bridge_token_read','native_window_pixels_verified','selected_text_capture_verified','visible_url_verified')) {
    if ($r.$key -isnot [bool] -or $r.$key) { throw 'The delivery report claimed an unsupported action or evidence scope.' }
  }
  if (@($r.PSObject.Properties.Name | Where-Object { $_ -cin @('failure','error_kind','error_hresult','failure_observation','edge_policy_observation','window_capture','delivery_cleanup_unavailable') }).Count -or ($r.application_pid -isnot [int] -and $r.application_pid -isnot [long]) -or $r.application_pid -le 0 -or ($r.delivery_shortcut_events -isnot [int] -and $r.delivery_shortcut_events -isnot [long]) -or $r.delivery_shortcut_events -ne 6 -or $r.extension_id -isnot [string] -or $r.extension_id -cnotmatch '^[a-p]{32}$' -or $r.bundled_extension_version -cne '0.1.0') { throw 'The delivery report contained failure or incomplete identity evidence.' }
}
function ConvertTo-PickleExtensionZeroInference($State) {
  $keys=@('total_requests','show_count','chat_count','source_verified','context_verified','window_capture_required','marker_in_context','rejected','scenario','rejection_stage','rejection_ocr')
  if (@($State.PSObject.Properties).Count -ne $keys.Count -or @($State.PSObject.Properties.Name | Where-Object { $_ -cnotin $keys }).Count) { throw 'The no-inference observer contained unsupported fields.' }
  $counts=@('total_requests','show_count','chat_count')
  foreach ($key in $counts) { if (($State.$key -isnot [int] -and $State.$key -isnot [long]) -or $State.$key -lt 0 -or $State.$key -gt 16) { throw 'The reference-only observer had invalid request counters.' } }
  if ($State.chat_count -ne 0 -or $State.total_requests -ne $State.show_count) { throw 'The reference-only scenario made an inference or unclassified request.' }
  foreach ($key in @('source_verified','context_verified','window_capture_required','marker_in_context','rejected')) { if ($State.$key -isnot [bool] -or $State.$key) { throw 'The reference-only observer saw an unexpected request or capture.' } }
  if ($State.scenario -cne 'simplify' -or $null -ne $State.rejection_stage -or $null -ne $State.rejection_ocr -or @($State.PSObject.Properties).Count -ne 11) { throw 'The zero-inference observer had an unexpected state.' }
  # Preserve metadata discovery distinctly from inference; normally all zero.
  return [pscustomobject]@{total_requests=$State.total_requests;show_count=$State.show_count;chat_count=0;rejected=$false}
}
function Get-PickleExtensionRegistrationBaseline {
  Assert-PickleExtensionRegistryAbsent
  $result=[Collections.Generic.List[object]]::new()
  foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)) {
    $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,$view)
    try {
      foreach ($leaf in Get-PickleExtensionRegistryPaths) {
        $parts=$leaf.Split([char]92)
        for ($length=1;$length -le $parts.Length;$length++) {
          $path=($parts[0..($length-1)] -join '\');$key=$base.OpenSubKey($path,$false)
          try { $result.Add([pscustomobject]@{View=$view;Path=$path;Existed=($null -ne $key)}) } finally { if ($null -ne $key) { $key.Dispose() } }
        }
      }
    } finally { $base.Dispose() }
  }
  return ,$result
}
function Test-PickleExtensionRemoveNewKey([bool]$Existed,[bool]$Exists,[int]$Values,[int]$Subkeys) {
  if ($Values -lt 0 -or $Subkeys -lt 0) { throw 'Registry cleanup counts were invalid.' }
  if ($Existed -or !$Exists) { return $false }
  if ($Values -ne 0 -or $Subkeys -ne 0) { throw 'Unexpected registration state was preserved; cleanup was refused.' }
  return $true
}
function Restore-PickleExtensionRegistrationBaseline($Baseline) {
  # Only exact previously absent keys, deepest first. No recursive deletion,
  # policy write, unrelated host, value overwrite or foreign-state restoration.
  foreach ($record in @($Baseline | Sort-Object { $_.Path.Length } -Descending)) {
    $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,$record.View)
    try {
      $key=$base.OpenSubKey($record.Path,$false)
      try { $remove=Test-PickleExtensionRemoveNewKey $record.Existed ($null -ne $key) $(if ($null -ne $key) { $key.ValueCount } else { 0 }) $(if ($null -ne $key) { $key.SubKeyCount } else { 0 }) }
      finally { if ($null -ne $key) { $key.Dispose() } }
      if ($remove) { $base.DeleteSubKey($record.Path,$false) }
    } finally { $base.Dispose() }
  }
  Assert-PickleExtensionRegistryAbsent
  foreach ($record in $Baseline) {
    $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,$record.View)
    try { $key=$base.OpenSubKey($record.Path,$false);try { if (($null -ne $key) -ne $record.Existed) { throw 'Registration key existence did not return to baseline.' } } finally { if ($null -ne $key) { $key.Dispose() } } }
    finally { $base.Dispose() }
  }
}
function Assert-PickleExtensionDeliveryCleanup($Report,[string]$Browser) {
  if (($Report.schema -isnot [int] -and $Report.schema -isnot [long]) -or $Report.schema -ne 1 -or $Report.test -cne 'windows-stable-extension-article-delivery' -or $Report.scope -cne 'extension-native-reference-delivery' -or $Report.browser -cne $Browser -or $Report.passed -isnot [bool]) { throw 'The delivery owner report had the wrong scope.' }
  $driver=ConvertTo-PickleExtensionDeliveryDriver $Report.driver
  if ($driver.browser -cne $Browser) { throw 'The cleanup browser identity changed.' }
  Assert-PickleExtensionBrowserCleanup $driver
  foreach ($key in @('owned_processes_stopped','profile_restored','registry_restored','archives_unchanged','bundled_extension_unchanged','lease_released','bridge_descriptor_removed')) { if ($Report.$key -isnot [bool] -or !$Report.$key) { throw 'The delivery owner did not prove complete cleanup.' } }
  if ($Report.application_forced_stop -isnot [bool] -or ($Report.application_forced_stop -and ($Report.ordinary_exit -eq $true -or $Report.passed))) { throw 'Forced application cleanup cannot attest ordinary successful exit.' }
  if ($Report.cleanup_stage -isnot [string] -or $Report.cleanup_stage -cne 'complete') { throw 'The final host/registry/profile cleanup sequence did not complete.' }
  if (($Report.application_pid -isnot [int] -and $Report.application_pid -isnot [long]) -or $Report.application_pid -le 0 -or $driver.application_pid -ne $Report.application_pid) { throw 'The driver did not retain the owned application identity.' }
  if ($Report.inference -eq $null -or @($Report.inference.PSObject.Properties).Count -ne 4 -or $Report.inference.rejected -isnot [bool] -or $Report.inference.rejected) { throw 'The delivery workflow did not preserve zero inference.' }
  foreach ($key in @('total_requests','show_count','chat_count')) { if (($Report.inference.$key -isnot [int] -and $Report.inference.$key -isnot [long]) -or $Report.inference.$key -lt 0 -or $Report.inference.$key -gt 16) { throw 'A delivery request counter was invalid.' } }
  if ($Report.inference.total_requests -ne $Report.inference.show_count -or $Report.inference.chat_count -ne 0) { throw 'The delivery workflow made an inference or unclassified request.' }
  $null=ConvertTo-PickleExtensionProxyState $Report.proxy
}
function Assert-PickleExtensionDeliveryReport($Report,[string]$Browser) {
  Assert-PickleExtensionDeliveryCleanup $Report $Browser
  if (!$Report.passed -or $Report.full_delivery_verified -isnot [bool] -or !$Report.full_delivery_verified -or $Report.ordinary_exit -isnot [bool] -or !$Report.ordinary_exit) { throw 'Real native delivery was not verified.' }
  Assert-PickleExtensionDeliveryDriver $Report.driver $Browser
}
function Get-PickleExtensionStrictAttributes([string]$Path) {
  try { return [IO.File]::GetAttributes($Path) }
  catch {
    $error=$_.Exception
    for ($depth=0;$error -and $depth -lt 8;$depth++) {
      if ($error -is [IO.FileNotFoundException] -or $error -is [IO.DirectoryNotFoundException]) { return $null }
      $error=$error.InnerException
    }
    throw 'The owned native-host path was unreadable; absence was not proved.'
  }
}
function Assert-PickleExtensionFinalHostAbsent([string]$Support,[bool]$AppDead,[bool]$LeaseIdle) {
  if (!$AppDead -or !$LeaseIdle) { throw 'Final native-host absence requires the dead owned application and idle lease.' }
  $root=Assert-PickleReadingDirectory $Support
  $native=Join-Path $root 'NativeHost';$attributes=Get-PickleExtensionStrictAttributes $native
  if ($null -eq $attributes) { return }
  if (($attributes -band [IO.FileAttributes]::ReparsePoint) -or !($attributes -band [IO.FileAttributes]::Directory)) { throw 'The native-host path was unsafe; absence was not proved.' }
  foreach ($entry in [IO.Directory]::EnumerateFileSystemEntries($native)) { throw 'Native-host entries remain after application shutdown; recovery data was preserved.' }
}
function Get-PickleExtensionRegistryLeafState([Microsoft.Win32.RegistryHive]$Hive,[Microsoft.Win32.RegistryView]$View,[string]$Path) {
  if (!$IsWindows) { throw 'Native-host registry inspection requires Windows.' }
  $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive,$View)
  try {
    $key=$base.OpenSubKey($Path,$false)
    try { return [pscustomobject]@{Present=($null -ne $key);Values=$(if ($null -eq $key) { 0 } else { $key.ValueCount });Subkeys=$(if ($null -eq $key) { 0 } else { $key.SubKeyCount })} }
    finally { if ($null -ne $key) { $key.Dispose() } }
  } finally { $base.Dispose() }
}
function Assert-PickleExtensionPostExitRegistryState($Baseline,[ValidateSet('chrome','edge')][string]$Browser) {
  # Validate the complete original snapshot shape/order before permitting any
  # exception to key absence. Only a tested browser's newly created empty HKCU
  # leaf may remain after RegDeleteKeyValueW removed the default value.
  $expected=[Collections.Generic.List[object]]::new()
  $views=@([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)
  foreach ($view in $views) {
    foreach ($leaf in Get-PickleExtensionRegistryPaths) {
      $parts=$leaf.Split([char]92)
      for ($length=1;$length -le $parts.Length;$length++) { $expected.Add([pscustomobject]@{View=$view;Path=($parts[0..($length-1)] -join '\')}) }
    }
  }
  $rows=@($Baseline)
  if ($rows.Count -ne $expected.Count) { throw 'The original registration baseline was missing or incomplete.' }
  for ($index=0;$index -lt $expected.Count;$index++) {
    $row=$rows[$index];$keys=@($row.PSObject.Properties.Name)
    if ($keys.Count -ne 3 -or @($keys | Where-Object { $_ -cnotin @('View','Path','Existed') }).Count -or $row.View -isnot [Microsoft.Win32.RegistryView] -or $row.View -ne $expected[$index].View -or $row.Path -isnot [string] -or $row.Path -cne $expected[$index].Path -or $row.Existed -isnot [bool]) { throw 'The original registration baseline was unknown or unreadable.' }
  }
  $vendor=if ($Browser -ceq 'edge') { 'Microsoft\Edge' } else { 'Google\Chrome' }
  $allowed='SOFTWARE\'+$vendor+'\NativeMessagingHosts\com.pickle.reader'
  foreach ($view in $views) {
    $original=@($rows | Where-Object { $_.View -eq $view -and $_.Path -ceq $allowed })
    if ($original.Count -ne 1 -or $original[0].Existed) { throw 'The tested native-host leaf was not explicitly absent at baseline.' }
  }
  foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryHive]::LocalMachine)) {
    foreach ($view in $views) {
      foreach ($path in Get-PickleExtensionRegistryPaths) {
        $state=Get-PickleExtensionRegistryLeafState $hive $view $path;$keys=@($state.PSObject.Properties.Name)
        if ($keys.Count -ne 3 -or @($keys | Where-Object { $_ -cnotin @('Present','Values','Subkeys') }).Count -or $state.Present -isnot [bool]) { throw 'The post-exit registration state was unknown.' }
        foreach ($name in @('Values','Subkeys')) { if (($state.$name -isnot [int] -and $state.$name -isnot [long]) -or $state.$name -lt 0 -or $state.$name -gt [int]::MaxValue) { throw 'The post-exit registration counts were invalid.' } }
        if (!$state.Present) {
          if ($state.Values -ne 0 -or $state.Subkeys -ne 0) { throw 'Missing registration state had inconsistent counts.' }
        } elseif ($hive -ne [Microsoft.Win32.RegistryHive]::CurrentUser -or $path -cne $allowed -or $state.Values -ne 0 -or $state.Subkeys -ne 0) { throw 'Unexpected post-exit registration data was preserved; cleanup was refused.' }
      }
    }
  }
}
function Assert-PickleExtensionPostExitAbsence($Report,[string]$Support,[bool]$AppDead,[bool]$LeaseIdle,$Baseline,[ValidateSet('chrome','edge')][string]$Browser) {
  $Report.cleanup_stage='host-absence'
  Assert-PickleExtensionFinalHostAbsent $Support $AppDead $LeaseIdle
  $Report.cleanup_stage='registry-state'
  Assert-PickleExtensionPostExitRegistryState $Baseline $Browser
}
function Invoke-PickleExtensionDeliveryStage([string]$PackageDirectory,[ValidateSet('chrome','edge')][string]$Browser,[string]$EvidencePath) {
  if (!$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'Native reference delivery runs only on hosted Windows CI.' }
  if (Test-Path -LiteralPath $EvidencePath) { throw 'Delivery evidence must be new.' }
  $null=Assert-PickleReadingDirectory ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($EvidencePath)))
  $package=Assert-PickleReadingDirectory $PackageDirectory;$application=Join-Path $package 'PickleRune.exe';$support=Get-PickleReadingDataDirectory
  Assert-PickleExtensionHostFilesAbsent $support;$registry=Get-PickleExtensionRegistrationBaseline
  if ((Invoke-PickleReadingLease $application) -ne 0) { throw 'Delivery cannot replace a live application profile.' }
  $bundle=Get-PickleBundledExtension $package;$browserExe=Get-PickleExtensionBrowser $Browser
  $work=Join-Path ([IO.Path]::GetTempPath()) ('pickle-extension-delivery-'+[guid]::NewGuid().ToString('N'))
  $null=Assert-PickleReadingDirectory $work;New-Item -ItemType Directory -Path $work | Out-Null
  $profile=Join-Path $work 'browser-profile';New-Item -ItemType Directory -Path $profile | Out-Null
  $report=[ordered]@{schema=1;test='windows-stable-extension-article-delivery';scope='extension-native-reference-delivery';browser=$Browser;passed=$false;full_delivery_verified=$false;driver=$null;application_pid=$null;ordinary_exit=$false;application_forced_stop=$false;inference=$null;proxy=$null;owned_processes_stopped=$false;profile_restored=$false;registry_restored=$false;archives_unchanged=$false;bundled_extension_unchanged=$false;lease_released=$false;bridge_descriptor_removed=$false;cleanup_stage='not-started';recovery_directory=$work}
  $driver=$null;$app=$null;$proxy=$null;$observer=$null;$snapshot=$null;$failed=$false;$stage='prepare'
  try {
    [IO.File]::WriteAllText($EvidencePath,($report | ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
    $snapshot=Save-PickleReadingProfile $support (Join-Path $work 'backup')
    $fixture=Build-PickleExtensionFixture $work
    $contract=[ordered]@{before='BEFORE';selected='Café shuttle service may reduce waiting on this route in dry weather. Evidence from one route does not establish reliability in rain.';after='AFTER';answer='This response must never be requested.'}
    Initialize-PickleReadingArchives $snapshot $contract $fixture
    $contractPath=Join-Path $work 'contract.json';[IO.File]::WriteAllText($contractPath,($contract | ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    $python=(Get-Command python -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $observer=Start-PickleReadingChild $python @((Join-Path $PSScriptRoot 'windows-reading-provider.py'),'--contract',$contractPath,'--ready',(Join-Path $work 'observer-ready.json'),'--state',(Join-Path $work 'observer-state.json'),'--stop-file',(Join-Path $work 'observer-stop'))
    $observerInfo=Wait-PickleReadingReady $observer $python (Join-Path $work 'observer-ready.json')
    $proxy=Start-PickleReadingChild $python @((Join-Path $PSScriptRoot 'windows-extension-page-fixture.py'),'--ready',(Join-Path $work 'proxy-ready.json'),'--state',(Join-Path $work 'proxy-state.json'),'--stop-file',(Join-Path $work 'proxy-stop'))
    $proxyInfo=Wait-PickleReadingReady $proxy $python (Join-Path $work 'proxy-ready.json')
    foreach ($port in @($observerInfo.port,$proxyInfo.port)) { if (($port -isnot [int] -and $port -isnot [long]) -or $port -lt 1 -or $port -gt 65535) { throw 'An owned delivery server had an invalid port.' } }
    $settings=[ordered]@{version=1;data=[ordered]@{mode='Local';local_address=('http://127.0.0.1:'+$observerInfo.port);local_model='pickle-reading-fixture:local';same_device=$true;strict_offline=$true;modifier_gesture=$false;shortcut='ctrl+alt+p';automatic_menu=$false;history_enabled=$false;screen_context_enabled=$false;web_context_enabled=$true;jev_enabled=$false;cloud_consent=$false;jev_consent=$false;local_consent='';context_consent_destination='';streaming=$true;transcriber_executable='';transcriber_model='';local_vision_model='';exclusions=@()}}
    Write-PickleReadingAtomic (Join-Path $support 'settings.json') ([Text.UTF8Encoding]::new($false).GetBytes(($settings | ConvertTo-Json -Depth 5)))
    $stage='start-actual-application';$app=Start-PickleReadingChild $application @();$null=$app.Handle;$report.application_pid=$app.Id
    $stage='actual-extension-delivery';$driverEvidence=Join-Path $work 'driver.json'
    $driver=Start-PickleReadingChild $fixture @('deliver-reference','--browser',$Browser,'--executable',$browserExe,'--profile',$profile,'--extension',$bundle.Directory,'--proxy-port',([string]$proxyInfo.port),'--evidence',$driverEvidence,'--application',$application,'--application-pid',([string]$app.Id),'--application-start-ticks',([string]$app.StartTime.ToUniversalTime().Ticks),'--support',$support)
    $null=$driver.Handle
    if (!$driver.WaitForExit(120000)) { throw 'The owned delivery driver exceeded its deadline.' }
    if (Test-Path -LiteralPath $driverEvidence -PathType Leaf) { $report.driver=ConvertTo-PickleExtensionDeliveryDriver (Read-PickleReadingJson $driverEvidence) }
    if ($null -ne $report.driver -and $report.driver.application_pid -ne $app.Id) { throw 'The delivery driver application identity changed.' }
    if ($driver.ExitCode -ne 0) { throw 'The actual native reference delivery did not complete.' }
    Assert-PickleExtensionDeliveryDriver $report.driver $Browser
  } catch { $failed=$true;$report.failure_stage=$stage;$report.failure='The controlled native reference delivery did not complete.' }
  finally {
    try {
      $report.cleanup_stage='owned-processes'
      if ($null -ne $driver -and !$driver.HasExited) { $failed=$true;$driver.Kill();if (!$driver.WaitForExit(10000)) { throw 'The owned delivery driver did not stop.' } }
      if ($null -ne $app -and !$app.HasExited) { $failed=$true;$report.application_forced_stop=$true;$app.Kill();if (!$app.WaitForExit(10000)) { throw 'The owned delivery app did not stop.' } }
      foreach ($pair in @(@($proxy,'proxy-stop'),@($observer,'observer-stop'))) {
        $child=$pair[0];if ($null -ne $child -and !$child.HasExited) { [IO.File]::WriteAllText((Join-Path $work $pair[1]),'stop');if (!$child.WaitForExit(5000)) { $failed=$true;$child.Kill();if (!$child.WaitForExit(10000)) { throw 'An owned delivery server did not stop.' } } }
      }
      $browserCleanup=$false
      if ($null -ne $report.driver) { Assert-PickleExtensionBrowserCleanup $report.driver;$browserCleanup=$true }
      $report.owned_processes_stopped=($null -ne $driver -and $driver.HasExited -and $browserCleanup -and $null -ne $app -and $app.HasExited -and $null -ne $proxy -and $proxy.HasExited -and $null -ne $observer -and $observer.HasExited)
      if (!$report.owned_processes_stopped) { throw 'Complete owned process cleanup was not established.' }
      $report.ordinary_exit=($app.ExitCode -eq 0 -and $report.driver.ordinary_quit_verified -eq $true)
      $report.cleanup_stage='observer-state'
      if ($proxy.ExitCode -ne 0 -or $observer.ExitCode -ne 0) { throw 'An owned delivery observer exited unsuccessfully.' }
      $observerState=Read-PickleReadingJson (Join-Path $work 'observer-state.json')
      $counters=[ordered]@{}
      foreach ($key in @('total_requests','show_count','chat_count')) { $value=$observerState.$key;if (($value -is [int] -or $value -is [long]) -and $value -ge 0 -and $value -le 16) { $counters[$key]=$value } }
      if ($counters.Count -eq 3) { $report.observer_counts=[pscustomobject]$counters }
      $report.inference=ConvertTo-PickleExtensionZeroInference $observerState
      $report.proxy=ConvertTo-PickleExtensionProxyState (Read-PickleReadingJson (Join-Path $work 'proxy-state.json'))
      $report.cleanup_stage='lease'
      $report.lease_released=(Invoke-PickleReadingLease $application) -eq 0
      $report.bridge_descriptor_removed=!(Test-Path -LiteralPath (Join-Path $support 'BrowserBridge/connection.json'))
      if (!$report.lease_released -or !$report.bridge_descriptor_removed) { throw 'The application lease or bridge was still live.' }
      $report.cleanup_stage='archives'
      Assert-PickleReadingOwnedArchives $snapshot;$report.archives_unchanged=$true
      # Real Disconnect must have removed every host file/default value. Only
      # the newly created EMPTY registry keys are eligible for direct cleanup.
      Assert-PickleExtensionPostExitAbsence $report $support $app.HasExited $report.lease_released $registry $Browser
      $report.cleanup_stage='registry-baseline'
      Restore-PickleExtensionRegistrationBaseline $registry;$report.registry_restored=$true
      $report.cleanup_stage='profile-restoration'
      Restore-PickleReadingProfile $snapshot { $app.HasExited -and (Invoke-PickleReadingLease $application) -eq 0 };$report.profile_restored=$true
      $report.cleanup_stage='bundle'
      Assert-PickleExtensionFingerprint $bundle.Inventory (Get-PickleExtensionFingerprint $bundle.Directory);$report.bundled_extension_unchanged=$true
      $report.cleanup_stage='profile-directory'
      Remove-Item -LiteralPath $profile -Recurse -Force
      $report.cleanup_stage='complete'
      if (!$failed) { Assert-PickleExtensionDeliveryDriver $report.driver $Browser;$report.full_delivery_verified=$true;$report.passed=$true }
    } catch { $failed=$true;$report.passed=$false;$report.full_delivery_verified=$false;$report.cleanup_failure='Owned delivery cleanup or baseline verification did not complete.' }
    finally {
      foreach ($child in @($driver,$app,$proxy,$observer)) { if ($null -ne $child) { $child.Dispose() } }
      if ($report.passed) { try { Remove-Item -LiteralPath $work -Recurse -Force;$report.Remove('recovery_directory') } catch { $report.passed=$false;$report.full_delivery_verified=$false;$report.cleanup_failure='Owned delivery work removal did not complete.' } }
      try { [IO.File]::WriteAllText($EvidencePath,($report | ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false)) }
      catch { $report.passed=$false;$report.full_delivery_verified=$false;$failed=$true }
    }
  }
  if (!$report.passed) { throw 'The hosted actual native reference delivery failed; no delivery pass was claimed.' }
}
function Invoke-PickleExtensionDeliveryCore([string]$PackageDirectory,[ValidateSet('chrome','edge')][string]$Browser,[string]$EvidencePath) {
  if ($PSEdition -cne 'Core' -or !$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'Delivery delegation is limited to hosted Windows PowerShell 7.' }
  $self=[Diagnostics.Process]::GetCurrentProcess();try { $powerShell=$self.MainModule.FileName } finally { $self.Dispose() }
  $start=New-PickleExtensionCoreStart $powerShell (Join-Path $PSScriptRoot 'invoke-windows-extension-delivery.ps1') $PackageDirectory $Browser $EvidencePath
  $process=[Diagnostics.Process]::Start($start)
  try {
    $null=$process.Handle;$code=Wait-PickleExtensionCore $process
    try { $report=Read-PickleReadingJson $EvidencePath } catch { throw 'The delivery owner did not produce bounded readable evidence.' }
    Assert-PickleExtensionDeliveryCleanup $report $Browser
    if ($code -ne 0) { return $false }
    Assert-PickleExtensionDeliveryReport $report $Browser;return $true
  } finally { try { if (!$process.HasExited) { $process.Kill();if (!$process.WaitForExit(10000)) { throw 'The delivery owner did not stop.' } } } finally { $process.Dispose() } }
}
