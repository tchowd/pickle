# Installed identity-enabled owned-window diagnostic. No target/window inputs.
function Initialize-PickleCapturePipe {
  if ('PickleCapturePipe' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Threading.Tasks;
public sealed class PickleCapturePipe {
  readonly byte[] stored;
  int count;
  public volatile bool Overflow;
  public volatile bool Failed;
  public Task Completion;
  PickleCapturePipe(int limit) { stored=new byte[limit]; }
  public static PickleCapturePipe Start(Stream stream,int limit) {
    var reader=new PickleCapturePipe(limit);
    reader.Completion=reader.Drain(stream);
    return reader;
  }
  async Task Drain(Stream stream) {
    try {
      var buffer=new byte[1024];
      for (;;) {
        int read=await stream.ReadAsync(buffer,0,buffer.Length).ConfigureAwait(false);
        if (read==0) break;
        int room=stored.Length-count;
        int keep=Math.Min(room,read);
        Array.Copy(buffer,0,stored,count,keep);
        count+=keep;
        if (keep!=read) Overflow=true;
      }
    } catch { Failed=true; }
  }
  public byte[] Bytes() {
    if (!Completion.IsCompleted) throw new InvalidOperationException("Pipe has not ended.");
    var copy=new byte[count];Array.Copy(stored,copy,count);return copy;
  }
}
'@
}

function Assert-PickleWindowCaptureReport([string]$Text) {
  if ([Text.Encoding]::UTF8.GetByteCount($Text) -gt 16384) { throw 'Owned capture report exceeded its bound.' }
  $report=$Text | ConvertFrom-Json -ErrorAction Stop
  $required=@('schema','test','passed','package_identity','production_window_capture','expected_word_matched','jpeg_bytes','width','height','dark_pixels','painted_before_capture','owned_window_closed','fixture_thread_joined','fixture_process_stopped','native_capture')
  $actual=@($report.PSObject.Properties.Name)
  if ($actual.Count -ne $required.Count -or @($actual | Where-Object { $_ -cnotin $required }).Count) { throw 'Owned capture report fields did not match the fixed contract.' }
  if (($report.schema -isnot [int] -and $report.schema -isnot [long]) -or $report.schema -ne 1 -or $report.test -cne 'windows-owned-window-capture') { throw 'Owned capture report schema did not match.' }
  foreach ($key in @('passed','package_identity','production_window_capture','expected_word_matched','painted_before_capture','owned_window_closed','fixture_thread_joined','fixture_process_stopped')) {
    if ($report.$key -isnot [bool] -or !$report.$key) { throw 'Owned capture did not verify every required native step and cleanup.' }
  }
  foreach ($key in @('jpeg_bytes','width','height','dark_pixels')) {
    if (($report.$key -isnot [int] -and $report.$key -isnot [long]) -or $report.$key -le 0) { throw 'Owned capture dimensions or image counters were invalid.' }
  }
  if ($report.width -ne 768 -or $report.height -ne 180 -or $report.jpeg_bytes -gt 8MB -or $report.dark_pixels -gt ($report.width*$report.height)) { throw 'Owned capture image metadata exceeded the fixed fixture bounds.' }
  $native=$report.native_capture
  $nativeKeys=@('stage','hresult','supported','frame_arrived','width','height','driver_type')
  $actualNative=@($native.PSObject.Properties.Name)
  if ($actualNative.Count -ne $nativeKeys.Count -or @($actualNative | Where-Object { $_ -cnotin $nativeKeys }).Count) { throw 'Native capture observation fields did not match.' }
  foreach ($key in @('hresult','width','height','driver_type')) {
    if ($native.$key -isnot [int] -and $native.$key -isnot [long]) { throw 'Native capture counters must be integers.' }
  }
  if ($native.stage -cne 'complete' -or $native.hresult -ne 0 -or $native.supported -isnot [bool] -or !$native.supported -or $native.frame_arrived -isnot [bool] -or !$native.frame_arrived -or $native.width -ne $report.width -or $native.height -ne $report.height -or $native.driver_type -ne 1) { throw 'Native hardware capture and frame arrival were not verified.' }
  return $report
}

function Invoke-PickleWindowCaptureProbe([string]$Application,[string]$EvidencePath) {
  Initialize-PickleCapturePipe
  $expected=[IO.Path]::GetFullPath((Resolve-Path -LiteralPath $Application).Path)
  if (Test-Path -LiteralPath $EvidencePath) { Remove-Item -LiteralPath $EvidencePath }
  $stderrPath=[IO.Path]::ChangeExtension($EvidencePath,'.stderr.log')
  $start=[Diagnostics.ProcessStartInfo]::new()
  $start.FileName=$expected;$start.Arguments='--verify-windows-capture'
  $start.UseShellExecute=$false;$start.CreateNoWindow=$true
  $start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
  $process=$null;$output=$null;$errors=$null;$text=$null;$verified=$false
  try {
    $process=[Diagnostics.Process]::Start($start)
    # Retain this exact child handle through all waits and forced cleanup.
    $null=$process.Handle
    $output=[PickleCapturePipe]::Start($process.StandardOutput.BaseStream,16384)
    $errors=[PickleCapturePipe]::Start($process.StandardError.BaseStream,16384)
    $owned=$false;$timer=[Diagnostics.Stopwatch]::StartNew()
    while (!$process.HasExited) {
      if (!$owned) {
        try { $path=$process.MainModule.FileName } catch { $path=$null }
        if ($path) {
          if (![string]::Equals([IO.Path]::GetFullPath($path),$expected,[StringComparison]::OrdinalIgnoreCase)) { throw 'The capture diagnostic did not own the exact installed executable.' }
          $owned=$true
        }
      }
      if ($output.Overflow -or $errors.Overflow) { throw 'Owned capture diagnostic output exceeded its bound.' }
      if ($timer.ElapsedMilliseconds -ge 45000) { throw 'Owned capture diagnostic exceeded 45 seconds.' }
      if ($process.WaitForExit(50)) { break }
    }
    if (!$owned) { throw 'Installed executable ownership was not observed before the diagnostic exited.' }
    if (!$output.Completion.Wait(3000) -or !$errors.Completion.Wait(3000)) { throw 'Owned capture diagnostic pipes did not reach EOF.' }
    if ($output.Failed -or $errors.Failed -or $output.Overflow -or $errors.Overflow) { throw 'Owned capture diagnostic output was incomplete or exceeded its bound.' }
    if ($process.ExitCode -ne 0) { throw 'Installed owned-window capture diagnostic failed. See its bounded stderr category.' }
    $utf8=[Text.UTF8Encoding]::new($false,$true)
    $text=$utf8.GetString($output.Bytes())
    $null=Assert-PickleWindowCaptureReport $text
    $verified=$true
  } finally {
    if ($null -ne $process) {
      try {
        if (!$process.HasExited) {
          $process.Kill()
          if (!$process.WaitForExit(10000)) { throw 'Exact owned capture diagnostic process did not stop.' }
        }
        foreach ($pipe in @($output,$errors)) {
          if ($null -ne $pipe -and !$pipe.Completion.Wait(3000)) { $verified=$false }
        }
        if ($null -ne $errors -and $errors.Completion.IsCompleted) { [IO.File]::WriteAllBytes($stderrPath,$errors.Bytes()) }
      } finally { $process.Dispose() }
    }
  }
  if (!$verified) { throw 'Owned capture diagnostic cleanup or EOF was not verified.' }
  # Publish only the actual successful app report, after process exit + EOF.
  [IO.File]::WriteAllText($EvidencePath,$text,[Text.UTF8Encoding]::new($false))
}
