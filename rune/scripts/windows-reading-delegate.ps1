# Desktop-compatible boundary: only the reading acceptance needs PowerShell 7.
# Appx deployment and the signed test certificate lifecycle stay in the caller.
function ConvertTo-PickleReadingWindowsArgument([AllowEmptyString()][string]$Value) {
  # ProcessStartInfo.Arguments bypasses shells but still needs Windows argv
  # quoting on .NET Framework, which has no structured ArgumentList property.
  $result=[Text.StringBuilder]::new();[void]$result.Append([char]34);$slashes=0
  foreach ($character in $Value.ToCharArray()) {
    if ($character -eq [char]92) { $slashes++;continue }
    if ($character -eq [char]34) {
      [void]$result.Append([char]92,2*$slashes+1);[void]$result.Append($character)
    } else {
      [void]$result.Append([char]92,$slashes);[void]$result.Append($character)
    }
    $slashes=0
  }
  [void]$result.Append([char]92,2*$slashes);[void]$result.Append([char]34)
  return $result.ToString()
}

function New-PickleReadingCoreStart([string]$PowerShell,[string]$Script,[string]$Application,[string]$DataDirectory,[string]$EvidencePath,[string]$Configuration,[switch]$RequireWindowCapture) {
  $start=[Diagnostics.ProcessStartInfo]::new()
  $start.FileName=$PowerShell;$start.UseShellExecute=$false
  $arguments=@('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$Script,'-Application',$Application,'-DataDirectory',$DataDirectory,'-EvidencePath',$EvidencePath,'-Configuration',$Configuration)
  if ($RequireWindowCapture) { $arguments+= '-RequireWindowCapture' }
  $start.Arguments=(($arguments | ForEach-Object { ConvertTo-PickleReadingWindowsArgument $_ }) -join ' ')
  return $start
}

function Invoke-PickleReadingCore([string]$Application,[string]$DataDirectory,[string]$EvidencePath,[ValidateSet('debug','release')][string]$Configuration,[switch]$RequireWindowCapture) {
  if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'The reading delegate runs only on Windows.' }
  $command=Get-Command pwsh -CommandType Application -ErrorAction Stop | Select-Object -First 1
  $powerShell=[IO.Path]::GetFullPath($command.Source)
  $item=Get-Item -LiteralPath $powerShell -Force
  if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or [IO.Path]::GetFileName($powerShell) -ine 'pwsh.exe') { throw 'The reading acceptance requires the runner installed PowerShell 7 executable.' }
  $entry=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'invoke-windows-reading-core.ps1'))
  if (Test-Path -LiteralPath $EvidencePath) { throw 'The delegated reading evidence must be new.' }
  $process=[Diagnostics.Process]::Start((New-PickleReadingCoreStart $powerShell $entry $Application $DataDirectory $EvidencePath $Configuration -RequireWindowCapture:$RequireWindowCapture))
  try {
    if (!$process.WaitForExit(300000)) {
      # The dedicated entry establishes its kill-on-close job before any child
      # launch or profile mutation. Killing this exact owner closes that handle;
      # no process-name matching, PID reopening or Desktop WinRT migration.
      $process.Kill()
      if (!$process.WaitForExit(10000)) { throw 'The timed-out PowerShell 7 reading owner did not terminate.' }
      # The workflow writes a recovery location before replacing profile bytes.
      # Preserve it verbatim; a timeout never certifies restoration or a pass.
      throw 'The PowerShell 7 reading acceptance exceeded its five-minute deadline; any private profile backup was retained.'
    }
    if ($process.ExitCode -ne 0) { throw 'The PowerShell 7 reading acceptance failed; inspect its controlled evidence.' }
    $evidence=Read-PickleReadingJson $EvidencePath
    if ($evidence.schema -ne 1 -or $evidence.test -cne 'windows-installed-reading-workflow' -or $evidence.configuration -cne $Configuration -or $evidence.require_window_capture -isnot [bool] -or $evidence.require_window_capture -ne [bool]$RequireWindowCapture -or $evidence.marker_in_context -isnot [bool] -or $evidence.marker_in_context -ne [bool]$RequireWindowCapture -or $evidence.passed -ne $true -or $evidence.profile_restored -ne $true -or $evidence.owned_children_stopped -ne $true) { throw 'The PowerShell 7 reading delegate omitted successful cleanup evidence.' }
  } finally {
    if (!$process.HasExited) {
      $process.Kill()
      if (!$process.WaitForExit(10000)) { throw 'The owned PowerShell 7 reading delegate remained alive during cleanup.' }
    }
    $process.Dispose()
  }
}
