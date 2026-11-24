param(
  [string]$InstallerPath = 'artifacts/PickleRune-setup.exe',
  [string]$InstallDirectory,
  [ValidateSet('debug','release')][string]$Configuration = 'debug',
  [string]$ExpectedVersion
)
function Assert-PicklePackageReadingEvidence($Evidence,[bool]$RequireWindowCapture,[string]$Configuration,[ValidateSet('simplify','explain-may','follow-up-return-clear','formatted-copy')][string]$Scenario='simplify') {
  if ($Evidence.scenario -cne $Scenario -or $Evidence.driver.scenario -cne $Scenario -or $Evidence.restart_driver.scenario -cne $Scenario -or $Evidence.server.scenario -cne $Scenario) { throw 'The package reading evidence scenario differed.' }
  if ($Scenario -cne 'simplify' -and ($RequireWindowCapture -or $Configuration -cne 'debug')) { throw 'Additional reading gates require a separate screen-off debug workflow.' }
  if (($Evidence.schema -isnot [int] -and $Evidence.schema -isnot [long]) -or $Evidence.schema -ne 1 -or $Evidence.test -cne 'windows-installed-reading-workflow' -or $Evidence.configuration -cne $Configuration) { throw 'The package reading evidence did not match this acceptance run.' }
  foreach ($key in @('passed','ordinary_exit','lease_released','control_removed','restart_ordinary_exit','restart_lease_released','restart_control_removed','saved_archive_verified','saved_archive_unchanged','profile_restored','owned_children_stopped')) {
    if ($Evidence.$key -isnot [bool] -or !$Evidence.$key) { throw 'The installed reading, Save, restart and cleanup workflow did not pass completely.' }
  }
  foreach ($item in @(@($Evidence,'require_window_capture'),@($Evidence,'marker_in_context'),@($Evidence.server,'window_capture_required'),@($Evidence.server,'marker_in_context'))) {
    if ($item[0].($item[1]) -isnot [bool] -or $item[0].($item[1]) -ne $RequireWindowCapture) { throw 'The package reading evidence did not prove the required OCR presence or screen-off absence.' }
  }
  foreach ($key in @('source_verified','context_verified')) {
    if ($Evidence.server.$key -isnot [bool] -or !$Evidence.server.$key) { throw 'The owned provider did not verify the selected source and context boundary.' }
  }
  if ($Evidence.server.rejected -isnot [bool] -or $Evidence.server.rejected) { throw 'The owned provider rejected a request.' }
  $requests=if ($Scenario -ceq 'follow-up-return-clear') { 2 } else { 1 }
  foreach ($item in @(@('total_requests',($requests*2)),@('show_count',$requests),@('chat_count',$requests))) {
    $value=$Evidence.server.($item[0])
    if (($value -isnot [int] -and $value -isnot [long]) -or $value -ne $item[1]) { throw 'The owned provider request count differed.' }
  }
  foreach ($key in @('passed','initial_compose_ready','fixture_selection_verified','chooser_source_verified','consent_before_network_verified','provider_source_verified','source_retained_verified','window_marker_painted','window_marker_absent_from_accessibility','completed_answer_verified','saved_answer_verified','ordinary_quit_verified')) {
    if ($Evidence.driver.$key -isnot [bool] -or !$Evidence.driver.$key) { throw 'The native reading driver omitted a required acceptance step.' }
  }
  $restartKeys=@('passed','application_identity_verified','saved_entry_verified','saved_detail_verified','ordinary_quit_verified')
  if ($Scenario -ceq 'explain-may') {
    foreach ($key in @('explain_before_base_verified','explain_term_value_verified','accepted_explain_turn_verified')) {
      if ($Evidence.driver.$key -isnot [bool] -or !$Evidence.driver.$key) { throw 'The accepted before-base Explain request was not verified.' }
    }
    $restartKeys+=@('saved_card_explain_metadata_verified','saved_detail_raw_term_verified')
  } elseif ($Scenario -ceq 'follow-up-return-clear') {
    Assert-PickleFollowUpKeyboardEvidence $Evidence.driver $Evidence.server $Evidence.restart_driver
    $restartKeys+=@('saved_card_follow_up_metadata_verified','saved_detail_raw_question_verified')
  } else { $restartKeys+='saved_metadata_absence_verified' }
  foreach ($key in $restartKeys) {
    if ($Evidence.restart_driver.$key -isnot [bool] -or !$Evidence.restart_driver.$key) { throw 'The native saved-answer restart driver did not pass completely.' }
  }
  if ($Scenario -ceq 'formatted-copy') {
    if ($Evidence.driver.native_copy_verified -isnot [bool] -or !$Evidence.driver.native_copy_verified -or $Evidence.restart_driver.saved_native_copy_verified -isnot [bool] -or !$Evidence.restart_driver.saved_native_copy_verified) { throw 'The native formatted-answer Copy roundtrips were not verified.' }
    foreach ($driver in @($Evidence.driver,$Evidence.restart_driver)) {
      if ($driver.known_fixture_clipboard_cleared -isnot [bool] -or !$driver.known_fixture_clipboard_cleared -or $driver.clipboard_sender_pid_verified -isnot [bool] -or $driver.clipboard_sender_pid_verified) { throw 'The clipboard cleanup or sender-provenance boundary differed.' }
    }
  }
  foreach ($driver in @($Evidence.driver,$Evidence.restart_driver)) {
    if (($driver.app_exit_code -isnot [int] -and $driver.app_exit_code -isnot [long]) -or $driver.app_exit_code -ne 0) { throw 'The installed reading application did not quit normally.' }
  }
}
$ErrorActionPreference = 'Stop'
$uiMode = if ($Configuration -eq 'release') { 'native-window' } else { 'headless-render-and-native-window' }
Set-Location (Join-Path $PSScriptRoot '..')
$installer = (Resolve-Path $InstallerPath).Path
if (!$ExpectedVersion) {
  $metadata = (& cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json)
  if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the expected package version.' }
  $ExpectedVersion = ($metadata.packages | Where-Object { $_.name -eq 'pickle-app' }).version
}
if (!$ExpectedVersion) { throw 'Supply the expected installed app version.' }
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PickleRune'
if (Test-Path $uninstallKey) { throw 'Use a clean, isolated Windows test account. This smoke test does not overwrite an existing installed-app registration.' }
$testDirectory = if ($InstallDirectory) { [IO.Path]::GetFullPath($InstallDirectory) } else { Join-Path ([IO.Path]::GetTempPath()) ('pickle-package-smoke-' + [guid]::NewGuid().ToString('N')) }
if (Test-Path -LiteralPath $testDirectory) { throw 'The smoke installation directory must be new and test-owned.' }
$secondDirectory = Join-Path ([IO.Path]::GetTempPath()) ('pickle-package-second-' + [guid]::NewGuid().ToString('N'))
$process = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$testDirectory") -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Installer failed with exit code $($process.ExitCode)." }
$application = Join-Path $testDirectory 'PickleRune.exe'
function Invoke-AppProbe([string[]]$Arguments) {
  $result = Start-Process -FilePath $application -ArgumentList $Arguments -Wait -PassThru
  return $result.ExitCode
}
function Assert-RetryableUninstall([string]$Uninstaller, [string]$Directory) {
  $hostCopy = Join-Path $Directory 'pickle-browser-host.exe'
  # Deny FILE_SHARE_DELETE on one auxiliary payload file without starting any browser process.
  $held = [IO.File]::Open($hostCopy, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
  try {
    $blocked = Start-Process -FilePath $Uninstaller -ArgumentList @('/S', "_?=$Directory") -Wait -PassThru
    if ($blocked.ExitCode -eq 0) { throw 'Uninstall should report its locked auxiliary file.' }
    if (!(Test-Path (Join-Path $Directory 'PickleRune.exe'))) { throw 'A failed uninstall removed the main executable needed for safe retry.' }
  } finally { $held.Dispose() }
}
if ((Invoke-AppProbe @('--check-running')) -ne 0) { throw 'A Pickle instance is already running or could not be inspected safely. Use an isolated test account.' }
foreach ($filename in @('PickleRune.exe', 'pickle-browser-host.exe', 'Uninstall.exe', 'Pickle.ico', 'THIRD_PARTY_NOTICES.txt', 'dependency-inventory.json', 'browser-extension/manifest.json')) {
  if (!(Test-Path (Join-Path $testDirectory $filename))) { throw "Installed payload is missing $filename." }
}
$variant = if (Test-Path (Join-Path $testDirectory 'PickleIdentity.msix')) { 'identity' } else { 'unpackaged' }
$versionReport = Join-Path $PWD "artifacts/$variant-$Configuration-installed-version.txt"
$versionProbe = Start-Process -FilePath $application -ArgumentList @('--version') -RedirectStandardOutput $versionReport -Wait -PassThru
if ($versionProbe.ExitCode -ne 0 -or (Get-Content -Raw $versionReport).Trim() -cne "Pickle Rune $ExpectedVersion") { throw 'The installed application version does not match the expected package version.' }
$capabilities = Join-Path $testDirectory 'capabilities.json'
$probe = Start-Process -FilePath $application -ArgumentList @('--capabilities') -RedirectStandardOutput $capabilities -Wait -PassThru
if ($probe.ExitCode -ne 0) { throw 'Installed application capabilities probe failed.' }
$report = Get-Content $capabilities -Raw | ConvertFrom-Json
if (!$report.selection -or !$report.menu_bar) { throw 'Installed application did not report the core Windows capabilities.' }
$hasIdentity = Test-Path (Join-Path $testDirectory 'PickleIdentity.msix')
if ($hasIdentity -and !$report.window_capture_ocr) { throw 'The signed external identity did not reach the installed app process.' }
Copy-Item -LiteralPath $capabilities -Destination (Join-Path $PWD $(if ($hasIdentity) { 'artifacts/identity-capabilities.json' } else { 'artifacts/unpackaged-capabilities.json' }))
if ($hasIdentity) {
  $ocrReport = Join-Path $PWD 'artifacts/identity-ocr-result.json'
  $ocrErrors = Join-Path $PWD 'artifacts/identity-native-ocr-stderr.log'
  $priorFixturePath = [Environment]::GetEnvironmentVariable('PICKLE_OCR_FIXTURE_PNG','Process')
  try {
    $env:PICKLE_OCR_FIXTURE_PNG = Join-Path $PWD 'artifacts/identity-native-ocr-fixture.png'
    $ocr = Start-Process -FilePath $application -ArgumentList @('--verify-windows-ocr') -RedirectStandardOutput $ocrReport -RedirectStandardError $ocrErrors -Wait -PassThru
  } finally { [Environment]::SetEnvironmentVariable('PICKLE_OCR_FIXTURE_PNG',$priorFixturePath,'Process') }
  if ($ocr.ExitCode -ne 0) {
    # Only this fixed owned-image probe writes this log; it never contains source/window OCR.
    $reader = [IO.File]::OpenText($ocrErrors)
    try {
      $buffer = New-Object char[] 16384
      $read = $reader.Read($buffer,0,$buffer.Length)
      $controlledDiagnostic = if ($read -gt 0) { -join $buffer[0..($read-1)] } else { '' }
    } finally { $reader.Dispose() }
    Write-Host $controlledDiagnostic
    throw 'Installed Windows OCR could not recognize the fixed in-memory fixture. See the controlled OCR stderr evidence.'
  }
  $ocrResult = Get-Content -Raw $ocrReport | ConvertFrom-Json
  if ($ocrResult.test -cne 'windows-ocr-fixed-fixture' -or $ocrResult.passed -ne $true) { throw 'The installed OCR probe did not return its expected evidence.' }
}
if ($Configuration -eq 'debug') {
  $snapshot = Join-Path $PWD $(if ($hasIdentity) { 'artifacts/identity-installed-reader.png' } else { 'artifacts/unpackaged-installed-reader.png' })
  $tree = Join-Path $PWD $(if ($hasIdentity) { 'artifacts/identity-installed-reader-tree.json' } else { 'artifacts/unpackaged-installed-reader-tree.json' })
  $render = Start-Process -FilePath $application -ArgumentList @('--scene', 'sample', '--snap', "`"$snapshot`"", '--tree', "`"$tree`"") -Wait -PassThru
  if ($render.ExitCode -ne 0 -or !(Test-Path $snapshot) -or !(Test-Path $tree)) { throw 'Installed debug application could not render the offline sample.' }
}
$uninstaller = Join-Path $testDirectory 'Uninstall.exe'
& (Join-Path $PSScriptRoot 'test-windows-startup-smoke.ps1')
. (Join-Path $PSScriptRoot 'windows-native-smoke.ps1')
. (Join-Path $PSScriptRoot 'windows-startup-smoke.ps1')
. (Join-Path $PSScriptRoot 'windows-reading-smoke.ps1')
$supportRoot = Get-PickleReadingDataDirectory
Assert-PickleInstalledStartupFailure $application $supportRoot (Join-Path $PWD "artifacts/$variant-$Configuration-native-smoke-startup-failure.json") $Configuration
. (Join-Path $PSScriptRoot 'windows-reading-delegate.ps1')
$readingEvidence=Join-Path $PWD "artifacts/$variant-$Configuration-native-smoke-reading.json"
Invoke-PickleReadingCore $application $supportRoot $readingEvidence $Configuration -RequireWindowCapture:$hasIdentity
Assert-PicklePackageReadingEvidence (Read-PickleReadingJson $readingEvidence) $hasIdentity $Configuration
if (!$hasIdentity -and $Configuration -ceq 'debug' -and $env:GITHUB_ACTIONS -ceq 'true' -and $env:RUNNER_ENVIRONMENT -ceq 'github-hosted') {
  # Additional real term/action intent acceptance; the default Simplify absence
  # workflow above remains independently required for every package variant.
  $explainEvidence=Join-Path $PWD 'artifacts/unpackaged-debug-native-smoke-explain-reading.json'
  Invoke-PickleReadingCore $application $supportRoot $explainEvidence $Configuration -Scenario 'explain-may'
  Assert-PicklePackageReadingEvidence (Read-PickleReadingJson $explainEvidence) $false $Configuration -Scenario 'explain-may'
  $followEvidence=Join-Path $PWD 'artifacts/unpackaged-debug-native-smoke-follow-up-reading.json'
  Invoke-PickleReadingCore $application $supportRoot $followEvidence $Configuration -Scenario 'follow-up-return-clear'
  Assert-PicklePackageReadingEvidence (Read-PickleReadingJson $followEvidence) $false $Configuration -Scenario 'follow-up-return-clear'
}
. (Join-Path $PSScriptRoot 'windows-extension-smoke.ps1')
. (Join-Path $PSScriptRoot 'windows-extension-delivery.ps1')
if (Test-PickleExtensionSetupGate $Configuration $hasIdentity $PSEdition ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) $env:GITHUB_ACTIONS $env:RUNNER_ENVIRONMENT) {
  # Keep strict load-only evidence, then attempt authenticated article delivery
  # after either clean outcome. Each mode owns its own profile/job.
  & (Join-Path $PSScriptRoot 'test-windows-extension-smoke.ps1')
  & (Join-Path $PSScriptRoot 'test-windows-extension-delivery.ps1')
  $extensionFailures=[Collections.Generic.List[string]]::new()
  foreach ($browser in @('chrome','edge')) {
    if (!(Invoke-PickleExtensionCore $testDirectory $browser (Join-Path $PWD "artifacts/windows-extension-$browser-native-smoke.json"))) { $extensionFailures.Add($browser) }
    if (!(Invoke-PickleExtensionDeliveryCore $testDirectory $browser (Join-Path $PWD "artifacts/windows-extension-$browser-delivery-native-smoke.json"))) { $extensionFailures.Add("$browser-delivery") }
  }
  # Separate fresh profile/app/job, additive to both unchanged shortcut modes.
  if (!(Invoke-PickleExtensionDeliveryCore $testDirectory 'chrome' (Join-Path $PWD 'artifacts/windows-extension-chrome-popup-delivery-native-smoke.json') -Trigger 'popup')) { $extensionFailures.Add('chrome-popup-delivery') }
  # Each browser delegate has certified cleanup before returning a safe failure.
  # Preserve those independent reports before exercising the OS clipboard.
  $copyEvidence=Join-Path $PWD 'artifacts/unpackaged-debug-native-smoke-formatted-copy-reading.json'
  Invoke-PickleReadingCore $application $supportRoot $copyEvidence $Configuration -Scenario 'formatted-copy'
  Assert-PicklePackageReadingEvidence (Read-PickleReadingJson $copyEvidence) $false $Configuration -Scenario 'formatted-copy'
  if ($extensionFailures.Count) { throw 'One or more Stable extension setup or article-delivery stages failed; scoped reports were retained.' }
}
if ($hasIdentity) {
  # The installed user workflow above is the package capture gate. Retain the
  # isolated self-launch diagnostic's portable collector/protocol regressions;
  # its native command remains available explicitly for manual diagnosis.
  . (Join-Path $PSScriptRoot 'windows-capture-smoke.ps1')
  & (Join-Path $PSScriptRoot 'test-windows-capture-smoke.ps1')
}
$background = $null
try {
  $stdout = Join-Path $PWD "artifacts/$variant-$Configuration-native-stdout.log"
  $stderr = Join-Path $PWD "artifacts/$variant-$Configuration-native-stderr.log"
  $background = Start-Process -FilePath $application -ArgumentList @('--sample') -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
  $nativeTest = if ($Configuration -eq 'release') { 'windows-native-release-window' } else { 'windows-native-debug-window' }
  Assert-PickleNativeReleaseWindow $background $application (Join-Path $PWD "artifacts/$variant-$Configuration-native-smoke.json") $nativeTest
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  $running = $false
  while ([DateTime]::UtcNow -lt $deadline) {
    $background.Refresh()
    if ($background.HasExited) { throw 'The owned packaged application exited before holding its instance lease.' }
    if ((Invoke-AppProbe @('--check-running')) -eq 10) { $running = $true; break }
    Start-Sleep -Milliseconds 100
  }
  if (!$running) { throw 'The packaged application never reported its running lease.' }
  $blockedInstall = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$testDirectory") -Wait -PassThru
  if ($blockedInstall.ExitCode -ne 10) { throw "Installation while running must fail with status 10; got $($blockedInstall.ExitCode)." }
  # _?= keeps the actual uninstaller in this process, so ExitCode reflects the guard result.
  $blockedUninstall = Start-Process -FilePath $uninstaller -ArgumentList @('/S', "_?=$testDirectory") -Wait -PassThru
  if ($blockedUninstall.ExitCode -ne 10 -or !(Test-Path $application)) { throw 'Uninstall failed to preserve a running installation.' }
  if ((Invoke-AppProbe @('--disconnect-browsers')) -ne 10) { throw 'Browser cleanup must refuse while Pickle is running.' }
}
finally {
  # Only the exact process launched above is stopped; never kill a process by executable name.
  if ($null -ne $background) {
    $background.Refresh()
    if (!$background.HasExited) { $background.Kill(); $background.WaitForExit(10000) | Out-Null }
  }
}
if ((Invoke-AppProbe @('--check-running')) -ne 0) { throw 'The instance lease did not recover after the test process exited.' }
if ((Invoke-AppProbe @('--disconnect-browsers')) -ne 0) { throw 'The installed application browser-cleanup command failed.' }
$marker = Join-Path $testDirectory 'unrelated-file-must-remain.txt'
Set-Content -LiteralPath $marker -Value 'Uninstall must preserve unrelated files.'
$dataMarker = Join-Path $supportRoot ('package-smoke-retention-' + [guid]::NewGuid().ToString('N') + '.txt')
New-Item -ItemType Directory -Path $supportRoot -Force | Out-Null
Set-Content -LiteralPath $dataMarker -Value 'Uninstall must preserve application data.'
if ($hasIdentity) {
  $identity = Get-Content -Raw (Join-Path $testDirectory 'PickleIdentity.json') | ConvertFrom-Json
  $registered = @(Get-AppxPackage -Name $identity.name | Where-Object { $_.PackageFamilyName -ceq $identity.family })
  if ($registered.Count -ne 1) { throw 'Expected the exact signed identity family to be registered.' }
  $metadataPath = Join-Path $testDirectory 'PickleIdentity.json'
  $originalMetadata = [IO.File]::ReadAllBytes($metadataPath)
  try {
    $differentIdentity = Get-Content -Raw $metadataPath | ConvertFrom-Json
    $differentIdentity.name = $identity.name + '.Other'
    $differentIdentity.family = $differentIdentity.name + '_' + ($identity.family -split '_')[-1]
    $differentIdentity | ConvertTo-Json | Set-Content -Encoding utf8 $metadataPath
    $expectedMetadata = [Convert]::ToBase64String([IO.File]::ReadAllBytes($metadataPath))
    $migration = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$testDirectory") -Wait -PassThru
    if ($migration.ExitCode -eq 0) { throw 'Same-path identity migration must be refused before copying.' }
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($metadataPath)) -cne $expectedMetadata) { throw 'Rejected migration overwrote existing identity metadata.' }
  } finally { [IO.File]::WriteAllBytes($metadataPath,$originalMetadata) }
  $repair = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$testDirectory") -Wait -PassThru
  if ($repair.ExitCode -ne 0) { throw 'Repairing the same identity version and location failed.' }
  $conflict = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$secondDirectory") -Wait -PassThru
  if ($conflict.ExitCode -eq 0) { throw 'An identity already owned by another external directory must not be stolen.' }
  if (!(Test-Path $application) -or (Get-ItemProperty -LiteralPath $uninstallKey).InstallLocation -ne $testDirectory) { throw 'Identity conflict changed the original installed application.' }
  Assert-RetryableUninstall $uninstaller $testDirectory
  $removed = Start-Process -FilePath $uninstaller -ArgumentList @('/S', "_?=$testDirectory") -Wait -PassThru
  if ($removed.ExitCode -ne 0 -or (Test-Path $application)) { throw 'Signed identity uninstall failed.' }
  if (@(Get-AppxPackage -Name $identity.name | Where-Object { $_.PackageFamilyName -ceq $identity.family }).Count) { throw 'Owned identity was retained after successful uninstall.' }
  if (!(Test-Path $marker) -or !(Test-Path $dataMarker) -or (Test-Path $uninstallKey)) { throw 'Identity uninstall violated data retention or owned entry cleanup.' }
  if (Test-Path $uninstaller) { Remove-Item -LiteralPath $uninstaller }
  Remove-Item -LiteralPath $dataMarker
  Write-Host "PASS ($Configuration, UI=$uiMode): signed external identity, installed version/runtime identity, repair, conflict refusal, live-instance guards, owned identity/browser cleanup and retained data."
  Write-Host "Test-only diagnostic directory retained at $testDirectory"
  return
}
# A second install replaces the global app entry. Removing the old one must preserve the new one.
$second = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$secondDirectory") -Wait -PassThru
if ($second.ExitCode -ne 0) { throw 'The second isolated installation failed.' }
Assert-RetryableUninstall $uninstaller $testDirectory
$removed = Start-Process -FilePath $uninstaller -ArgumentList @('/S', "_?=$testDirectory") -Wait -PassThru
if ($removed.ExitCode -ne 0) { throw 'Uninstaller reported failure.' }
$deadline = [DateTime]::UtcNow.AddSeconds(20)
while ((Test-Path $application) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 200 }
if (Test-Path $application) { throw 'Uninstall did not remove the installed executable.' }
if (!(Test-Path $marker) -or !(Test-Path $dataMarker)) { throw 'Uninstall removed unrelated files or application data.' }
$registeredLocation = (Get-ItemProperty -LiteralPath $uninstallKey).InstallLocation
if ($registeredLocation -ne $secondDirectory) { throw 'The older uninstaller removed or changed another installation registration.' }
$secondApplication = Join-Path $secondDirectory 'PickleRune.exe'
if (!(Test-Path $secondApplication)) { throw 'The older uninstaller removed another installation executable.' }
if (Test-Path $uninstaller) { Remove-Item -LiteralPath $uninstaller }
$secondUninstaller = Join-Path $secondDirectory 'Uninstall.exe'
$removed = Start-Process -FilePath $secondUninstaller -ArgumentList @('/S', "_?=$secondDirectory") -Wait -PassThru
if ($removed.ExitCode -ne 0) { throw 'The second uninstaller reported failure.' }
if (Test-Path $secondApplication) { throw 'The second uninstaller did not remove its executable.' }
if (Test-Path $secondUninstaller) { Remove-Item -LiteralPath $secondUninstaller }
if (Test-Path $uninstallKey) { throw 'The final owned installed-app registration was not removed.' }
if (!(Test-Path $dataMarker)) { throw 'Application data was not retained after final uninstall.' }
Remove-Item -LiteralPath $dataMarker
Write-Host "PASS ($Configuration, UI=$uiMode): Windows installation/version, live-instance guards, browser cleanup, ownership-aware uninstall, and data retention."
Write-Host "Test-only diagnostic directory retained at $testDirectory"
