# Portable profile/HTTP fixtures; Windows additionally compiles/runs the non-UI self-test.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-reading-smoke.ps1')
. (Join-Path $PSScriptRoot 'windows-reading-delegate.ps1')
. (Join-Path $PSScriptRoot 'windows-reading-containment.ps1')
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
  # Import only the pure evidence predicate, never the package installer body.
  # Execute its actual two production call sites against owned non-UI stubs;
  # a removed/incorrect switch or assertion must fail the positive identity case.
  $packageTokens=$null;$packageErrors=$null
  $packageAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'test-windows-package.ps1'),[ref]$packageTokens,[ref]$packageErrors)
  if ($packageErrors.Count) { throw 'The package acceptance script did not parse.' }
  $predicate=@($packageAst.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Assert-PicklePackageReadingEvidence'},$true))
  $readingCall=@($packageAst.FindAll({param($node) $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Invoke-PickleReadingCore'},$true))
  $reportCall=@($packageAst.FindAll({param($node) $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Assert-PicklePackageReadingEvidence'},$true))
  $oldProbe=@($packageAst.FindAll({param($node) $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Invoke-PickleWindowCaptureProbe'},$true))
  if ($predicate.Count -ne 1 -or $readingCall.Count -ne 4 -or $reportCall.Count -ne 4 -or $oldProbe.Count) { throw 'The actual reading gates or explicit-only legacy capture diagnostic wiring differed.' }
  . ([scriptblock]::Create($predicate[0].Extent.Text))
  $actualPackageCalls=[scriptblock]::Create($readingCall[0].Extent.Text+"`n"+$reportCall[0].Extent.Text)
  foreach ($required in @($false,$true)) {
    $validReport=[ordered]@{schema=1;test='windows-installed-reading-workflow';configuration='release';scenario='simplify';require_window_capture=$required;marker_in_context=$required;
      passed=$true;ordinary_exit=$true;lease_released=$true;control_removed=$true;restart_ordinary_exit=$true;restart_lease_released=$true;restart_control_removed=$true;saved_archive_verified=$true;saved_archive_unchanged=$true;profile_restored=$true;owned_children_stopped=$true;
      server=@{scenario='simplify';total_requests=2;show_count=1;chat_count=1;source_verified=$true;context_verified=$true;window_capture_required=$required;marker_in_context=$required;rejected=$false};
      driver=@{scenario='simplify';passed=$true;initial_compose_ready=$true;fixture_selection_verified=$true;chooser_source_verified=$true;consent_before_network_verified=$true;provider_source_verified=$true;source_retained_verified=$true;window_marker_painted=$true;window_marker_absent_from_accessibility=$true;completed_answer_verified=$true;saved_answer_verified=$true;ordinary_quit_verified=$true;app_exit_code=0};
      restart_driver=@{scenario='simplify';passed=$true;application_identity_verified=$true;saved_entry_verified=$true;saved_detail_verified=$true;saved_metadata_absence_verified=$true;ordinary_quit_verified=$true;app_exit_code=0}}
    $report=($validReport | ConvertTo-Json -Depth 5 | ConvertFrom-Json)
    & {
      param($Calls,$Report,[bool]$Mode)
      $application='owned-fixture-app.exe';$supportRoot='owned-fixture-profile';$readingEvidence='owned-fixture-report.json';$hasIdentity=$Mode;$Configuration='release'
      $observed=[pscustomobject]@{called=$false;required=$null;read=$false}
      function Invoke-PickleReadingCore([string]$Application,[string]$DataDirectory,[string]$EvidencePath,[string]$Configuration,[switch]$RequireWindowCapture) {
        if ($Application -cne 'owned-fixture-app.exe' -or $DataDirectory -cne 'owned-fixture-profile' -or $EvidencePath -cne 'owned-fixture-report.json' -or $Configuration -cne 'release') { throw 'The package did not call the exact installed reading workflow.' }
        $observed.called=$true;$observed.required=[bool]$RequireWindowCapture
      }
      function Read-PickleReadingJson([string]$Path) {
        if (!$observed.called -or $Path -cne 'owned-fixture-report.json') { throw 'The package did not read its actual reading evidence after the workflow.' }
        $observed.read=$true;return $Report
      }
      & $Calls
      if (!$observed.called -or !$observed.read -or $observed.required -ne $Mode) { throw 'The actual package call skipped its identity capture requirement or report gate.' }
    } $actualPackageCalls $report $required
    foreach ($change in @(
      @('schema',1.0),@('schema','1'),@('scenario','explain-may'),@('passed',$false),@('require_window_capture',(!$required)),@('marker_in_context',(!$required)),@('profile_restored',$false),@('saved_archive_verified',$false),@('saved_archive_unchanged',$false),@('restart_ordinary_exit',$false),
      @('server.window_capture_required',(!$required)),@('server.marker_in_context',(!$required)),@('server.context_verified',$false),@('server.total_requests',1),@('server.chat_count',2),@('server.rejected',$true),
      @('driver.passed',$false),@('driver.fixture_selection_verified',$false),@('driver.chooser_source_verified',$false),@('driver.consent_before_network_verified',$false),@('driver.source_retained_verified',$false),@('driver.window_marker_painted',$false),@('driver.window_marker_absent_from_accessibility',$false),@('driver.saved_answer_verified',$false),@('driver.app_exit_code',1),
      @('restart_driver.passed',$false),@('restart_driver.saved_detail_verified',$false),@('restart_driver.saved_metadata_absence_verified',$false),@('restart_driver.app_exit_code',1),@('marker_in_context','true')
    )) {
      $bad=$validReport | ConvertTo-Json -Depth 5 | ConvertFrom-Json
      $parts=$change[0].Split('.')
      if ($parts.Count -eq 2) { $bad.($parts[0]).($parts[1])=$change[1] } else { $bad.($parts[0])=$change[1] }
      Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $bad $required 'release' }
    }
    $missing=$validReport | ConvertTo-Json -Depth 5 | ConvertFrom-Json
    $missing.driver.PSObject.Properties.Remove('window_marker_painted')
    Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $missing $required 'release' }
    if (!$required) { Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $report $true 'release' } }
  }
  # The additional scenario must be guarded and executed before any browser
  # extension gate. Execute the exact package block with synthetic reports;
  # neither the installer nor app/native UI runs in these portable tests.
  $explainBlock=$readingCall[1].Parent.Parent.Parent
  if ($explainBlock -isnot [Management.Automation.Language.IfStatementAst] -or
      $explainBlock.Extent.EndOffset -ge $packageAst.Extent.Text.IndexOf(". (Join-Path `$PSScriptRoot 'windows-extension-smoke.ps1')")) { throw 'The additional Explain gate must precede extension setup.' }
  $explainReport=$validReport | ConvertTo-Json -Depth 5 | ConvertFrom-Json
  $explainReport.configuration='debug';$explainReport.require_window_capture=$false;$explainReport.marker_in_context=$false
  foreach ($node in @($explainReport,$explainReport.server,$explainReport.driver,$explainReport.restart_driver)) { $node.scenario='explain-may' }
  $explainReport.server.window_capture_required=$false;$explainReport.server.marker_in_context=$false
  foreach ($key in @('explain_before_base_verified','explain_term_value_verified','accepted_explain_turn_verified')) { $explainReport.driver | Add-Member $key $true }
  foreach ($key in @('saved_card_explain_metadata_verified','saved_detail_raw_term_verified')) { $explainReport.restart_driver | Add-Member $key $true }
  $explainReport.restart_driver.PSObject.Properties.Remove('saved_metadata_absence_verified')
  Assert-PicklePackageReadingEvidence $explainReport $false 'debug' -Scenario 'explain-may'
  foreach ($key in @('explain_before_base_verified','explain_term_value_verified','accepted_explain_turn_verified')) {
    $explainReport.driver.$key=$false
    Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $explainReport $false 'debug' -Scenario 'explain-may' }
    $explainReport.driver.$key=$true
  }
  foreach ($key in @('saved_card_explain_metadata_verified','saved_detail_raw_term_verified')) {
    $explainReport.restart_driver.$key=$false
    Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $explainReport $false 'debug' -Scenario 'explain-may' }
    $explainReport.restart_driver.$key=$true
  }
  Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $explainReport $false 'debug' }
  Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $explainReport $true 'debug' -Scenario 'explain-may' }
  $followReport=$explainReport | ConvertTo-Json -Depth 6 | ConvertFrom-Json
  foreach ($node in @($followReport,$followReport.server,$followReport.driver,$followReport.restart_driver)) { $node.scenario='follow-up-return-clear' }
  $followReport.server.total_requests=4;$followReport.server.show_count=2;$followReport.server.chat_count=2
  $followReport.server | Add-Member follow_up_verified $true
  foreach ($key in @('follow_up_question_value_verified','accepted_follow_up_verified','clear_follow_ups_verified','clear_base_source_draft_preserved')) { $followReport.driver | Add-Member $key $true }
  foreach ($item in @(@('follow_up_typed_events',((Get-PickleReadingFollowQuestion).Length*2)),@('follow_up_return_events',2),@('unsent_draft_typed_events',('Unsent Café draft — keep me'.Length*2)))) { $followReport.driver | Add-Member $item[0] $item[1] }
  foreach ($key in @('saved_card_follow_up_metadata_verified','saved_detail_raw_question_verified')) { $followReport.restart_driver | Add-Member $key $true }
  Assert-PicklePackageReadingEvidence $followReport $false 'debug' -Scenario 'follow-up-return-clear'
  foreach ($change in @(@('server.total_requests',2),@('server.show_count',1),@('server.chat_count',1),@('server.follow_up_verified',$false),@('server.follow_up_verified','true'),
      @('driver.follow_up_question_value_verified',$false),@('driver.accepted_follow_up_verified',$false),@('driver.clear_follow_ups_verified',$false),@('driver.clear_base_source_draft_preserved',$false),
      @('driver.follow_up_typed_events',2),@('driver.follow_up_return_events',1),@('driver.follow_up_return_events','2'),@('driver.unsent_draft_typed_events',0),
      @('restart_driver.saved_card_follow_up_metadata_verified',$false),@('restart_driver.saved_detail_raw_question_verified',$false))) {
    $bad=$followReport | ConvertTo-Json -Depth 6 | ConvertFrom-Json
    $parts=$change[0].Split('.');$bad.($parts[0]).($parts[1])=$change[1]
    Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $bad $false 'debug' -Scenario 'follow-up-return-clear' }
  }
  foreach ($path in @('server.follow_up_verified','driver.follow_up_question_value_verified','driver.accepted_follow_up_verified','driver.clear_follow_ups_verified','driver.clear_base_source_draft_preserved',
      'driver.follow_up_typed_events','driver.follow_up_return_events','driver.unsent_draft_typed_events','restart_driver.saved_card_follow_up_metadata_verified','restart_driver.saved_detail_raw_question_verified')) {
    $bad=$followReport | ConvertTo-Json -Depth 6 | ConvertFrom-Json
    $parts=$path.Split('.');$bad.($parts[0]).PSObject.Properties.Remove($parts[1])
    Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $bad $false 'debug' -Scenario 'follow-up-return-clear' }
  }
  Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $followReport $true 'debug' -Scenario 'follow-up-return-clear' }
  Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $followReport $false 'release' -Scenario 'follow-up-return-clear' }
  Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $followReport $false 'debug' -Scenario 'explain-may' }
  $copyReport=$validReport | ConvertTo-Json -Depth 5 | ConvertFrom-Json
  $copyReport.configuration='debug';$copyReport.require_window_capture=$false;$copyReport.marker_in_context=$false
  foreach ($node in @($copyReport,$copyReport.server,$copyReport.driver,$copyReport.restart_driver)) { $node.scenario='formatted-copy' }
  $copyReport.server.window_capture_required=$false;$copyReport.server.marker_in_context=$false
  $copyReport.driver | Add-Member native_copy_verified $true
  $copyReport.restart_driver | Add-Member saved_native_copy_verified $true
  foreach ($driver in @($copyReport.driver,$copyReport.restart_driver)) { $driver | Add-Member known_fixture_clipboard_cleared $true;$driver | Add-Member clipboard_sender_pid_verified $false }
  Assert-PicklePackageReadingEvidence $copyReport $false 'debug' -Scenario 'formatted-copy'
  foreach ($pair in @(@('driver','native_copy_verified'),@('restart_driver','saved_native_copy_verified'),@('driver','known_fixture_clipboard_cleared'),@('restart_driver','known_fixture_clipboard_cleared'),@('driver','clipboard_sender_pid_verified'),@('restart_driver','clipboard_sender_pid_verified'))) {
    $bad=$copyReport | ConvertTo-Json -Depth 5 | ConvertFrom-Json
    $bad.($pair[0]).($pair[1])=!$bad.($pair[0]).($pair[1])
    Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $bad $false 'debug' -Scenario 'formatted-copy' }
    $bad.($pair[0]).PSObject.Properties.Remove($pair[1])
    Assert-ReadingRefused { Assert-PicklePackageReadingEvidence $bad $false 'debug' -Scenario 'formatted-copy' }
  }
  foreach ($mode in @(@('debug',$false,'true','github-hosted',$true),@('debug',$true,'true','github-hosted',$false),@('release',$false,'true','github-hosted',$false),
      @('debug',$false,$null,$null,$false),@('debug',$false,'false','github-hosted',$false),@('debug',$false,'true','self-hosted',$false),
      @('debug',$false,'true',$null,$false),@('debug',$false,'TRUE','github-hosted',$false))) {
    & {
      param($Block,$Report,$FollowReport,$Mode)
      $Configuration=$Mode[0];$hasIdentity=$Mode[1];$application='owned-app';$supportRoot='owned-profile'
      $observed=[pscustomobject]@{called=0;read=0;next=$null}
      function Invoke-PickleReadingCore($Application,$DataDirectory,$EvidencePath,$Configuration,[string]$Scenario,[switch]$RequireWindowCapture) {
        $expectedScenario=@('explain-may','follow-up-return-clear')[$observed.called]
        $expectedFile=@('unpackaged-debug-native-smoke-explain-reading.json','unpackaged-debug-native-smoke-follow-up-reading.json')[$observed.called]
        if ($observed.called -ge 2 -or $Scenario -cne $expectedScenario -or $RequireWindowCapture -or $Application -cne 'owned-app' -or $DataDirectory -cne 'owned-profile' -or $Configuration -cne 'debug' -or (Split-Path $EvidencePath -Leaf) -cne $expectedFile -or $observed.called -ne $observed.read) { throw 'The package additional reading invocation differed.' }
        $observed.called++;$observed.next=$EvidencePath
      }
      function Read-PickleReadingJson($Path) { if ($observed.called -ne $observed.read+1 -or $Path -cne $observed.next) { throw 'Reading evidence read before its exact actual workflow.' };$observed.read++;if ($observed.read -eq 1) { return $Report };return $FollowReport }
      $priorActions=[Environment]::GetEnvironmentVariable('GITHUB_ACTIONS','Process')
      $priorRunner=[Environment]::GetEnvironmentVariable('RUNNER_ENVIRONMENT','Process')
      try {
        [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS',$Mode[2],'Process')
        [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT',$Mode[3],'Process')
        & ([scriptblock]::Create($Block.Extent.Text))
        $expectedCount=if ($Mode[4]) { 2 } else { 0 }
        if ($observed.called -ne $expectedCount -or $observed.read -ne $expectedCount) { throw 'An additional reading run was skipped or admitted for the wrong package or runner.' }
      } finally {
        [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS',$priorActions,'Process')
        [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT',$priorRunner,'Process')
      }
    } $explainBlock $explainReport $followReport $mode
  }
  foreach ($case in @(
    @('', '""'),
    @('plain', '"plain"'),
    @('C:\owned path\', '"C:\owned path\\"'),
    @('owned"argument', '"owned\"argument"'),
    @('owned\"argument', '"owned\\\"argument"')
  )) {
    if ((ConvertTo-PickleReadingWindowsArgument $case[0]) -cne $case[1]) { throw 'Desktop-compatible Windows argv quoting changed a structured argument.' }
  }
  $coreStart=New-PickleReadingCoreStart 'C:\Program Files\PowerShell\7\pwsh.exe' 'C:\owned script\reading.ps1' 'C:\owned app\PickleRune.exe' 'C:\owned data\' 'C:\owned evidence\reading.json' 'release'
  if ($coreStart.UseShellExecute -or $coreStart.FileName -cne 'C:\Program Files\PowerShell\7\pwsh.exe' -or $coreStart.Arguments -notlike '*"-Configuration" "release" "-Scenario" "simplify"' -or $coreStart.Arguments -notlike '*"C:\owned data\\"*') { throw 'The reading delegation must preserve its exact executable and named paths without a shell.' }
  $captureStart=New-PickleReadingCoreStart 'C:\Program Files\PowerShell\7\pwsh.exe' 'C:\owned script\reading.ps1' 'C:\owned app\PickleRune.exe' 'C:\owned data\' 'C:\owned evidence\reading.json' 'release' -RequireWindowCapture
  if ($coreStart.Arguments -like '*RequireWindowCapture*' -or $captureStart.Arguments -cne ($coreStart.Arguments+' "-RequireWindowCapture"')) { throw 'Only explicitly required window capture may reach the delegated acceptance.' }
  # Exercise the same Arguments transport with only a non-UI, test-owned script.
  $argumentProbe=Join-Path $root 'owned argv probe.ps1';$argumentResult=Join-Path $root 'owned argv result.json'
  [IO.File]::WriteAllText($argumentProbe,'param([string]$Application,[string]$DataDirectory,[string]$EvidencePath,[string]$Configuration,[switch]$RequireWindowCapture,[string]$Scenario); [IO.File]::WriteAllText($EvidencePath,(@($Application,$DataDirectory,$Configuration,[bool]$RequireWindowCapture,$Scenario) | ConvertTo-Json -Compress))')
  $powerShell=(Get-Process -Id $PID).Path
  $probeApplication='C:\owned Café app\PickleRune.exe';$probeData='C:\owned data\'
  foreach ($transportCase in @(@($false,'simplify'),@($true,'simplify'),@($false,'explain-may'),@($false,'follow-up-return-clear'),@($false,'formatted-copy'))) {
    $requireCapture=$transportCase[0];$scenario=$transportCase[1]
    $argumentProcess=[Diagnostics.Process]::Start((New-PickleReadingCoreStart $powerShell $argumentProbe $probeApplication $probeData $argumentResult 'release' -RequireWindowCapture:$requireCapture -Scenario $scenario))
    try {
      if (!$argumentProcess.WaitForExit(10000) -or $argumentProcess.ExitCode -ne 0) { throw 'The owned structured-argument transport probe failed.' }
      $transport=Read-PickleReadingJson $argumentResult
      if ($transport.Count -ne 5 -or $transport[0] -cne $probeApplication -or $transport[1] -cne $probeData -or $transport[2] -cne 'release' -or $transport[3] -isnot [bool] -or $transport[3] -ne $requireCapture -or $transport[4] -cne $scenario) { throw 'The delegate transport changed Unicode, spaces, trailing slashes, configuration, capture switch or closed scenario.' }
    } finally { if (!$argumentProcess.HasExited) { $argumentProcess.Kill();$argumentProcess.WaitForExit(10000) | Out-Null };$argumentProcess.Dispose() }
  }
  $primary='Original controlled startup failure.'
  $cleanup=[ordered]@{cleanup_categories=@()}
  Add-PickleReadingCleanupFailure $cleanup ([ref]$primary) 'fixture-forced-stop'
  Add-PickleReadingCleanupFailure $cleanup ([ref]$primary) 'fixture-exit-nonzero'
  if ($primary -cne 'Original controlled startup failure.' -or $cleanup.cleanup_categories.Count -ne 2) { throw 'Cleanup diagnostics overwrote the primary reading failure.' }
  try { Throw-PickleReadingFailure 'ready-pid-mismatch' } catch { if ((Get-PickleReadingFailureCategory $_) -cne 'ready-pid-mismatch') { throw 'Fixed readiness category was lost.' } }
  try { throw 'unexpected-sensitive-source' } catch { if ((Get-PickleReadingFailureCategory $_) -cne 'harness-operation-failed') { throw 'Unexpected exception content entered a failure category.' } }
  $attempts=[pscustomobject]@{count=0}
  Invoke-PickleReadingIdentityBarrier { $attempts.count++;if ($attempts.count -lt 3) { Throw-PickleReadingFailure 'owned-child-executable-unreadable' } } ([DateTime]::UtcNow.AddSeconds(2))
  if ($attempts.count -ne 3) { throw 'Only transient unreadable metadata should be retried before identity succeeds.' }
  foreach ($category in @('owned-child-exited','owned-child-executable-mismatch','harness-operation-failed')) {
    $attempts.count=0
    try {
      Invoke-PickleReadingIdentityBarrier { $attempts.count++;if ($category -eq 'harness-operation-failed') { throw 'unexpected-sensitive-source' };Throw-PickleReadingFailure $category } ([DateTime]::UtcNow.AddSeconds(2))
      throw 'Nontransient identity failure was accepted.'
    } catch {
      if ($attempts.count -ne 1 -or (Get-PickleReadingFailureCategory $_) -cne $category) { throw 'Exited, mismatched or unknown failures must not be retried.' }
    }
  }
  $attempts.count=0
  try {
    Invoke-PickleReadingIdentityBarrier { $attempts.count++;Throw-PickleReadingFailure 'owned-child-executable-unreadable' } ([DateTime]::UtcNow.AddMilliseconds(500))
    throw 'Unreadable metadata was accepted after its deadline.'
  } catch { if ($attempts.count -lt 1 -or (Get-PickleReadingFailureCategory $_) -cne 'owned-child-executable-unreadable') { throw 'Unreadable metadata deadline lost its fixed failure category.' } }
  try {
    Invoke-PickleReadingIdentityBarrier { Start-Sleep -Milliseconds 40 } ([DateTime]::UtcNow.AddMilliseconds(10))
    throw 'Identity success after the original deadline was accepted.'
  } catch { if ((Get-PickleReadingFailureCategory $_) -cne 'ready-timeout') { throw 'Late identity inspection did not enforce the original deadline.' } }
  $owned=Start-PickleReadingChild $python.Source @('-c','import sys; sys.exit(23)')
  try {
    if (!$owned.WaitForExit(5000)) { throw 'Owned non-UI exit fixture timed out.' }
    $state=[ordered]@{owned_processes=[ordered]@{}}
    Save-PickleReadingProcessState $state 'fixture' $owned $python.Source -BeforeCleanup
    if ($state.owned_processes.fixture.exit_code -ne 23 -or !$state.owned_processes.fixture.exited_before_cleanup -or $state.owned_processes.fixture.forced_cleanup) { throw 'Owned helper exit evidence was not preserved.' }
    try { Assert-PickleReadingChild $owned $python.Source;throw 'An exited helper passed readiness.' } catch { if ((Get-PickleReadingFailureCategory $_) -cne 'owned-child-exited') { throw 'An exited helper did not retain its fixed category.' } }
  } finally { if (!$owned.HasExited) { $owned.Kill();$owned.WaitForExit(5000) | Out-Null };$owned.Dispose() }
  $diagnostic=[pscustomobject]@{schema=1;test='windows-owned-selection-source';passed=$false;pid=17;stage='fixture-controls';failure='unexpected-fixture-error';error_kind='type-initialization';error_hresult=-2146233036;inner_error_kind='native-library-unavailable';inner_error_hresult=-2146233052}
  $clean=ConvertTo-PickleReadingFixtureFailure $diagnostic 17
  if ($clean.pid -ne 17 -or $clean.error_kind -cne 'type-initialization') { throw 'Whitelisted fixture diagnostic was not preserved.' }
  Assert-ReadingRefused { ConvertTo-PickleReadingFixtureFailure $diagnostic 18 }
  $diagnostic | Add-Member -NotePropertyName message -NotePropertyValue 'unexpected-sensitive-source'
  Assert-ReadingRefused { ConvertTo-PickleReadingFixtureFailure $diagnostic 17 }
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
  # Both originally present and originally absent archives must be restored
  # only after their current bytes are proven to be our exact test outputs.
  foreach ($originalArchives in @($false,$true)) {
    foreach ($savedScenario in @('simplify','explain-may','follow-up-return-clear','formatted-copy')) {
    $case=Join-Path $root ('saved-'+[guid]::NewGuid().ToString('N'))
    $profile=Join-Path $case 'data';New-Item -ItemType Directory -Path $profile -Force | Out-Null
    $caseSettings=Join-Path $profile 'settings.json'
    [IO.File]::WriteAllText($caseSettings,'Original fixture preferences.')
    $bookmarks=Join-Path $profile 'bookmarks.json';$caseHistory=Join-Path $profile 'history.json'
    if ($originalArchives) {
      [IO.File]::WriteAllText($bookmarks,'Original bookmark bytes, never rendered.')
      [IO.File]::WriteAllText($caseHistory,'Original history bytes, never rendered.')
    }
    $fixturePath=Join-Path $case 'ReadingFixture.exe'
    [IO.File]::WriteAllText($fixturePath,'Owned path fixture only; never executed.')
    $contract=[ordered]@{selected='Café controlled selected passage.';answer='The controlled answer retains Café.'}
    $savedSnapshot=Save-PickleReadingProfile $profile (Join-Path $case 'backup')
    Initialize-PickleReadingArchives $savedSnapshot $contract $fixturePath -Scenario $savedScenario
    if ($savedSnapshot.OwnedArchives.Count -ne 2 -or [IO.File]::ReadAllText($bookmarks) -cne '{"version":1,"data":[]}' -or [IO.File]::ReadAllText($caseHistory) -cne '{"version":1,"data":[]}') { throw 'Original archives were not isolated before any saved-answer UI.' }
    Assert-PickleReadingOwnedArchives $savedSnapshot
    [IO.File]::WriteAllText($caseSettings,'Temporary fixture preferences.')
    $recorded=if ($originalArchives) { [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffffff',[Globalization.CultureInfo]::InvariantCulture)+'00+00:00' } else { [DateTimeOffset]::UtcNow.ToString('o') }
    $entry=[ordered]@{id=[guid]::NewGuid().ToString('D');recorded_at=$recorded;app_name='ReadingFixture';app_id=('windows:'+$fixturePath.ToLowerInvariant());action='Simplify';question=$null;passage=$contract.selected;answer=$contract.answer;quality=$null;chart=$null;reference=$null}
    if ($savedScenario -ceq 'explain-may') { $entry.action='Explain';$entry.question='may' }
    elseif ($savedScenario -ceq 'follow-up-return-clear') { $entry.action='Follow-up';$entry.question=Get-PickleReadingFollowQuestion;$entry.answer=Get-PickleReadingFollowAnswer }
    elseif ($savedScenario -ceq 'formatted-copy') { $entry.answer=Get-PickleReadingExpectedAnswer $contract $savedScenario }
    $archive=[ordered]@{version=1;data=@($entry)}
    $validSaved=[Text.UTF8Encoding]::new($false).GetBytes(($archive | ConvertTo-Json -Depth 8 -Compress))
    Write-PickleReadingAtomic $bookmarks $validSaved
    Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
    if ([IO.File]::ReadAllText($caseSettings) -cne 'Temporary fixture preferences.') { throw 'Unadmitted Save output caused partial restoration.' }
    $savedSnapshot.SaveAdmitted=$true
    $actual=Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore -Scenario $savedScenario
    if ($savedScenario -ceq 'explain-may') {
      Assert-ReadingRefused { Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore }
      foreach ($badQuestion in @($null,'MAY','Explain “may”',"Explain the meaning of ‘may’ in this passage, using the surrounding context. Keep it brief; distinguish the contextual meaning from other meanings.")) {
        $badEntry=[ordered]@{};foreach ($key in $entry.Keys) { $badEntry[$key]=$entry[$key] };$badEntry.question=$badQuestion
        Write-PickleReadingAtomic $bookmarks ([Text.UTF8Encoding]::new($false).GetBytes((@{version=1;data=@($badEntry)} | ConvertTo-Json -Depth 8 -Compress)))
        Assert-ReadingRefused { Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore -Scenario $savedScenario }
        Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
      }
      Write-PickleReadingAtomic $bookmarks $validSaved
    } elseif ($savedScenario -ceq 'follow-up-return-clear') {
      Assert-ReadingRefused { Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore }
      foreach ($badQuestion in @($null,'What does may mean for the Cafe route?',(Get-PickleReadingFollowQuestion).Replace('“','"').Replace('”','"'))) {
        $badEntry=[ordered]@{};foreach ($key in $entry.Keys) { $badEntry[$key]=$entry[$key] };$badEntry.question=$badQuestion
        Write-PickleReadingAtomic $bookmarks ([Text.UTF8Encoding]::new($false).GetBytes((@{version=1;data=@($badEntry)} | ConvertTo-Json -Depth 8 -Compress)))
        Assert-ReadingRefused { Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore -Scenario $savedScenario }
        Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
      }
      Write-PickleReadingAtomic $bookmarks $validSaved
    } else { Assert-ReadingRefused { Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore -Scenario 'explain-may' } }
    if ($actual.Entry.id -cne $entry.id -or $actual.Entry.recorded_at -cne $recorded -or $actual.Entry.passage -cne $contract.selected -or $actual.Entry.answer -cne (Get-PickleReadingExpectedAnswer $contract $savedScenario) -or $actual.Hash -cne (Get-PickleReadingHash $validSaved)) { throw 'The actual saved Entry or full seven/nine-digit timestamp was not preserved exactly.' }
    $savedSnapshot.OwnedArchives['bookmarks.json'].SavedHash=$actual.Hash
    Assert-PickleReadingOwnedArchives $savedSnapshot
    foreach ($change in @(
      @{key='passage';value='BEFORE unrelated source'},
      @{key='answer';value='Different accepted answer'},
      @{key='action';value='Adjustment'},
      @{key='quality';value='Unexpected quality label'},
      @{key='question';value='Unexpected question'},
      @{key='chart';value='bar_chart'},
      @{key='reference';value=@{url='https://example.invalid/';title='Unexpected reference';kind='article'}},
      @{key='app_id';value=('windows:'+(Join-Path $case 'different.exe').ToLowerInvariant())},
      @{key='app_name';value='Another source'},
      @{key='recorded_at';value='2000-01-01T00:00:00Z'},
      @{key='id';value='invalid-id'},
      @{key='unexpected';value='Extra data'}
    )) {
      $changed=[ordered]@{};foreach ($key in $entry.Keys) { $changed[$key]=$entry[$key] };$changed[$change.key]=$change.value
      $bad=[Text.UTF8Encoding]::new($false).GetBytes((@{version=1;data=@($changed)} | ConvertTo-Json -Depth 8 -Compress))
      Write-PickleReadingAtomic $bookmarks $bad
      Assert-ReadingRefused { Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore -Scenario $savedScenario }
      Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
      if ((Get-PickleReadingHash ([IO.File]::ReadAllBytes($bookmarks))) -cne (Get-PickleReadingHash $bad) -or [IO.File]::ReadAllText($caseSettings) -cne 'Temporary fixture preferences.') { throw 'Archive refusal mutated an existing or newly created profile file.' }
    }
    foreach ($badArchive in @(@{version=2;data=@($entry)},@{version=1;data=@($entry,$entry)},@{version=1;data=@()},@{version=1;data=@($entry);extra=$true})) {
      Write-PickleReadingAtomic $bookmarks ([Text.UTF8Encoding]::new($false).GetBytes(($badArchive | ConvertTo-Json -Depth 8 -Compress)))
      Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
    }
    $encoded=[Text.Encoding]::UTF8.GetString($validSaved)
    $encodedAction='"action":"'+$entry.action+'"'
    foreach ($badJson in @($encoded.Replace('"version":1,','"version":1,"version":1,'),$encoded.Replace('"version":1,','"version":1.0,'),$encoded.Replace($encodedAction,$encodedAction+','+$encodedAction))) {
      Write-PickleReadingAtomic $bookmarks ([Text.Encoding]::UTF8.GetBytes($badJson))
      Assert-ReadingRefused { Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore -Scenario $savedScenario }
    }
    Write-PickleReadingAtomic $bookmarks $validSaved
    # A second valid-looking record must not replace the entry already verified
    # after ordinary quit, even though its fixed source/answer are identical.
    $changedId=[ordered]@{};foreach ($key in $entry.Keys) { $changedId[$key]=$entry[$key] };$changedId.id=[guid]::NewGuid().ToString('D')
    Write-PickleReadingAtomic $bookmarks ([Text.UTF8Encoding]::new($false).GetBytes((@{version=1;data=@($changedId)} | ConvertTo-Json -Depth 8 -Compress)))
    $null=Read-PickleReadingSavedEntry $bookmarks $contract $fixturePath $savedSnapshot.SaveNotBefore -Scenario $savedScenario
    Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
    Write-PickleReadingAtomic $bookmarks ([Text.Encoding]::UTF8.GetBytes('{"version":1,"data":[]}'))
    Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
    Write-PickleReadingAtomic $bookmarks $validSaved
    [IO.File]::WriteAllText($caseHistory,'Unexpected concurrent history content.')
    Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
    if ([IO.File]::ReadAllText($caseHistory) -cne 'Unexpected concurrent history content.' -or [IO.File]::ReadAllText($caseSettings) -cne 'Temporary fixture preferences.') { throw 'Concurrent history refusal failed to preserve every file.' }
    Write-PickleReadingAtomic $caseHistory ([Text.Encoding]::UTF8.GetBytes('{"version":1,"data":[]}'))
    Remove-Item -LiteralPath $bookmarks
    Assert-ReadingRefused { Restore-PickleReadingProfile $savedSnapshot { $true } }
    Write-PickleReadingAtomic $bookmarks $validSaved
    Restore-PickleReadingProfile $savedSnapshot { $true }
    if ([IO.File]::ReadAllText($caseSettings) -cne 'Original fixture preferences.') { throw 'Saved workflow failed to restore original preferences.' }
    if ($originalArchives) {
      if ([IO.File]::ReadAllText($bookmarks) -cne 'Original bookmark bytes, never rendered.' -or [IO.File]::ReadAllText($caseHistory) -cne 'Original history bytes, never rendered.') { throw 'Existing original archives were not restored byte exactly.' }
    } elseif ((Test-Path -LiteralPath $bookmarks) -or (Test-Path -LiteralPath $caseHistory)) { throw 'Only verified synthetic new archives should have been removed.' }
    }
  }
  if ($IsWindows) {
    # Assign only a dedicated subprocess; the native declaration itself is safe
    # to compile in this parent without invoking any kernel entry point.
    Initialize-PickleReadingDelegateJobType
    $containmentProbe=Join-Path $root 'owned containment probe.ps1';$containmentReady=Join-Path $root 'owned containment ready.json'
    [IO.File]::WriteAllText($containmentProbe,@'
param([string]$Helpers,[string]$Ready,[string]$PowerShell)
$ErrorActionPreference='Stop'
. (Join-Path $Helpers 'windows-reading-smoke.ps1')
. (Join-Path $Helpers 'windows-reading-containment.ps1')
Initialize-PickleReadingDelegateContainment
$child=Start-PickleReadingChild $PowerShell @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 120')
[IO.File]::WriteAllText($Ready,(@{pid=$PID;child_pid=$child.Id;child_start_time_utc_ticks=$child.StartTime.ToUniversalTime().Ticks} | ConvertTo-Json -Compress))
Start-Sleep -Seconds 120
'@)
    $containedOwner=Start-PickleReadingChild $powerShell @('-NoProfile','-NonInteractive','-File',$containmentProbe,'-Helpers',$PSScriptRoot,'-Ready',$containmentReady,'-PowerShell',$powerShell)
    $containedChild=$null
    try {
      $containedInfo=Wait-PickleReadingReady $containedOwner $powerShell $containmentReady
      $candidate=[Diagnostics.Process]::GetProcessById([int]$containedInfo.child_pid)
      try {
        # Retain its OS handle before checking creation time and executable.
        # An unrelated process that reused this PID is never cleanup-owned.
        $null=$candidate.Handle
        if ($candidate.StartTime.ToUniversalTime().Ticks -ne [long]$containedInfo.child_start_time_utc_ticks) { throw 'The contained child PID no longer identifies the original owned process.' }
        Invoke-PickleReadingIdentityBarrier { Assert-PickleReadingChild $candidate $powerShell } ([DateTime]::UtcNow.AddSeconds(10))
        $containedChild=$candidate;$candidate=$null
      } finally { if ($null -ne $candidate) { $candidate.Dispose() } }
      $containedOwner.Kill()
      if (!$containedOwner.WaitForExit(10000) -or !$containedChild.WaitForExit(10000)) { throw 'Closing the dedicated reading owner did not terminate its contained child.' }
    } finally {
      foreach ($child in @($containedChild,$containedOwner)) { if ($null -ne $child) { if (!$child.HasExited) { $child.Kill();$child.WaitForExit(10000) | Out-Null };$child.Dispose() } }
    }
    $executable=Build-PickleReadingFixture $root
    $canonical=Get-PickleReadingCanonicalExecutable $executable
    $alias=Join-Path (Join-Path $root 'unused-parent') '../ReadingFixture.exe'
    if ((Get-PickleReadingCanonicalExecutable $alias) -cne $canonical) { throw 'Owned executable path aliases did not resolve to the same file.' }
    $other=Join-Path $root 'OtherFixture.exe';[IO.File]::Copy($executable,$other)
    if ((Get-PickleReadingCanonicalExecutable $other) -ceq $canonical) { throw 'Different installed executable paths shared identity.' }
    $selfTest=Start-PickleReadingChild $executable @('--self-test')
    try { if (!$selfTest.WaitForExit(10000) -or $selfTest.ExitCode -ne 0) { throw 'Owned native fixture non-UI predicates failed.' } }
    finally { if (!$selfTest.HasExited) { $selfTest.Kill();$selfTest.WaitForExit(10000) | Out-Null };$selfTest.Dispose() }
  } else {
    Assert-ReadingRefused { Assert-PickleInstalledReadingWorkflow '/absent-app' '/absent-profile' '/absent-evidence' 'debug' }
  }
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
Write-Host 'Reading fixtures passed: bounded HTTP/source contract, owned Save/restart archive guards, byte-exact restoration, tamper/schema/link/count/size refusals; no native app or GUI launched.'
