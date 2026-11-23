# Separate hosted-only native delivery contract. Never weakens load-only scope.
. (Join-Path $PSScriptRoot 'windows-extension-smoke.ps1')

function ConvertTo-PickleExtensionDeliveryDriver($Report) {
  $clean=ConvertTo-PickleExtensionDriverScalars $Report $true
  if (($clean.schema -isnot [int] -and $clean.schema -isnot [long]) -or $clean.schema -ne 1 -or $clean.test -cne 'windows-stable-extension-reference-delivery' -or $clean.scope -cne 'extension-native-reference-delivery' -or $clean.browser -cnotin @('chrome','edge') -or $clean.passed -isnot [bool]) { throw 'The delivery driver had the wrong closed scope.' }
  if (@($clean.PSObject.Properties.Name) -ccontains 'native_host_executed') { throw 'Delivery cannot invent direct native-host process observation.' }
  return $clean
}
function Assert-PickleExtensionDeliveryDriver($Report,[string]$Browser) {
  $r=ConvertTo-PickleExtensionDeliveryDriver $Report
  if ($r.browser -cne $Browser) { throw 'The delivery browser identity changed.' }
  foreach ($key in @('passed','full_delivery_verified','atomic_job_membership_verified','owned_window_verified','owned_browser_foreground_verified','address_focus_ancestry_verified','address_focus_known_name','address_focus_is_edit','address_focus_value_supported','navigation_value_verified','extensions_document_verified','committed_internal_uri_verified','developer_mode_enabled','load_unpacked_invoked','folder_picker_owned','folder_path_value_verified','folder_picker_closed','extension_card_identity_verified','extension_loaded','browser_job_drained','app_launched','initial_compose_ready','native_host_registered','owned_registration_verified','owned_article_verified','context_preview_revealed','accepted_article_verified','native_host_route_verified','authenticated_bridge_verified','ordinary_disconnect_verified','ordinary_quit_verified')) {
    if ($r.$key -isnot [bool] -or !$r.$key) { throw 'The actual extension delivery chain did not complete.' }
  }
  foreach ($pair in @(@('navigation_shortcut_events',4),@('navigation_enter_events',2),@('navigation_confirm_shortcut_events',4),@('navigation_escape_events',2))) { if (($r.($pair[0]) -isnot [int] -and $r.($pair[0]) -isnot [long]) -or $r.($pair[0]) -ne $pair[1]) { throw 'The original ordinary browser navigation was incomplete.' } }
  foreach ($key in @('devtools_or_cdp_used','extension_code_modified','native_host_process_start_observed','inference_action_invoked','bridge_token_read','native_window_pixels_verified','selected_text_capture_verified','visible_url_verified')) {
    if ($r.$key -isnot [bool] -or $r.$key) { throw 'The delivery report claimed an unsupported action or evidence scope.' }
  }
  if (@($r.PSObject.Properties.Name | Where-Object { $_ -cin @('failure','failure_observation','edge_policy_observation','window_capture','delivery_cleanup_unavailable') }).Count -or ($r.application_pid -isnot [int] -and $r.application_pid -isnot [long]) -or $r.application_pid -le 0 -or ($r.delivery_shortcut_events -isnot [int] -and $r.delivery_shortcut_events -isnot [long]) -or $r.delivery_shortcut_events -ne 6 -or $r.extension_id -isnot [string] -or $r.extension_id -cnotmatch '^[a-p]{32}$' -or $r.extension_version -cne '0.1.0') { throw 'The delivery report contained failure or incomplete identity evidence.' }
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
  if ($driver.browser -cne $Browser -or $driver.browser_job_drained -isnot [bool] -or !$driver.browser_job_drained) { throw 'The exact browser job did not prove complete termination.' }
  foreach ($key in @('owned_processes_stopped','profile_restored','registry_restored','archives_unchanged','bundled_extension_unchanged','lease_released','bridge_descriptor_removed')) { if ($Report.$key -isnot [bool] -or !$Report.$key) { throw 'The delivery owner did not prove complete cleanup.' } }
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
  $report=[ordered]@{schema=1;test='windows-stable-extension-article-delivery';scope='extension-native-reference-delivery';browser=$Browser;passed=$false;full_delivery_verified=$false;driver=$null;application_pid=$null;ordinary_exit=$false;inference=$null;proxy=$null;owned_processes_stopped=$false;profile_restored=$false;registry_restored=$false;archives_unchanged=$false;bundled_extension_unchanged=$false;lease_released=$false;bridge_descriptor_removed=$false;recovery_directory=$work}
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
      if ($null -ne $driver -and !$driver.HasExited) { $failed=$true;$driver.Kill();if (!$driver.WaitForExit(10000)) { throw 'The owned delivery driver did not stop.' } }
      if ($null -ne $app -and !$app.HasExited) { $failed=$true;$app.Kill();if (!$app.WaitForExit(10000)) { throw 'The owned delivery app did not stop.' } }
      foreach ($pair in @(@($proxy,'proxy-stop'),@($observer,'observer-stop'))) {
        $child=$pair[0];if ($null -ne $child -and !$child.HasExited) { [IO.File]::WriteAllText((Join-Path $work $pair[1]),'stop');if (!$child.WaitForExit(5000)) { $failed=$true;$child.Kill();if (!$child.WaitForExit(10000)) { throw 'An owned delivery server did not stop.' } } }
      }
      $report.owned_processes_stopped=($null -ne $driver -and $driver.HasExited -and $null -ne $report.driver -and $report.driver.browser_job_drained -eq $true -and $null -ne $app -and $app.HasExited -and $null -ne $proxy -and $proxy.HasExited -and $null -ne $observer -and $observer.HasExited)
      if (!$report.owned_processes_stopped) { throw 'Complete owned process cleanup was not established.' }
      $report.ordinary_exit=($app.ExitCode -eq 0 -and $report.driver.ordinary_quit_verified -eq $true)
      if ($proxy.ExitCode -ne 0 -or $observer.ExitCode -ne 0) { throw 'An owned delivery observer exited unsuccessfully.' }
      $observerState=Read-PickleReadingJson (Join-Path $work 'observer-state.json')
      $counters=[ordered]@{}
      foreach ($key in @('total_requests','show_count','chat_count')) { $value=$observerState.$key;if (($value -is [int] -or $value -is [long]) -and $value -ge 0 -and $value -le 16) { $counters[$key]=$value } }
      if ($counters.Count -eq 3) { $report.observer_counts=[pscustomobject]$counters }
      $report.inference=ConvertTo-PickleExtensionZeroInference $observerState
      $report.proxy=ConvertTo-PickleExtensionProxyState (Read-PickleReadingJson (Join-Path $work 'proxy-state.json'))
      $report.lease_released=(Invoke-PickleReadingLease $application) -eq 0
      $report.bridge_descriptor_removed=!(Test-Path -LiteralPath (Join-Path $support 'BrowserBridge/connection.json'))
      if (!$report.lease_released -or !$report.bridge_descriptor_removed) { throw 'The application lease or bridge was still live.' }
      Assert-PickleReadingOwnedArchives $snapshot;$report.archives_unchanged=$true
      # Real Disconnect must have removed every host file/default value. Only
      # the newly created EMPTY registry keys are eligible for direct cleanup.
      Assert-PickleExtensionHostFilesAbsent $support
      Restore-PickleExtensionRegistrationBaseline $registry;$report.registry_restored=$true
      Restore-PickleReadingProfile $snapshot { $app.HasExited -and (Invoke-PickleReadingLease $application) -eq 0 };$report.profile_restored=$true
      Assert-PickleExtensionFingerprint $bundle.Inventory (Get-PickleExtensionFingerprint $bundle.Directory);$report.bundled_extension_unchanged=$true
      Remove-Item -LiteralPath $profile -Recurse -Force
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
