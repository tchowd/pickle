$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'windows-capture-smoke.ps1')
$valid=[ordered]@{schema=1;test='windows-owned-window-capture';passed=$true;package_identity=$true;production_window_capture=$true;expected_word_matched=$true;jpeg_bytes=2000;width=768;height=180;dark_pixels=500;painted_before_capture=$true;owned_window_closed=$true;fixture_thread_joined=$true;fixture_process_stopped=$true;native_capture=[ordered]@{stage='complete';hresult=0;supported=$true;frame_arrived=$true;width=768;height=180;driver_type=1}}
$null=Assert-PickleWindowCaptureReport ($valid | ConvertTo-Json -Compress)
foreach ($change in @(
  @{key='fixture_process_stopped';value=$false},
  @{key='production_window_capture';value=1},
  @{key='width';value='768'},
  @{key='height';value=0},
  @{key='jpeg_bytes';value=9000000},
  @{key='dark_pixels';value=99999999},
  @{key='test';value='windows-ocr-fixed-fixture'},
  @{key='native_capture';value=@{stage='complete';hresult=0;supported=$true;frame_arrived=$false;width=768;height=180;driver_type=1}},
  @{key='source_text';value='forbidden extra field'}
)) {
  $bad=[ordered]@{};foreach ($key in $valid.Keys) { $bad[$key]=$valid[$key] };$bad[$change.key]=$change.value
  $rejected=$false
  try { $null=Assert-PickleWindowCaptureReport ($bad | ConvertTo-Json -Compress) } catch { $rejected=$true }
  if (!$rejected) { throw "Invalid controlled report was admitted: $($change.key)" }
}
# Windows PowerShell 5 serializes the double 1.0 as the integer token 1.
# Mutate the actual JSON wire token, rather than an object that the serializer
# can normalize back into a valid report before the validator sees it.
$validJson=$valid | ConvertTo-Json -Compress
if (!$validJson.Contains('"schema":1,')) { throw 'The controlled schema wire token was not found.' }
foreach ($literal in @('1.0','"1"','true')) {
  $token='"schema":'+$literal+','
  $raw=$validJson.Replace('"schema":1,',$token)
  if ($raw -ceq $validJson -or !$raw.Contains($token)) { throw 'The invalid controlled schema wire token was not preserved.' }
  $rejected=$false
  try { $null=Assert-PickleWindowCaptureReport $raw } catch { $rejected=$true }
  if (!$rejected) { throw "Invalid raw schema literal was admitted: $literal" }
}
Initialize-PickleCapturePipe
$stream=[IO.MemoryStream]::new([byte[]]::new(25000))
try {
  $pipe=[PickleCapturePipe]::Start($stream,16384)
  if (!$pipe.Completion.Wait(3000) -or !$pipe.Overflow -or $pipe.Failed -or !$pipe.Eof -or $pipe.StoredBytes -ne 16384 -or $pipe.Bytes().Length -ne 16384) { throw 'Bounded output collector did not cap and fully drain its controlled input.' }
} finally { $stream.Dispose() }
$fake=[pscustomobject]@{StoredBytes=0;Eof=$true;Overflow=$false;Failed=$false;unexpected='private text must not be copied'}
$record=New-PickleCaptureProcessReport -Stage 'exit-code' -Owned $true -Started $true -ExitCode -1073741819 -Exited $true -ForcedStop $false -CleanupFailed $false -Verified $false -Output $fake -Errors $fake
$wire=$record | ConvertTo-Json -Compress
$read=$wire | ConvertFrom-Json
if ($read.exit_code -ne -1073741819 -or ($read.exit_code -isnot [int] -and $read.exit_code -isnot [long]) -or !$read.owned -or !$read.process_exited -or $read.passed -or $read.forced_stop -or $read.stdout_bytes -ne 0 -or $read.stderr_bytes -ne 0 -or !$read.stdout_eof -or !$read.stderr_eof -or $wire.Contains('private') -or $wire.Contains('unexpected')) { throw 'Signed fatal exit or zero-byte EOF evidence was lost or arbitrary data copied.' }
$unfinished=[pscustomobject]@{StoredBytes=16384;Eof=$false;Overflow=$true;Failed=$true}
$record=New-PickleCaptureProcessReport -Stage 'pipe-wait' -Owned $true -Started $true -ExitCode $null -Exited $false -ForcedStop $true -CleanupFailed $true -Verified $false -Output $unfinished -Errors $null
if ($null -ne $record.exit_code -or $record.process_exited -or !$record.forced_stop -or !$record.cleanup_failed -or !$record.stdout_overflow -or !$record.stdout_failed -or $record.stdout_eof -or $record.stderr_bytes -ne 0 -or $record.stderr_eof) { throw 'Forced-stop/incomplete-pipe evidence incorrectly implied clean EOF.' }
$badStage=$false
try { $null=New-PickleCaptureProcessReport -Stage 'untrusted arbitrary message' } catch { $badStage=$true }
if (!$badStage) { throw 'A non-categorical process evidence stage was admitted.' }
# Both failures are owned and deterministic: this text file is not executable,
# and the evidence parent directory does not exist. No process or GUI is run.
# The second filesystem failure must not replace the Process.Start error.
$temp=Join-Path ([IO.Path]::GetTempPath()) ('pickle-capture-primary-'+[guid]::NewGuid().ToString('N'))
$null=[IO.Directory]::CreateDirectory($temp)
try {
  $invalid=Join-Path $temp 'invalid.exe'
  [IO.File]::WriteAllText($invalid,'owned invalid executable',[Text.UTF8Encoding]::new($false))
  $evidence=Join-Path (Join-Path $temp 'absent') 'capture.json'
  $preserved=$false
  try { Invoke-PickleWindowCaptureProbe -Application $invalid -EvidencePath $evidence }
  catch { $preserved=($_.InvocationInfo.Line.Contains('[Diagnostics.Process]::Start($start)')) }
  if (!$preserved -or (Test-Path -LiteralPath $evidence)) { throw 'Evidence-write failure replaced the original owned launch failure or published capture success.' }
} finally { Remove-Item -LiteralPath $temp -Recurse -Force }
Write-Host 'PASS: fixed capture contract, bounded collector/EOF, signed fatal exit and failure-only scalar process evidence.'
