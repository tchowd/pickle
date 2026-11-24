$ErrorActionPreference='Stop'
if ($PSEdition -cne 'Core') { throw 'NSIS prerequisite tests require PowerShell Core.' }
. (Join-Path $PSScriptRoot 'windows-nsis.ps1')
function New-NsisResult([int]$Code=0,[string]$Output='',[bool]$Timeout=$false,[bool]$CleanupFailed=$false,[bool]$Overflow=$false) {
  return [pscustomobject]@{ExitCode=$Code;Output=$Output;TimedOut=$Timeout;CleanupFailed=$CleanupFailed;Overflow=$Overflow}
}
function Invoke-NsisCase([object[]]$Results,[bool]$CompilerPresent=$true) {
  $state=[pscustomobject]@{Calls=[Collections.Generic.List[object]]::new();Sleeps=[Collections.Generic.List[int]]::new();Index=0;CompilerChecks=0;Passed=$false;Failure=$null}
  $run={ param($Executable,$Arguments,$TimeoutSeconds,$KillTree)
    $state.Calls.Add([pscustomobject]@{Executable=$Executable;Arguments=@($Arguments);Seconds=$TimeoutSeconds;Tree=$KillTree})
    if ($state.Index -ge $Results.Count) { throw 'Unexpected extra NSIS command.' }
    $value=$Results[$state.Index];$state.Index++;return $value
  }.GetNewClosure()
  $find={ $state.CompilerChecks++;if ($CompilerPresent) { return 'owned-makensis.exe' };return $null }.GetNewClosure()
  $delay={ param($Seconds) $state.Sleeps.Add($Seconds) }.GetNewClosure()
  try { Install-PickleNsis -Run $run -FindCompiler $find -FindChocolatey { 'owned-choco.exe' } -Delay $delay;$state.Passed=$true } catch { $state.Failure=$_.Exception.Message }
  return $state
}
$line="Failed to fetch results from V2 feed at 'https://community.chocolatey.org/api/v2/Packages(Id='nsis',Version='3.13.0')' with following message : Response status code does not indicate success: 503 (Service Unavailable)."
foreach ($pair in @(@('502','Bad Gateway'),@('503','Service Unavailable'),@('504','Gateway Timeout'))) {
  $transientLine=$line.Replace('503 (Service Unavailable)',($pair[0]+' ('+$pair[1]+')'))
  if (!(Test-PickleNsisTransientFailure 1 $transientLine)) { throw 'A verified transient official-feed failure was not recognized.' }
}
foreach ($output in @('503 Service Unavailable','unknown failure',$line.Replace('community.chocolatey.org','untrusted.invalid'),$line.Replace("Id='nsis'","Id='other'"),$line.Replace('503','404'),$line.Replace('https://','http://'),($line+"`nchecksum failed"),($line+"`nTLS handshake failed"),($line+"`nSSL error"),($line+"`nCertificate verification failed"),($line+"`nAccess is denied"),($line+"`nPermission error"),($line+"`nUnauthorized"),($line+"`nForbidden"),($line+"`nAuthentication failed"),($line+"`nAuthorization required"),($line+"`nHashes do not match"),($line+"`nThe system cannot find the path specified."),($line+"`nThere is not enough space on the disk."),($line+"`nUnknown additional failure"),($line+"`nResponse status code does not indicate success: 403 (Forbidden)."),('x'*(1MB+1)))) {
  if (Test-PickleNsisTransientFailure 1 $output) { throw 'A permanent, mixed, unknown or untrusted failure was retried.' }
}
$observed="Chocolatey v2.7.5`nInstalling the following packages:`nnsis`nBy installing, you accept licenses for the packages.`n"+$line+"`nNeed to add specific handling for exception type NuGetResolverInputException`nUnable to find package 'nsis'. Existing packages must be restored before performing an install or update.`nChocolatey installed 0/0 packages.`n See the log for details (C:\ProgramData\chocolatey\logs\chocolatey.log)."
if (!(Test-PickleNsisTransientFailure 1 $observed)) { throw 'The exact observed Chocolatey failure transcript was not eligible.' }
if (Test-PickleNsisTransientFailure 0 $line) { throw 'A successful native exit was classified as retryable.' }
$ok=New-NsisResult;$version=New-NsisResult 0 'v3.13';$transient=New-NsisResult 1 $line
$state=Invoke-NsisCase @($transient,$ok,$version)
if (!$state.Passed -or $state.Calls.Count -ne 3 -or $state.CompilerChecks -ne 1 -or ($state.Sleeps -join ',') -cne '3') { throw 'Transient failure then installation and version success did not follow the bounded sequence.' }
foreach ($call in @($state.Calls[0],$state.Calls[1])) {
  if ($call.Executable -cne 'owned-choco.exe' -or ($call.Arguments -join '|') -cne 'install|nsis|--yes|--no-progress' -or $call.Seconds -ne 90 -or !$call.Tree) { throw 'The existing Chocolatey argv/policy or timeout changed.' }
}
if ($state.Calls[2].Executable -cne 'owned-makensis.exe' -or ($state.Calls[2].Arguments -join '|') -cne '/VERSION' -or $state.Calls[2].Seconds -ne 5 -or $state.Calls[2].Tree) { throw 'Compiler probing did not remain a bounded read-only command.' }
$state=Invoke-NsisCase @($transient,$transient,$transient)
if ($state.Passed -or $state.Calls.Count -ne 3 -or $state.CompilerChecks -ne 0 -or ($state.Sleeps -join ',') -cne '3,6') { throw 'Transient exhaustion exceeded the exact attempt or delay budget.' }
foreach ($result in @((New-NsisResult 1 'unknown failure'),(New-NsisResult 1 (($line+"`nchecksum mismatch"))),(New-NsisResult 1 (($line+"`nTLS error"))),(New-NsisResult 1 $line $true),(New-NsisResult 1 $line $false $true),(New-NsisResult 1 $line $false $false $true))) {
  $state=Invoke-NsisCase @($result,$ok,$version)
  if ($state.Passed -or $state.Calls.Count -ne 1 -or $state.CompilerChecks -ne 0 -or $state.Sleeps.Count -ne 0) { throw 'A permanent/unsafe result was retried or reached compiler acceptance.' }
}
# A failed native install remains a failure even if a compiler happens to exist.
$state=Invoke-NsisCase @((New-NsisResult 1 'permanent refusal'),$version) $true
if ($state.Passed -or $state.Calls.Count -ne 1 -or $state.CompilerChecks -ne 0) { throw 'An existing compiler hid a failed install.' }
$state=Invoke-NsisCase @($ok,$version) $false
if ($state.Passed -or $state.Calls.Count -ne 1 -or $state.Sleeps.Count -ne 0) { throw 'Exit zero with missing compiler was accepted or retried.' }
foreach ($result in @((New-NsisResult 1 'v3.13'),(New-NsisResult 0 ''),(New-NsisResult 0 'v3.13' $true),(New-NsisResult 0 'v3.13' $false $true),(New-NsisResult 0 'v3.13' $false $false $true))) {
  $state=Invoke-NsisCase @($ok,$result)
  if ($state.Passed -or $state.Calls.Count -ne 2 -or $state.Sleeps.Count -ne 0) { throw 'A failed compiler probe was accepted or retried.' }
}
$startRefused=$false
try { Install-PickleNsis -Run { throw 'private arbitrary start detail' } -FindChocolatey { 'owned-choco.exe' } -Delay { throw 'No retry permitted.' } }
catch { $startRefused=$_.Exception.Message -ceq 'NSIS prerequisite command could not start or collect safely.' }
if (!$startRefused) { throw 'A start failure leaked arbitrary details or failed to stop.' }
# Bind the actual workflow callsites, and the early builder prerequisite before
# any Cargo work, rather than accepting a helper that packaging never calls.
$workflow=Get-Content -Raw (Join-Path $PSScriptRoot '../../.github/workflows/rune.yml')
$sequence='(?m)^\s+\./scripts/test-windows-nsis\.ps1\r?\n\s+\. \./scripts/windows-nsis\.ps1\r?\n\s+Install-PickleNsis\r?\n\s+\./scripts/build-windows\.ps1 -Configuration (debug|release) -Installer'
$sites=[regex]::Matches($workflow,$sequence)
if ($sites.Count -ne 2 -or @($sites | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique).Count -ne 2 -or $workflow -match 'choco install nsis') { throw 'Both actual workflow sites must use the tested prerequisite before their unchanged builders.' }
$builder=Get-Content -Raw (Join-Path $PSScriptRoot 'build-windows.ps1')
if ($builder.IndexOf('$makensis = Get-PickleNsisCompiler') -lt 0 -or $builder.IndexOf('$makensis = Get-PickleNsisCompiler') -gt $builder.IndexOf('$buildArgs =') -or $builder -notmatch '(?s)if \(\$Installer\) \{\s+\$makensis = Get-PickleNsisCompiler\s+if \(!\$makensis\)') { throw 'The installer compiler prerequisite must fail before Cargo.' }
# Actual owned headless CLI collection, independent of Chocolatey or NSIS.
# This attests quick-exit status, bounded bytes and timeout/root acknowledgement
# cross-platform; it does not claim Windows installer descendant semantics.
$hostExe=[Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
function Invoke-OwnedNsisCli([string]$Script,[int]$Seconds) {
  $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script))
  return Invoke-PickleNsisNative $hostExe @('-NoLogo','-NoProfile','-EncodedCommand',$encoded) $Seconds $true
}
$quick=Invoke-OwnedNsisCli "[Console]::Out.Write('v3.13'); exit 23" 5
if ($quick.ExitCode -ne 23 -or $quick.TimedOut -or $quick.CleanupFailed -or $quick.Overflow -or $quick.Output.Trim() -cne 'v3.13') { throw 'The retained exact quick-exit process lost its status or output.' }
$bounded=Invoke-OwnedNsisCli "[Console]::Out.Write(('x' * 600000))" 5
if ($bounded.ExitCode -ne 0 -or !$bounded.Overflow -or $bounded.CleanupFailed -or $bounded.Output.Length -gt 1MB) { throw ('The actual collector did not cap controlled output: exit='+$bounded.ExitCode+' timeout='+$bounded.TimedOut+' cleanup='+$bounded.CleanupFailed+' overflow='+$bounded.Overflow+' length='+$bounded.Output.Length) }
$elapsed=[Diagnostics.Stopwatch]::StartNew()
$timeout=Invoke-OwnedNsisCli 'Start-Sleep -Seconds 15' 1
$elapsed.Stop()
if (!$timeout.TimedOut -or $timeout.CleanupFailed -or $null -eq $timeout.ExitCode -or $timeout.Overflow -or $elapsed.ElapsedMilliseconds -gt 7000) { throw 'The owned headless timeout was not killed and acknowledged within its bound.' }
Write-Host ("PASS owned headless collector: quick_exit={0}; overflow={1}; timeout_exit={2}; elapsed_ms={3}; no installer or GUI." -f $quick.ExitCode,$bounded.Overflow,$timeout.ExitCode,$elapsed.ElapsedMilliseconds)
$summary=@(Write-PickleNsisAttemptSummary 'attempt 1 of 3' (New-NsisResult 23 'private arbitrary output') $false 6>&1) -join ''
if (!$summary.Contains('exit=23') -or !$summary.Contains('official_feed_transient=False') -or $summary.Contains('private') -or $summary.Contains('arbitrary')) { throw 'Fixed attempt diagnostics lost scalar status or exposed captured output.' }
Write-Host 'PASS: NSIS exact argv, transient-only retry, bounded attempts, permanent refusal, compiler prerequisite and both workflow callsites; owned headless collector checks passed; no installer or GUI executed.'
