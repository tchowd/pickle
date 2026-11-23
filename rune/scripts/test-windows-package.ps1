param(
  [string]$InstallerPath = 'artifacts/PickleRune-setup.exe',
  [string]$InstallDirectory,
  [ValidateSet('debug','release')][string]$Configuration = 'debug',
  [string]$ExpectedVersion
)
$ErrorActionPreference = 'Stop'
$uiMode = if ($Configuration -eq 'release') { 'native-window' } else { 'headless-render' }
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
  $ocr = Start-Process -FilePath $application -ArgumentList @('--verify-windows-ocr') -RedirectStandardOutput $ocrReport -RedirectStandardError $ocrErrors -Wait -PassThru
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
$background = $null
try {
  if ($Configuration -eq 'release') {
    . (Join-Path $PSScriptRoot 'windows-native-smoke.ps1')
    $stdout = Join-Path $PWD "artifacts/$variant-release-native-stdout.log"
    $stderr = Join-Path $PWD "artifacts/$variant-release-native-stderr.log"
    $background = Start-Process -FilePath $application -ArgumentList @('--sample') -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    Assert-PickleNativeReleaseWindow $background $application (Join-Path $PWD "artifacts/$variant-release-native-smoke.json")
  } else {
    $background = Start-Process -FilePath $application -ArgumentList @('--background') -PassThru
  }
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
$supportRoot = Join-Path $env:LOCALAPPDATA 'Pickle\PickleRune\data'
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
