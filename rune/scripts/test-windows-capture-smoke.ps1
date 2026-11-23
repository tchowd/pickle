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
  @{key='schema';value=1.0},
  @{key='native_capture';value=@{stage='complete';hresult=0;supported=$true;frame_arrived=$false;width=768;height=180;driver_type=1}},
  @{key='source_text';value='forbidden extra field'}
)) {
  $bad=[ordered]@{};foreach ($key in $valid.Keys) { $bad[$key]=$valid[$key] };$bad[$change.key]=$change.value
  $rejected=$false
  try { $null=Assert-PickleWindowCaptureReport ($bad | ConvertTo-Json -Compress) } catch { $rejected=$true }
  if (!$rejected) { throw "Invalid controlled report was admitted: $($change.key)" }
}
Initialize-PickleCapturePipe
$stream=[IO.MemoryStream]::new([byte[]]::new(25000))
try {
  $pipe=[PickleCapturePipe]::Start($stream,16384)
  if (!$pipe.Completion.Wait(3000) -or !$pipe.Overflow -or $pipe.Failed -or $pipe.Bytes().Length -ne 16384) { throw 'Bounded output collector did not cap and fully drain its controlled input.' }
} finally { $stream.Dispose() }
Write-Host 'PASS: fixed capture report contract, false/ill-typed/extra-field refusals, bounded pipe storage and EOF.'
