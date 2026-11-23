# Dedicated hosted-only setup discovery. Stage one never launches Pickle or
# registers a host; actual extension-to-app delivery remains explicitly open.
. (Join-Path $PSScriptRoot 'windows-reading-smoke.ps1')

function Assert-PickleExtensionHostFilesAbsent([string]$Support) {
  $root=Assert-PickleReadingDirectory $Support
  $native=Join-Path $root 'NativeHost'
  if (Test-Path -LiteralPath $native) {
    Assert-PickleReadingDirectory $native | Out-Null
    if (@(Get-ChildItem -LiteralPath $native -Force).Count) { throw 'Preexisting native-host data was preserved; setup acceptance was refused.' }
  }
}
function Get-PickleExtensionRegistryPaths {
  foreach ($vendor in @('Google\Chrome','Microsoft\Edge','Chromium')) {
    "SOFTWARE\$vendor\NativeMessagingHosts\com.pickle.reader"
  }
}
function Assert-PickleExtensionRegistryAbsent {
  if (!$IsWindows) { throw 'Native-host registry inspection requires Windows.' }
  foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryHive]::LocalMachine)) {
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32,[Microsoft.Win32.RegistryView]::Registry64)) {
      $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view)
      try {
        foreach ($path in Get-PickleExtensionRegistryPaths) {
          $key=$base.OpenSubKey($path,$false)
          if ($null -ne $key) {
            try { throw 'A preexisting Pickle native-host registration was preserved; acceptance was refused.' }
            finally { $key.Dispose() }
          }
        }
      } finally { $base.Dispose() }
    }
  }
}
function Get-PickleExtensionFingerprint([string]$Directory) {
  # Unlike the reading harness's restoration inventory, this setup-only stage
  # excludes NO transient controls: it never launches the app or owns its data.
  $root=Assert-PickleReadingDirectory $Directory
  $files=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::OrdinalIgnoreCase)
  if (Test-Path -LiteralPath $root) {
    $pending=[Collections.Generic.Queue[string]]::new();$pending.Enqueue($root)
    $count=0;$total=0L
    while ($pending.Count) {
      foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
        $count++
        if ($count -gt 512 -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The observed inventory exceeded its bound or found a link.' }
        if ($item.PSIsContainer) { $pending.Enqueue($item.FullName);continue }
        $total+=$item.Length;if ($total -gt 64MB) { throw 'The observed inventory exceeded 64 MiB.' }
        $relative=ConvertTo-PickleReadingRelative ($item.FullName.Substring($root.TrimEnd([IO.Path]::DirectorySeparatorChar).Length+1))
        $files.Add($relative,$item)
      }
    }
  }
  $result=[Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
  $remaining=64MB
  foreach ($item in $files.GetEnumerator()) {
    $bytes=Read-PickleReadingBytes $item.Value.FullName $remaining;$remaining-=$bytes.Length
    $result.Add($item.Key,(Get-PickleReadingHash $bytes))
  }
  return ,$result
}
function Assert-PickleExtensionFingerprint($Before,$After) {
  if ($Before.Count -ne $After.Count) { throw 'An observed file inventory changed.' }
  foreach ($entry in $Before.GetEnumerator()) {
    if (!$After.ContainsKey($entry.Key) -or $entry.Value -cne $After[$entry.Key]) { throw 'An observed file changed.' }
  }
}
function Get-PickleBundledExtension([string]$PackageDirectory) {
  $package=Assert-PickleReadingDirectory $PackageDirectory
  foreach ($name in @('PickleRune.exe','pickle-browser-host.exe')) {
    $item=Get-Item -LiteralPath (Join-Path $package $name) -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -le 0 -or $item.Length -gt 512MB) { throw 'The staged application/native-host payload was invalid.' }
  }
  $extension=Assert-PickleReadingDirectory (Join-Path $package 'browser-extension')
  $inventory=Get-PickleExtensionFingerprint $extension
  $expected=@('manifest.json','background.js','extract.js','popup.js','popup.html','Readability.js','Readability-LICENSE.md','README.md')
  if ($inventory.Count -ne $expected.Count -or @($inventory.Keys | Where-Object { $_ -cnotin $expected }).Count) { throw 'The bundled extension inventory differed from the reviewed files.' }
  $manifest=Read-PickleReadingJson (Join-Path $extension 'manifest.json')
  if ($manifest.manifest_version -ne 3 -or $manifest.name -cne 'Pickle — page and video context' -or $manifest.version -cne '0.1.0' -or $manifest.action.default_popup -cne 'popup.html' -or $manifest.background.service_worker -cne 'background.js' -or @($manifest.permissions).Count -ne 3 -or @($manifest.permissions | Where-Object { $_ -cnotin @('activeTab','scripting','nativeMessaging') }).Count) { throw 'The bundled extension manifest differed from the reviewed development extension.' }
  return [pscustomobject]@{Directory=$extension;Inventory=$inventory;ManifestSha256=$inventory['manifest.json']}
}
function Get-PickleExtensionBrowser([ValidateSet('chrome','edge')][string]$Browser) {
  $tail=if ($Browser -eq 'chrome') { 'Google/Chrome/Application/chrome.exe' } else { 'Microsoft/Edge/Application/msedge.exe' }
  foreach ($name in @('ProgramW6432','PROGRAMFILES','PROGRAMFILES(X86)')) {
    $root=[Environment]::GetEnvironmentVariable($name)
    if ($root) { $path=Join-Path $root $tail;if (Test-Path -LiteralPath $path -PathType Leaf) { return [IO.Path]::GetFullPath($path) } }
  }
  throw 'The required installed Stable browser was unavailable.'
}
function Build-PickleExtensionFixture([string]$Directory) {
  $framework=Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319'
  $compiler=Join-Path $framework 'csc.exe'
  if (!(Test-Path -LiteralPath $compiler -PathType Leaf)) { throw 'The inbox Framework compiler was unavailable.' }
  $output=Join-Path $Directory 'BrowserExtensionFixture.exe'
  $references=@('System.dll','System.Core.dll','System.Web.Extensions.dll','WPF/WindowsBase.dll','WPF/UIAutomationClient.dll','WPF/UIAutomationTypes.dll') | ForEach-Object { Join-Path $framework $_ }
  foreach ($reference in $references) { if (!(Test-Path -LiteralPath $reference -PathType Leaf)) { throw 'A required inbox UI Automation reference was unavailable.' } }
  $arguments=@('/nologo','/langversion:5','/target:exe',"/out:$output")+@($references | ForEach-Object { "/reference:$_" })+@((Join-Path $PSScriptRoot '../native/windows/BrowserExtensionFixture.cs'))
  $compile=Start-PickleReadingChild $compiler $arguments
  try { if (!$compile.WaitForExit(30000) -or $compile.ExitCode -ne 0 -or !(Test-Path -LiteralPath $output -PathType Leaf)) { throw 'The owned extension fixture did not compile within its deadline.' } }
  finally { if (!$compile.HasExited) { $compile.Kill();$compile.WaitForExit(10000) | Out-Null };$compile.Dispose() }
  return $output
}
function Assert-PickleExtensionStageReport($Report,[string]$Browser) {
  if (@($Report.PSObject.Properties.Name) -ccontains 'failure_observation') { throw 'Failure observations cannot attest a successful loading stage.' }
  if (($Report.schema -isnot [int] -and $Report.schema -isnot [long]) -or $Report.schema -ne 1 -or $Report.test -cne 'windows-stable-bundled-extension-load' -or $Report.scope -cne 'bundled-extension-load-only' -or $Report.browser -cne $Browser) { throw 'The native setup stage report had the wrong scope.' }
  foreach ($key in @('passed','atomic_job_membership_verified','owned_window_verified','owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified','developer_mode_enabled','load_unpacked_invoked','folder_picker_owned','folder_path_value_verified','folder_picker_closed','extension_card_identity_verified','extension_loaded','browser_job_drained')) {
    if ($Report.$key -isnot [bool] -or !$Report.$key) { throw 'The native bundled-extension loading stage did not pass completely.' }
  }
  foreach ($item in @(@('navigation_shortcut_events',4),@('navigation_enter_events',2),@('navigation_confirm_shortcut_events',4),@('navigation_escape_events',2))) {
    $value=$Report.($item[0])
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -ne $item[1]) { throw 'The owned browser navigation input was incomplete.' }
  }
  foreach ($key in @('full_delivery_verified','native_host_registered','native_host_executed','authenticated_bridge_verified','app_launched','devtools_or_cdp_used','extension_code_modified')) {
    if ($Report.$key -isnot [bool] -or $Report.$key) { throw 'The setup-only report claimed an unverified feature or unsupported setup path.' }
  }
  if ($Report.extension_id -isnot [string] -or $Report.extension_id -cnotmatch '^[a-p]{32}$' -or $Report.extension_version -cne '0.1.0') { throw 'The native extension card did not attest an exact ID and version.' }
}
function ConvertTo-PickleExtensionDeveloperObservation($Observation) {
  $counters=@('nodes','property_errors','document_count','extensions_label_exact_count','extensions_document_exact_count','welcome_document_known_count','new_tab_document_known_count','address_edit_known_name_count','value_pattern_count','internal_uri_exact_count','internal_uri_without_slash_count','candidate_count')
  $keys=$counters+@('candidates_truncated','legacy_observed','candidates')
  if (@($Observation.PSObject.Properties).Count -ne $keys.Count -or @($Observation.PSObject.Properties.Name | Where-Object { $_ -cnotin $keys }).Count) { throw 'The bounded Developer mode observation had unknown fields.' }
  $clean=[ordered]@{}
  foreach ($key in $counters) {
    $value=$Observation.$key
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt 0 -or $value -gt 2048) { throw 'The owned observation counter exceeded its bound.' }
    $clean[$key]=$value
  }
  if ($clean.nodes -gt 1024 -or $Observation.candidates_truncated -isnot [bool] -or $Observation.candidates_truncated -ne ($clean.candidate_count -gt 16)) { throw 'The owned observation exceeded its node/candidate bound.' }
  $clean.candidates_truncated=$Observation.candidates_truncated
  if ($Observation.legacy_observed -isnot [bool] -or $Observation.legacy_observed) { throw 'The managed-only observation must not claim native Legacy support evidence.' };$clean.legacy_observed=$false
  $records=@($Observation.candidates)
  if ($records.Count -gt 16 -or $records.Count -gt $clean.candidate_count) { throw 'Too many Developer mode candidates were retained.' }
  $booleans=@('label_exact','label_casefold','label_trim_casefold','label_known_substring','enabled','offscreen','toggle_supported','invoke_supported')
  $numbers=@('control_type_id','toggle_state')
  $recordsClean=@(foreach ($record in $records) {
    if (@($record.PSObject.Properties).Count -ne $booleans.Count+$numbers.Count -or @($record.PSObject.Properties.Name | Where-Object { $_ -cnotin ($booleans+$numbers) }).Count) { throw 'The observed candidate had unknown fields.' }
    $item=[ordered]@{}
    foreach ($key in $booleans) { if ($record.$key -isnot [bool]) { throw 'An observed candidate flag had the wrong type.' };$item[$key]=$record.$key }
    foreach ($key in $numbers) { if (($record.$key -isnot [int] -and $record.$key -isnot [long]) -or $record.$key -lt -1 -or $record.$key -gt 4294967295L) { throw 'An observed candidate scalar had the wrong type or range.' };$item[$key]=$record.$key }
    if (!$item.label_known_substring -or $item.control_type_id -lt 50000 -or $item.control_type_id -gt 59999 -or ($item.toggle_supported -and $item.toggle_state -notin @(0,1,2)) -or (!$item.toggle_supported -and $item.toggle_state -ne -1)) { throw 'The observed candidate label/type/toggle scalars were inconsistent.' }
    [pscustomobject]$item
  })
  $clean.candidates=$recordsClean
  return [pscustomobject]$clean
}
function ConvertTo-PickleExtensionFailureObservation($Observation) {
  $counts=@('nodes','property_errors','title_candidate_count','developer_candidate_count','toggle_count','checkbox_count','button_count','owned_window_count')
  $keys=$counts+@('incomplete','title_records_truncated','developer_records_truncated','window_records_truncated','title_records','developer_records','window_records')
  if (@($Observation.PSObject.Properties).Count -ne $keys.Count -or @($Observation.PSObject.Properties.Name | Where-Object { $_ -cnotin $keys }).Count) { throw 'The failure observation had unknown fields.' }
  $clean=[ordered]@{}
  if ($Observation.incomplete -isnot [bool]) { throw 'The partial failure observation marker had the wrong type.' };$clean.incomplete=$Observation.incomplete
  foreach ($key in $counts) {
    $value=$Observation.$key
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt 0 -or $value -gt 1024) { throw 'A failure observation count was invalid.' }
    $clean[$key]=$value
  }
  foreach ($key in @('property_errors','title_candidate_count','developer_candidate_count','toggle_count','checkbox_count','button_count')) { if ($clean[$key] -gt $clean.nodes) { throw 'An observation count exceeded its owned nodes.' } }
  if ($clean.owned_window_count -gt 64) { throw 'The owned window inventory exceeded its bound.' }
  foreach ($group in @('title','developer','window')) {
    $count=if ($group -eq 'window') { $clean.owned_window_count } else { $clean[($group+'_candidate_count')] }
    $truncated=$Observation.($group+'_records_truncated');$records=@($Observation.($group+'_records'))
    if ($truncated -isnot [bool] -or $truncated -ne ($count -gt 8) -or $records.Count -ne [Math]::Min(8,$count)) { throw 'The failure records exceeded or contradicted their bound.' }
    $clean[($group+'_records_truncated')]=$truncated
    $clean[($group+'_records')]=@(foreach ($record in $records) {
      $bools=@();$ranges=@{};$extra=@()
      if ($group -eq 'title') { $bools=@('offscreen');$ranges=@{flags=@(1,31);control_type_id=@(50000,59999)};$extra=@('ancestor_role_ids') }
      elseif ($group -eq 'developer') { $bools=@('labeled_by_bound','enabled','offscreen','toggle_supported','invoke_supported');$ranges=@{name_flags=@(0,15);help_flags=@(0,15);labeled_by_flags=@(0,15);control_type_id=@(50000,59999);toggle_state=@(-1,2)} }
      else { $bools=@('original_window','owner_is_original','root_is_original','foreground','scan_truncated','scan_unavailable');$ranges=@{class_kind=@(0,2);known_error_count=@(0,128);scan_nodes=@(0,128)} }
      $recordKeys=@($bools)+@($ranges.Keys)+$extra
      if (@($record.PSObject.Properties).Count -ne $recordKeys.Count -or @($record.PSObject.Properties.Name | Where-Object { $_ -cnotin $recordKeys }).Count) { throw 'A failure record had unknown fields.' }
      $item=[ordered]@{}
      foreach ($key in $bools) { if ($record.$key -isnot [bool]) { throw 'A failure flag had the wrong type.' };$item[$key]=$record.$key }
      foreach ($key in $ranges.Keys) {
        $value=$record.$key;$range=$ranges[$key]
        if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt $range[0] -or $value -gt $range[1]) { throw 'A failure scalar had the wrong type or range.' };$item[$key]=$value
      }
      if ($group -eq 'title') {
        $roles=@($record.ancestor_role_ids)
        if (!$roles.Count -or $roles.Count -gt 8 -or $roles[0] -ne $item.control_type_id -or (($item.flags -band 1) -and !($item.flags -band 2))) { throw 'Title ancestry or flags were inconsistent.' }
        foreach ($role in $roles) { if (($role -isnot [int] -and $role -isnot [long]) -or $role -lt 50000 -or $role -gt 59999) { throw 'An ancestor role was invalid.' } };$item.ancestor_role_ids=$roles
      } elseif ($group -eq 'developer') {
        foreach ($key in @('name_flags','help_flags','labeled_by_flags')) { if ($item[$key] -notin @(0,8,12,14,15)) { throw 'A Developer label classification was invalid.' } }
        if ((!$item.labeled_by_bound -and $item.labeled_by_flags -ne 0) -or ($item.toggle_supported -and $item.toggle_state -notin @(0,1,2)) -or (!$item.toggle_supported -and $item.toggle_state -ne -1)) { throw 'Developer label ownership or pattern flags were inconsistent.' }
      } elseif ($item.known_error_count -gt $item.scan_nodes) { throw 'Error labels exceeded scanned owned nodes.' }
      [pscustomobject]$item
    })
  }
  return [pscustomobject]$clean
}
function ConvertTo-PickleExtensionStageReport($Report) {
  # Whitelist scalar diagnostic keys. The native helper never writes arbitrary
  # UI names/values or exception messages; no private paths enter artifacts.
  $booleans=@('passed','full_delivery_verified','native_host_registered','native_host_executed','authenticated_bridge_verified','app_launched','devtools_or_cdp_used','extension_code_modified','browser_job_drained','atomic_job_membership_verified','owned_window_verified','owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified','developer_toggle_found','developer_mode_enabled','load_unpacked_invoked','folder_picker_owned','folder_path_value_verified','folder_picker_closed','extension_name_found','extension_version_found','extension_card_identity_verified','extension_loaded','developer_observation_unavailable','failure_observation_unavailable')
  $numbers=@('schema','error_hresult','folder_field_candidates','extension_id_candidates','target_card_candidates','last_owned_uia_nodes','refused_uia_pid','last_owned_picker_windows','last_owned_browser_windows','navigation_shortcut_events','navigation_enter_events','navigation_confirm_shortcut_events','navigation_escape_events')
  $fixed=@{test=@('windows-stable-bundled-extension-load');scope=@('bundled-extension-load-only');browser=@('chrome','edge');stage=@('arguments','launch-contained-browser','owned-browser-window','ordinary-navigation','developer-mode','load-unpacked','owned-folder-picker','folder-picker-path','loaded-extension-card','browser-cleanup');error_kind=@('com','access-denied','io','argument','other')}
  $failures=@('requires-hosted-windows-ci','invalid-arguments','unsupported-browser','invalid-proxy-port','profile-not-empty','wrong-browser-executable','not-installed-stable-brand','invalid-bundled-manifest','unexpected-bundled-extension','stage-deadline','owned-browser-window-unavailable','navigation-window-not-owned','navigation-foreground-unavailable','owned-address-bar-unavailable','address-value-pattern-unavailable','navigation-value-not-applied','navigation-focus-or-value-changed','extensions-document-unavailable','committed-internal-uri-unavailable','extensions-document-changed','navigation-input-target-not-owned','navigation-input-key-already-held','navigation-input-incomplete','developer-toggle-unavailable','developer-toggle-pattern-unavailable','developer-mode-not-enabled','load-unpacked-unavailable','owned-folder-picker-unavailable','ambiguous-folder-field','folder-field-unavailable','folder-value-pattern-unavailable','folder-path-value-mismatch','select-folder-unavailable','folder-picker-did-not-close','ambiguous-target-card','ambiguous-extension-id','loaded-extension-id-unavailable','native-operation-failed','invalid-owned-directory','directory-reparse-point','invalid-regular-file','invalid-json-size','invalid-json-object','evidence-too-large','uia-element-not-owned','uia-window-not-owned','uia-node-bound','uia-label-bound','ambiguous-owned-control','control-not-actionable','invoke-pattern-unavailable','job-create-failed','job-limits-failed','attribute-size-failed','attribute-initialize-failed','job-list-failed','browser-create-failed','browser-job-membership-failed','invalid-browser-argument','ambiguous-owned-window')
  $clean=[ordered]@{}
  foreach ($field in $Report.PSObject.Properties) {
    $key=$field.Name;$value=$field.Value
    if ($key -ceq 'failure_observation') {
      if ($Report.passed -isnot [bool] -or $Report.passed -or $Report.stage -cnotin @('developer-mode','loaded-extension-card') -or $Report.failure -isnot [string]) { throw 'Failure diagnostics cannot attest success or another stage.' }
      $clean[$key]=ConvertTo-PickleExtensionFailureObservation $value
    }
    elseif ($key -cin @('developer_observation','page_observation_before_navigation','navigation_failure_observation')) { $clean[$key]=ConvertTo-PickleExtensionDeveloperObservation $value }
    elseif ($key -cin $booleans -and $value -is [bool]) { $clean[$key]=$value }
    elseif ($key -cin $numbers -and ($value -is [int] -or $value -is [long])) { $clean[$key]=$value }
    elseif ($fixed.ContainsKey($key) -and $value -cin $fixed[$key]) { $clean[$key]=$value }
    elseif ($key -ceq 'failure' -and $value -cin $failures) { $clean[$key]=$value }
    elseif ($key -ceq 'extension_id' -and $value -is [string] -and $value -cmatch '^[a-p]{32}$') { $clean[$key]=$value }
    elseif ($key -cin @('extension_version','browser_version') -and $value -is [string] -and $value -cmatch '^\d{1,5}\.\d{1,5}\.\d{1,5}(?:\.\d{1,5})?$') { $clean[$key]=$value }
    else { throw 'The owned stage report contained an unsupported diagnostic field.' }
  }
  return [pscustomobject]$clean
}
function ConvertTo-PickleExtensionProxyState($State) {
  $keys=@('total_requests','pages_served','rejected_requests','forwarded_requests')
  if (@($State.PSObject.Properties).Count -ne $keys.Count) { throw 'The owned proxy returned an invalid state.' }
  $clean=[ordered]@{}
  foreach ($key in $keys) {
    $value=$State.$key
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -lt 0 -or $value -gt 1000000) { throw 'The owned proxy returned invalid request counters.' }
    $clean[$key]=$value
  }
  if ($clean.forwarded_requests -ne 0 -or $clean.pages_served+$clean.rejected_requests -ne $clean.total_requests) { throw 'The owned proxy violated its non-forwarding contract.' }
  return [pscustomobject]$clean
}
function Test-PickleExtensionSetupGate([string]$Configuration,[bool]$HasIdentity,[string]$Edition,[bool]$Windows,[string]$Actions,[string]$Runner) {
  return $Configuration -ceq 'debug' -and !$HasIdentity -and $Edition -ceq 'Core' -and $Windows -and $Actions -ceq 'true' -and $Runner -ceq 'github-hosted'
}
function New-PickleExtensionCoreStart([string]$PowerShell,[string]$Entry,[string]$PackageDirectory,[string]$Browser,[string]$EvidencePath) {
  $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=$PowerShell;$start.UseShellExecute=$false
  foreach ($argument in @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$Entry,'-PackageDirectory',$PackageDirectory,'-Browser',$Browser,'-EvidencePath',$EvidencePath)) { $start.ArgumentList.Add($argument) }
  return $start
}
function Wait-PickleExtensionCore($Process,[int]$TimeoutMilliseconds=240000) {
  if ($TimeoutMilliseconds -lt 1 -or $TimeoutMilliseconds -gt 240000) { throw 'The extension owner deadline was invalid.' }
  $null=$Process.Handle
  if (!$Process.WaitForExit($TimeoutMilliseconds)) {
    # Kill only this retained original owner. Its non-inherited kill-on-close
    # job contains the fixture/browser/proxy; no PID reopening or name matching.
    $Process.Kill()
    if (!$Process.WaitForExit(10000)) { throw 'The timed-out extension owner did not terminate.' }
    throw 'The owned browser setup exceeded its four-minute deadline; no setup pass was claimed.'
  }
  return $Process.ExitCode
}
function Assert-PickleExtensionCleanupReport($Report,[string]$Browser) {
  if (($Report.schema -isnot [int] -and $Report.schema -isnot [long]) -or $Report.schema -ne 1 -or $Report.test -cne 'windows-stable-extension-setup-stage' -or $Report.scope -cne 'bundled-extension-load-only' -or $Report.browser -cne $Browser) { throw 'The delegated setup evidence had the wrong scope.' }
  if ($Report.passed -isnot [bool]) { throw 'The setup outcome was not a boolean.' }
  foreach ($key in @('app_profile_unchanged','owned_processes_stopped','temporary_profile_removed','bundled_extension_unchanged')) {
    if ($Report.$key -isnot [bool] -or !$Report.$key) { throw 'The delegated setup did not prove complete cleanup.' }
  }
  foreach ($key in @('full_delivery_verified','app_launched','app_profile_touched','native_host_registered','native_host_executed','authenticated_bridge_verified')) {
    if ($Report.$key -isnot [bool] -or $Report.$key) { throw 'The delegated setup claimed an unverified delivery path or touched app data.' }
  }
  if ($Report.manifest_sha256 -isnot [string] -or $Report.manifest_sha256 -cnotmatch '^[A-Fa-f0-9]{64}$') { throw 'The bundled manifest receipt was missing.' }
  $driver=ConvertTo-PickleExtensionStageReport $Report.driver
  if ($driver.schema -ne 1 -or $driver.test -cne 'windows-stable-bundled-extension-load' -or $driver.scope -cne 'bundled-extension-load-only' -or $driver.browser -cne $Browser -or $driver.browser_job_drained -isnot [bool] -or !$driver.browser_job_drained) { throw 'The owned browser job did not prove termination.' }
  foreach ($key in @('full_delivery_verified','native_host_registered','native_host_executed','authenticated_bridge_verified','app_launched','devtools_or_cdp_used','extension_code_modified')) {
    if ($driver.$key -isnot [bool] -or $driver.$key) { throw 'The native setup diagnostic contradicted its isolated setup scope.' }
  }
  $null=ConvertTo-PickleExtensionProxyState $Report.server
}
function Assert-PickleExtensionSetupReport($Report,[string]$Browser) {
  Assert-PickleExtensionCleanupReport $Report $Browser
  if (!$Report.passed) { throw 'The delegated extension setup did not pass.' }
  Assert-PickleExtensionStageReport (ConvertTo-PickleExtensionStageReport $Report.driver) $Browser
}
function Invoke-PickleExtensionCore([string]$PackageDirectory,[ValidateSet('chrome','edge')][string]$Browser,[string]$EvidencePath) {
  if ($PSEdition -cne 'Core' -or !$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'The browser setup delegate requires hosted Windows PowerShell 7.' }
  # Launch a NEW owner per browser. Calling the entry in-process would assign
  # the package runner itself to a second lifetime job and invalidate cleanup.
  $current=[Diagnostics.Process]::GetCurrentProcess()
  try { $powerShell=[IO.Path]::GetFullPath($current.MainModule.FileName) } finally { $current.Dispose() }
  $item=Get-Item -LiteralPath $powerShell -Force
  Assert-PickleReadingDirectory ([IO.Path]::GetDirectoryName($powerShell)) | Out-Null
  if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or [IO.Path]::GetFileName($powerShell) -ine 'pwsh.exe') { throw 'The browser delegate requires the exact current PowerShell 7 executable.' }
  if (Test-Path -LiteralPath $EvidencePath) { throw 'Delegated browser setup evidence must be new.' }
  $entry=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'invoke-windows-extension-smoke.ps1'))
  $process=[Diagnostics.Process]::Start((New-PickleExtensionCoreStart $powerShell $entry $PackageDirectory $Browser $EvidencePath))
  try {
    $exitCode=Wait-PickleExtensionCore $process
    $report=Read-PickleReadingJson $EvidencePath
    # A failed setup may proceed to the other browser ONLY after the retained
    # owner exited and strict evidence proves every owned process/data cleanup.
    Assert-PickleExtensionCleanupReport $report $Browser
    if ($exitCode -ne 0) { return $false }
    try { Assert-PickleExtensionSetupReport $report $Browser }
    catch { return $false }
    return $true
  } finally {
    try {
      if (!$process.HasExited) { $process.Kill();if (!$process.WaitForExit(10000)) { throw 'The owned extension delegate remained alive during cleanup.' } }
    } finally { $process.Dispose() }
  }
}
function Invoke-PickleExtensionSetupStage([string]$PackageDirectory,[ValidateSet('chrome','edge')][string]$Browser,[string]$EvidencePath) {
  if (!$IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'Native browser setup acceptance runs only on hosted Windows CI.' }
  if (Test-Path -LiteralPath $EvidencePath) { throw 'Extension setup evidence must be new.' }
  Assert-PickleReadingDirectory ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($EvidencePath))) | Out-Null
  $support=Get-PickleReadingDataDirectory
  Assert-PickleExtensionRegistryAbsent;Assert-PickleExtensionHostFilesAbsent $support
  $profileBefore=Get-PickleExtensionFingerprint $support
  $bundle=Get-PickleBundledExtension $PackageDirectory
  $executable=Get-PickleExtensionBrowser $Browser
  $work=Join-Path ([IO.Path]::GetTempPath()) ('pickle-extension-'+[guid]::NewGuid().ToString('N'))
  Assert-PickleReadingDirectory $work | Out-Null;New-Item -ItemType Directory -Path $work | Out-Null
  $profile=Join-Path $work 'owned-browser-profile';New-Item -ItemType Directory -Path $profile | Out-Null
  $report=[ordered]@{schema=1;test='windows-stable-extension-setup-stage';scope='bundled-extension-load-only';passed=$false;browser=$Browser;full_delivery_verified=$false;app_launched=$false;app_profile_touched=$false;app_profile_unchanged=$false;native_host_registered=$false;native_host_executed=$false;authenticated_bridge_verified=$false;driver=$null;server=$null;owned_processes_stopped=$false;temporary_profile_removed=$false;bundled_extension_unchanged=$false;manifest_sha256=$bundle.ManifestSha256}
  $driver=$null;$server=$null;$stage='compile-fixture';$failed=$false
  try {
    $fixture=Build-PickleExtensionFixture $work
    $stage='start-owned-proxy'
    $python=(Get-Command python -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $ready=Join-Path $work 'proxy-ready.json';$state=Join-Path $work 'proxy-state.json';$stop=Join-Path $work 'proxy-stop'
    $server=Start-PickleReadingChild $python @((Join-Path $PSScriptRoot 'windows-extension-page-fixture.py'),'--ready',$ready,'--state',$state,'--stop-file',$stop)
    $info=Wait-PickleReadingReady $server $python $ready
    if (($info.port -isnot [int] -and $info.port -isnot [long]) -or $info.port -lt 1 -or $info.port -gt 65535) { throw 'The owned proxy did not return a valid port.' }
    $stage='load-bundled-extension'
    $driverEvidence=Join-Path $work 'driver.json'
    $driver=Start-PickleReadingChild $fixture @('load','--browser',$Browser,'--executable',$executable,'--profile',$profile,'--extension',$bundle.Directory,'--proxy-port',([string]$info.port),'--evidence',$driverEvidence)
    $null=$driver.Handle
    if (!$driver.WaitForExit(120000)) { throw 'The owned browser extension driver exceeded its deadline.' }
    if (Test-Path -LiteralPath $driverEvidence -PathType Leaf) { $report.driver=ConvertTo-PickleExtensionStageReport (Read-PickleReadingJson $driverEvidence) }
    if ($driver.ExitCode -ne 0) { throw 'The owned native extension setup stage failed.' }
    Assert-PickleExtensionStageReport $report.driver $Browser
  } catch { $failed=$true;$report.failure_stage=$stage;$report.failure='Owned setup stage did not complete; full delivery remains unverified.' }
  finally {
    try {
      if ($null -ne $driver -and !$driver.HasExited) { $failed=$true;$driver.Kill();if (!$driver.WaitForExit(10000)) { throw 'Owned native driver did not stop.' } }
      if ($null -ne $server -and !$server.HasExited) { [IO.File]::WriteAllText((Join-Path $work 'proxy-stop'),'stop');if (!$server.WaitForExit(5000)) { $failed=$true;$server.Kill();if (!$server.WaitForExit(10000)) { throw 'Owned proxy did not stop.' } } }
      if ($null -ne $server -and $server.ExitCode -ne 0) { throw 'Owned proxy exited unsuccessfully.' }
      if ($null -ne $server -and (Test-Path -LiteralPath (Join-Path $work 'proxy-state.json'))) { $report.server=ConvertTo-PickleExtensionProxyState (Read-PickleReadingJson (Join-Path $work 'proxy-state.json')) }
      $report.owned_processes_stopped=($null -ne $driver -and $driver.HasExited -and $null -ne $report.driver -and $report.driver.browser_job_drained -eq $true -and $null -ne $server -and $server.HasExited)
      Assert-PickleExtensionRegistryAbsent;Assert-PickleExtensionHostFilesAbsent $support
      Assert-PickleExtensionFingerprint $profileBefore (Get-PickleExtensionFingerprint $support);$report.app_profile_unchanged=$true
      Assert-PickleExtensionFingerprint $bundle.Inventory (Get-PickleExtensionFingerprint $bundle.Directory);$report.bundled_extension_unchanged=$true
      if ($report.owned_processes_stopped) { $null=Get-PickleReadingFiles $work;Remove-Item -LiteralPath $profile -Recurse -Force;$report.temporary_profile_removed=$true }
      if (!$failed -and $report.owned_processes_stopped -and $report.temporary_profile_removed -and $report.app_profile_unchanged -and $report.bundled_extension_unchanged) { $report.passed=$true }
    } catch { $failed=$true;$report.passed=$false;$report.cleanup_failure='Owned cleanup or unchanged-profile verification failed.' }
    finally {
      foreach ($child in @($driver,$server)) { if ($null -ne $child) { $child.Dispose() } }
      if ($report.passed) {
        try { Remove-Item -LiteralPath $work -Recurse -Force }
        catch { $report.passed=$false;$report.cleanup_failure='Owned temporary-work removal failed.' }
      }
      if (!$report.passed) { $report.recovery_directory=$work }
      [IO.File]::WriteAllText($EvidencePath,($report | ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
    }
  }
  if (!$report.passed) { throw 'The hosted bundled-extension loading stage failed; no full-delivery pass was claimed.' }
}
