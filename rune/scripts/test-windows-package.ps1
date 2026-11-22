param([string]$InstallerPath = 'artifacts/PickleRune-setup.exe')
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
$installer = (Resolve-Path $InstallerPath).Path
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PickleRune'
if (Test-Path $uninstallKey) { throw 'Use a clean, isolated Windows test account. This smoke test does not overwrite an existing installed-app registration.' }
$testDirectory = Join-Path ([IO.Path]::GetTempPath()) ('pickle-package-smoke-' + [guid]::NewGuid().ToString('N'))
$secondDirectory = Join-Path ([IO.Path]::GetTempPath()) ('pickle-package-second-' + [guid]::NewGuid().ToString('N'))
$process = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$testDirectory") -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Installer failed with exit code $($process.ExitCode)." }
$application = Join-Path $testDirectory 'PickleRune.exe'
function Invoke-AppProbe([string[]]$Arguments) {
  $result = Start-Process -FilePath $application -ArgumentList $Arguments -Wait -PassThru
  return $result.ExitCode
}
if ((Invoke-AppProbe @('--check-running')) -ne 0) { throw 'A Pickle instance is already running or could not be inspected safely. Use an isolated test account.' }
foreach ($filename in @('PickleRune.exe', 'pickle-browser-host.exe', 'Uninstall.exe', 'Pickle.ico', 'THIRD_PARTY_NOTICES.txt', 'dependency-inventory.json', 'browser-extension/manifest.json')) {
  if (!(Test-Path (Join-Path $testDirectory $filename))) { throw "Installed payload is missing $filename." }
}
$capabilities = Join-Path $testDirectory 'capabilities.json'
$probe = Start-Process -FilePath $application -ArgumentList @('--capabilities') -RedirectStandardOutput $capabilities -Wait -PassThru
if ($probe.ExitCode -ne 0) { throw 'Installed application capabilities probe failed.' }
$report = Get-Content $capabilities -Raw | ConvertFrom-Json
if (!$report.selection -or !$report.menu_bar) { throw 'Installed application did not report the core Windows capabilities.' }
$snapshot = Join-Path $PWD 'artifacts/installed-reader.png'
$tree = Join-Path $PWD 'artifacts/installed-reader-tree.json'
$render = Start-Process -FilePath $application -ArgumentList @('--scene', 'sample', '--snap', "`"$snapshot`"", '--tree', "`"$tree`"") -Wait -PassThru
if ($render.ExitCode -ne 0 -or !(Test-Path $snapshot) -or !(Test-Path $tree)) { throw 'Installed application could not render the offline sample.' }
$uninstaller = Join-Path $testDirectory 'Uninstall.exe'
$background = $null
try {
  $background = Start-Process -FilePath $application -ArgumentList @('--background') -PassThru
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  $running = $false
  while ([DateTime]::UtcNow -lt $deadline) {
    $background.Refresh()
    if ($background.HasExited) { throw 'The packaged background application exited before holding its instance lease.' }
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
# A second install replaces the global app entry. Removing the old one must preserve the new one.
$second = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$secondDirectory") -Wait -PassThru
if ($second.ExitCode -ne 0) { throw 'The second isolated installation failed.' }
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
Write-Host 'PASS: Windows installation, offline rendering, live-instance guards, browser cleanup, ownership-aware uninstall, and data retention.'
Write-Host "Test-only diagnostic directory retained at $testDirectory"
