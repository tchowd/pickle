# Uses only the exact process started by the package smoke. No agent tooling, screenshots, or inference.
if (-not ('PickleNativeWindowProbe' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public sealed class PickleNativeWindowEvidence {
  public IntPtr Handle;
  public uint ProcessId;
  public string Title;
  public bool Visible;
  public int Cloaked;
  public int Left;
  public int Top;
  public int Right;
  public int Bottom;
}
public static class PickleNativeWindowProbe {
  delegate bool WindowCallback(IntPtr window, IntPtr data);
  [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left,Top,Right,Bottom; }
  [DllImport("user32.dll")] static extern bool EnumWindows(WindowCallback callback, IntPtr data);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out Rect rect);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr window, StringBuilder title, int capacity);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr window, uint attribute, out int value, int size);
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam, uint flags, uint timeout, out UIntPtr result);
  public static bool Eligible(uint actual, uint expected, bool visible, int cloaked, int width, int height) {
    // Exclude tray tooltips/message-only windows; these are the app's declared minimum dimensions.
    return actual==expected && visible && cloaked==0 && width>=480 && height>=180;
  }
  public static PickleNativeWindowEvidence[] Inspect(uint expected) {
    var found=new List<PickleNativeWindowEvidence>();
    bool enumerated=EnumWindows(delegate(IntPtr window, IntPtr data) {
      uint owner; GetWindowThreadProcessId(window,out owner);
      if (owner!=expected) return true;
      Rect rect; if (!GetWindowRect(window,out rect)) return true;
      bool visible=IsWindowVisible(window);
      int cloaked;
      if (DwmGetWindowAttribute(window,14,out cloaked,4)!=0) cloaked=-1;
      if (!Eligible(owner,expected,visible,cloaked,rect.Right-rect.Left,rect.Bottom-rect.Top)) return true;
      var title=new StringBuilder(256); GetWindowTextW(window,title,title.Capacity);
      found.Add(new PickleNativeWindowEvidence { Handle=window,ProcessId=owner,Title=title.ToString(),Visible=visible,Cloaked=cloaked,Left=rect.Left,Top=rect.Top,Right=rect.Right,Bottom=rect.Bottom });
      return true;
    },IntPtr.Zero);
    if (!enumerated) throw new InvalidOperationException("Could not enumerate the owned application's native windows.");
    return found.ToArray();
  }
  public static bool Responsive(IntPtr window) {
    UIntPtr result;
    // WM_NULL; SMTO_BLOCK | SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT. Each call is bounded to two seconds.
    return SendMessageTimeoutW(window,0,UIntPtr.Zero,IntPtr.Zero,0x23,2000,out result)!=IntPtr.Zero;
  }
}
'@
}

function Assert-PickleNativeReleaseWindow([Diagnostics.Process]$Process, [string]$ExpectedExecutable, [string]$EvidencePath) {
  $evidence = [ordered]@{ schema=1; test='windows-native-release-window'; passed=$false; process_id=$Process.Id; executable=$ExpectedExecutable; mode='native-sample'; window=$null; responsiveness=@() }
  try {
    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    $window = $null
    while ([DateTime]::UtcNow -lt $deadline) {
      $Process.Refresh()
      if ($Process.HasExited) { throw "The owned native sample process exited before showing a window (status $($Process.ExitCode))." }
      $actual = $Process.MainModule.FileName
      if (![string]::Equals([IO.Path]::GetFullPath($actual),[IO.Path]::GetFullPath($ExpectedExecutable),[StringComparison]::OrdinalIgnoreCase)) { throw 'The native sample process does not own the expected installed executable.' }
      $windows = @([PickleNativeWindowProbe]::Inspect([uint32]$Process.Id))
      if ($windows.Count) { $window=$windows[0]; break }
      Start-Sleep -Milliseconds 100
    }
    if (!$window) { throw 'The installed release application did not expose a visible, uncloaked native window within 25 seconds.' }
    $evidence.window = [ordered]@{ handle=('0x' + $window.Handle.ToInt64().ToString('x')); title=$window.Title; process_id=$window.ProcessId; visible=$window.Visible; cloaked=$window.Cloaked; left=$window.Left; top=$window.Top; right=$window.Right; bottom=$window.Bottom }
    Start-Sleep -Milliseconds 1000
    foreach ($iteration in 1..2) {
      $Process.Refresh()
      if ($Process.HasExited) { throw 'The installed release application exited during its native window smoke.' }
      $stillOwned = @([PickleNativeWindowProbe]::Inspect([uint32]$Process.Id) | Where-Object { $_.Handle -eq $window.Handle })
      if ($stillOwned.Count -ne 1) { throw 'The owned sample window disappeared or became hidden/cloaked.' }
      $timer = [Diagnostics.Stopwatch]::StartNew()
      $responsive = [PickleNativeWindowProbe]::Responsive($window.Handle)
      $timer.Stop()
      $evidence.responsiveness += @{ passed=$responsive; elapsed_ms=$timer.ElapsedMilliseconds; timeout_ms=2000 }
      if (!$responsive) { throw 'The owned release window did not respond to its bounded WM_NULL probe.' }
      Start-Sleep -Milliseconds 500
    }
    $Process.Refresh()
    if ($Process.HasExited) { throw 'The release application exited before completion of its native window smoke.' }
    $evidence.passed = $true
  } catch {
    $evidence.failure = $_.Exception.Message
    throw
  } finally {
    $evidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $EvidencePath -Encoding utf8
  }
}
