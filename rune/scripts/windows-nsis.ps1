# Shared NSIS prerequisite for the two hosted Windows packaging jobs.
# Discovery stays compatible with Windows PowerShell; installation needs Core
# for bounded termination of the exact Chocolatey process tree on timeout.
function Get-PickleNsisCompiler {
  $command=Get-Command makensis.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  $path=if ($command) { $command.Source } else { "${env:ProgramFiles(x86)}/NSIS/makensis.exe" }
  if (Test-Path -LiteralPath $path -PathType Leaf) { return (Get-Item -LiteralPath $path).FullName }
  return $null
}
function Test-PickleNsisTransientFailure([int]$ExitCode,[string]$Output) {
  if ($ExitCode -eq 0 -or !$Output -or $Output.Length -gt 1MB) { return $false }
  $transient="^Failed to fetch results from V2 feed at 'https://community\.chocolatey\.org/api/v2/Packages\(Id='nsis',Version='[0-9][0-9A-Za-z.+-]*'\)' with following message : Response status code does not indicate success: (?:502 \(Bad Gateway\)|503 \(Service Unavailable\)|504 \(Gateway Timeout\))\.$"
  $found=$false
  foreach ($line in ($Output -split '\r?\n')) {
    $line=$line.Trim();if (!$line) { continue }
    if ($line -cmatch $transient) { $found=$true;continue }
    # Only the observed resolver consequence and ordinary Chocolatey framing
    # may accompany that failure. ANY other cause, even alongside HTTP503,
    # refuses retry rather than relying on an incomplete error blacklist.
    if ($line -cmatch '^Chocolatey v[0-9]+(?:\.[0-9]+){1,3}$' -or
        $line -ceq 'Installing the following packages:' -or $line -ceq 'nsis' -or
        $line -ceq 'By installing, you accept licenses for the packages.' -or
        $line -ceq 'Need to add specific handling for exception type NuGetResolverInputException' -or
        $line -ceq "Unable to find package 'nsis'. Existing packages must be restored before performing an install or update." -or
        $line -ceq 'Chocolatey installed 0/0 packages.' -or
        $line -ceq 'See the log for details (C:\ProgramData\chocolatey\logs\chocolatey.log).') { continue }
    return $false
  }
  return $found
}
function Invoke-PickleNsisNative([string]$Executable,[string[]]$Arguments,[int]$TimeoutSeconds,[bool]$KillTree) {
  $work=Join-Path ([IO.Path]::GetTempPath()) ('pickle-nsis-'+[guid]::NewGuid().ToString('N'))
  $null=New-Item -ItemType Directory -Path $work -ErrorAction Stop
  $process=$null;$cleanupFailed=$false;$timedOut=$false;$overflow=$false;$output='';$code=$null
  $commandDeadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  try {
    try {
      $process=Start-Process -FilePath $Executable -ArgumentList $Arguments -RedirectStandardOutput (Join-Path $work 'stdout') -RedirectStandardError (Join-Path $work 'stderr') -NoNewWindow -PassThru -ErrorAction Stop
      $null=$process.Handle # Retain the exact process even if it exits immediately.
      if (!$process.WaitForExit([int][Math]::Max(0,($commandDeadline-[DateTime]::UtcNow).TotalMilliseconds))) { $timedOut=$true }
    } catch { $cleanupFailed=$true }
    finally {
      if ($process) {
        try {
          $drainDeadline=[DateTime]::UtcNow.AddSeconds(5)
          if (!$process.HasExited) {
            try { if ($KillTree) { $process.Kill($true) } else { $process.Kill() } } catch { $cleanupFailed=$true }
            if (!$process.WaitForExit([int][Math]::Max(0,($drainDeadline-[DateTime]::UtcNow).TotalMilliseconds))) { $cleanupFailed=$true }
          } elseif ($commandDeadline -lt $drainDeadline) { $drainDeadline=$commandDeadline }
          if ($process.HasExited) {
            $code=$process.ExitCode
            # Timed WaitForExit alone need not finish redirected-output events.
            # Core's async completion includes EOF; never use an infinite wait.
            if (!$process.WaitForExitAsync().Wait([int][Math]::Max(0,($drainDeadline-[DateTime]::UtcNow).TotalMilliseconds))) { $cleanupFailed=$true }
          }
        } catch { $cleanupFailed=$true }
        finally { $process.Dispose() }
      }
    }
    foreach ($name in @('stdout','stderr')) {
      $file=Join-Path $work $name
      if (!(Test-Path -LiteralPath $file -PathType Leaf)) { $cleanupFailed=$true;continue }
      $stream=$null;$bytes=[IO.MemoryStream]::new()
      try {
        $stream=[IO.File]::OpenRead($file);$buffer=New-Object byte[] 4096
        # Read at most cap+1, even if an inherited writer grows the file after
        # the root exits. The extra byte establishes overflow without draining.
        while ($bytes.Length -le 512KB) {
          $read=$stream.Read($buffer,0,[int][Math]::Min($buffer.Length,512KB+1-$bytes.Length))
          if ($read -eq 0) { break }
          $bytes.Write($buffer,0,$read)
        }
        if ($bytes.Length -gt 512KB) { $overflow=$true }
        else { $output+=[Text.Encoding]::UTF8.GetString($bytes.ToArray())+"`n" }
      } catch { $cleanupFailed=$true }
      finally { if ($stream) { $stream.Dispose() };$bytes.Dispose() }
    }
  } finally {
    try { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction Stop } catch { $cleanupFailed=$true }
  }
  return [pscustomobject]@{ExitCode=$code;TimedOut=$timedOut;CleanupFailed=$cleanupFailed;Overflow=$overflow;Output=$output}
}
function Write-PickleNsisAttemptSummary([string]$Stage,$Result,[bool]$Transient) {
  $code=if ($null -eq $Result.ExitCode) { 'unavailable' } else { [string][int]$Result.ExitCode }
  Write-Host ("NSIS {0}: exit={1}; timeout={2}; overflow={3}; cleanup_failed={4}; official_feed_transient={5}." -f $Stage,$code,[bool]$Result.TimedOut,[bool]$Result.Overflow,[bool]$Result.CleanupFailed,$Transient)
}
function Install-PickleNsis(
  [scriptblock]$Run,
  [scriptblock]$FindCompiler={ Get-PickleNsisCompiler },
  [scriptblock]$FindChocolatey={ (Get-Command choco.exe -CommandType Application -ErrorAction Stop).Source },
  [scriptblock]$Delay={ param($Seconds) Start-Sleep -Seconds $Seconds }
) {
  if (!$Run) {
    if ($PSEdition -cne 'Core' -or [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'NSIS setup requires PowerShell Core on Windows.' }
    $Run={ param($Executable,$Arguments,$TimeoutSeconds,$KillTree) Invoke-PickleNsisNative $Executable $Arguments $TimeoutSeconds $KillTree }
  }
  $choco=& $FindChocolatey
  if (!$choco) { throw 'Chocolatey is required for the NSIS prerequisite.' }
  for ($attempt=1;$attempt -le 3;$attempt++) {
    try { $result=& $Run $choco @('install','nsis','--yes','--no-progress') 90 $true }
    catch {
      Write-PickleNsisAttemptSummary "attempt $attempt of 3" ([pscustomobject]@{ExitCode=$null;TimedOut=$false;Overflow=$false;CleanupFailed=$true}) $false
      throw 'NSIS prerequisite command could not start or collect safely.'
    }
    $transient=Test-PickleNsisTransientFailure $result.ExitCode $result.Output
    Write-PickleNsisAttemptSummary "attempt $attempt of 3" $result $transient
    if ($result.TimedOut -or $result.CleanupFailed -or $result.Overflow -or $null -eq $result.ExitCode) { throw 'NSIS prerequisite command timed out or could not be collected and stopped safely.' }
    if ($result.ExitCode -eq 0) {
      $compiler=& $FindCompiler
      if (!$compiler) { throw 'Chocolatey succeeded but the NSIS compiler is unavailable.' }
      try { $version=& $Run $compiler @('/VERSION') 5 $false }
      catch {
        Write-PickleNsisAttemptSummary 'compiler probe' ([pscustomobject]@{ExitCode=$null;TimedOut=$false;Overflow=$false;CleanupFailed=$true}) $false
        throw 'The NSIS compiler version probe could not start or collect safely.'
      }
      Write-PickleNsisAttemptSummary 'compiler probe' $version $false
      if ($version.TimedOut -or $version.CleanupFailed -or $version.Overflow -or $null -eq $version.ExitCode -or $version.ExitCode -ne 0 -or $version.Output.Trim() -notmatch '^v[0-9][^\s]{0,63}$') { throw 'The NSIS compiler version probe did not succeed.' }
      Write-Host 'PASS: NSIS prerequisite native exit, compiler discovery and version probe.'
      return
    }
    if ($attempt -eq 3 -or !$transient) { throw 'NSIS prerequisite installation failed; no build was started.' }
    $seconds=if ($attempt -eq 1) { 3 } else { 6 }
    Write-Host "NSIS official feed returned a transient HTTP error; retrying attempt $($attempt+1) of 3."
    & $Delay $seconds
  }
}
