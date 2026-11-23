# Installed-process failure acceptance only. No test branch or synthetic dialog is added to the app.
function Get-PickleRetainedState([string]$Directory) {
  $state = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::Ordinal)
  if (!(Test-Path -LiteralPath $Directory)) { return ,$state }
  $root = Get-Item -LiteralPath $Directory -Force
  if (!$root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The smoke data directory must be a regular directory.' }
  $pending = [Collections.Generic.Queue[string]]::new()
  $pending.Enqueue($root.FullName)
  $count = 0; $bytes = 0L
  while ($pending.Count) {
    foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
      $count++
      if ($count -gt 512 -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The smoke data inventory exceeded its bounds or found an unsafe link.' }
      if ($item.PSIsContainer) { $pending.Enqueue($item.FullName); continue }
      $relative = $item.FullName.Substring($root.FullName.TrimEnd([IO.Path]::DirectorySeparatorChar).Length + 1)
      # These synchronization files have no persistent reader/settings content.
      if ($relative -cin @('instance.lock','instance.json','window.lock',('BrowserBridge'+[IO.Path]::DirectorySeparatorChar+'connection.json'))) { continue }
      $bytes += $item.Length
      if ($bytes -gt 64MB) { throw 'The smoke retained-data inventory exceeded 64 MiB.' }
      $state.Add($relative, ($item.Length.ToString() + ':' + (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash))
    }
  }
  return ,$state
}

function Assert-PickleRetainedState($Before, $After) {
  if ($Before.Count -ne $After.Count) { throw 'Startup failure changed the retained data inventory.' }
  foreach ($name in $Before.Keys) {
    if (!$After.ContainsKey($name) -or $Before[$name] -cne $After[$name]) { throw 'Startup failure changed retained application data.' }
  }
}

function Invoke-PickleBoundedProbe([string]$Application, [string]$Argument) {
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $Application
  $start.Arguments = $Argument
  $start.UseShellExecute = $false
  $process = [Diagnostics.Process]::Start($start)
  try {
    if (!$process.WaitForExit(10000)) { throw 'The installed maintenance probe exceeded ten seconds.' }
    return $process.ExitCode
  } finally {
    if (!$process.HasExited) { $process.Kill(); $process.WaitForExit(10000) | Out-Null }
    $process.Dispose()
  }
}

function New-PickleFailureProcessStart([string]$Application) {
  $start = [Diagnostics.ProcessStartInfo]::new()
  $start.FileName = $Application
  $start.Arguments = '--sample'
  $start.UseShellExecute = $false
  # Environment changes belong only to this owned child, never the shell or recovery launch.
  $start.EnvironmentVariables['RUNE_GPU_BACKEND'] = 'wgpu'
  $start.EnvironmentVariables['WGPU_ADAPTER_NAME'] = 'Pickle-CI-intentionally-missing-adapter-' + [guid]::NewGuid().ToString('N')
  return $start
}

function Initialize-PickleStartupDialogProbe {
  if ('PickleStartupDialogProbe' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public sealed class PickleStartupChildEvidence {
  public IntPtr Handle;
  public uint ProcessId;
  public IntPtr Parent;
  public int Id;
  public string ClassName;
  public bool Visible;
  public bool Enabled;
}
public sealed class PickleStartupDialogEvidence {
  public IntPtr Handle;
  public uint ProcessId;
  public string Title;
  public string ClassName;
  public IntPtr ButtonHandle;
  public uint ButtonProcessId;
  public int ButtonId;
  public string ButtonClass;
  public bool ButtonVisible;
  public bool ButtonEnabled;
  public bool ButtonHasExpectedParent;
  public PickleStartupChildEvidence[] Children = new PickleStartupChildEvidence[0];
  public int RejectedProcessChildren;
  public bool InventoryTruncated;
  public int ButtonCandidates;
}
public static class PickleStartupDialogProbe {
  delegate bool WindowCallback(IntPtr window, IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumWindows(WindowCallback callback, IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, WindowCallback callback, IntPtr data);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
  [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
  [DllImport("user32.dll")] static extern bool IsWindowEnabled(IntPtr window);
  [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr window);
  [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr window);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr window, StringBuilder title, int capacity);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr window, StringBuilder name, int capacity);
  [DllImport("user32.dll", SetLastError=true)] static extern bool PostMessageW(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);
  public const string ExpectedTitle = "Pickle could not open the reader";
  public static bool Eligible(uint actual, uint expected, bool visible, string title, string className) {
    return actual==expected && visible && String.Equals(title,ExpectedTitle,StringComparison.Ordinal) && String.Equals(className,"#32770",StringComparison.Ordinal);
  }
  public static bool EligibleButton(uint actual, uint expected, bool visible, bool enabled, int id, string className, bool correctParent) {
    return actual==expected && visible && enabled && id>0 && id<=0xffff && correctParent && String.Equals(className,"Button",StringComparison.OrdinalIgnoreCase);
  }
  public static PickleStartupChildEvidence SoleButton(PickleStartupChildEvidence[] children, IntPtr parent, uint expected, bool truncated) {
    PickleStartupChildEvidence selected=null;
    if (truncated) return null;
    foreach (var child in children) {
      if (!EligibleButton(child.ProcessId,expected,child.Visible,child.Enabled,child.Id,child.ClassName,child.Parent==parent)) continue;
      if (selected!=null) return null;
      selected=child;
    }
    return selected;
  }
  public static bool Ready(PickleStartupDialogEvidence dialog, uint expected) {
    return dialog!=null && !dialog.InventoryTruncated && dialog.ButtonCandidates==1 && dialog.ButtonHandle!=IntPtr.Zero && EligibleButton(dialog.ButtonProcessId,expected,dialog.ButtonVisible,dialog.ButtonEnabled,dialog.ButtonId,dialog.ButtonClass,dialog.ButtonHasExpectedParent);
  }
  static PickleStartupDialogEvidence Read(IntPtr window, uint expected) {
    uint owner; GetWindowThreadProcessId(window,out owner);
    if (owner!=expected || !IsWindowVisible(window)) return null;
    var title=new StringBuilder(256); GetWindowTextW(window,title,title.Capacity);
    var name=new StringBuilder(256); GetClassNameW(window,name,name.Capacity);
    if (!Eligible(owner,expected,true,title.ToString(),name.ToString())) return null;
    var dialog=new PickleStartupDialogEvidence { Handle=window,ProcessId=owner,Title=title.ToString(),ClassName=name.ToString() };
    var children=new List<PickleStartupChildEvidence>();
    int visited=0;
    // EnumChildWindows includes nested descendants. Record only this PID's
    // controls, and require a direct parent match before selecting any action.
    EnumChildWindows(window,delegate(IntPtr child, IntPtr data) {
      if (++visited>32) { dialog.InventoryTruncated=true; return false; }
      uint childOwner; GetWindowThreadProcessId(child,out childOwner);
      if (childOwner!=expected) { dialog.RejectedProcessChildren++; return true; }
      var childClass=new StringBuilder(128); GetClassNameW(child,childClass,childClass.Capacity);
      children.Add(new PickleStartupChildEvidence { Handle=child,ProcessId=childOwner,Parent=GetParent(child),Id=GetDlgCtrlID(child),ClassName=childClass.ToString(),Visible=IsWindowVisible(child),Enabled=IsWindowEnabled(child) });
      return true;
    },IntPtr.Zero);
    dialog.Children=children.ToArray();
    foreach (var child in dialog.Children) {
      if (EligibleButton(child.ProcessId,expected,child.Visible,child.Enabled,child.Id,child.ClassName,child.Parent==window)) dialog.ButtonCandidates++;
    }
    var button=SoleButton(dialog.Children,window,expected,dialog.InventoryTruncated);
    if (button!=null) {
      dialog.ButtonHandle=button.Handle;dialog.ButtonProcessId=button.ProcessId;dialog.ButtonId=button.Id;dialog.ButtonClass=button.ClassName;dialog.ButtonVisible=button.Visible;dialog.ButtonEnabled=button.Enabled;dialog.ButtonHasExpectedParent=true;
    }
    return dialog;
  }
  public static PickleStartupDialogEvidence[] Inspect(uint expected) {
    var found=new List<PickleStartupDialogEvidence>();
    bool enumerated=EnumWindows(delegate(IntPtr window, IntPtr data) {
      var evidence=Read(window,expected);
      if (evidence!=null) found.Add(evidence);
      return true;
    },IntPtr.Zero);
    if (!enumerated) throw new InvalidOperationException("Could not enumerate the owned startup-failure dialog.");
    return found.ToArray();
  }
  public static bool Dismiss(IntPtr window, uint expected) {
    // Recheck both the exact dialog and its sole real action button. WM_COMMAND's
    // control notification requires the source control HWND in lParam; zero
    // describes a menu command, not BN_CLICKED from this button.
    var dialog=Read(window,expected);
    return Ready(dialog,expected) && PostMessageW(window,0x111,new UIntPtr((uint)dialog.ButtonId),dialog.ButtonHandle);
  }
  public static bool StillExists(IntPtr window, uint expected) {
    uint owner; GetWindowThreadProcessId(window,out owner);
    return owner==expected && IsWindow(window);
  }
}
'@
}

function ConvertTo-PickleDialogEvidence($Dialog) {
  $children = @($Dialog.Children | ForEach-Object {
    [ordered]@{handle=('0x'+$_.Handle.ToInt64().ToString('x'));process_id=$_.ProcessId;parent=('0x'+$_.Parent.ToInt64().ToString('x'));id=$_.Id;class=$_.ClassName;visible=$_.Visible;enabled=$_.Enabled;direct_parent=($_.Parent -eq $Dialog.Handle)}
  })
  return [ordered]@{process_id=$Dialog.ProcessId;handle=('0x'+$Dialog.Handle.ToInt64().ToString('x'));title=$Dialog.Title;class=$Dialog.ClassName;button=[ordered]@{handle=('0x'+$Dialog.ButtonHandle.ToInt64().ToString('x'));process_id=$Dialog.ButtonProcessId;id=$Dialog.ButtonId;class=$Dialog.ButtonClass;visible=$Dialog.ButtonVisible;enabled=$Dialog.ButtonEnabled;correct_parent=$Dialog.ButtonHasExpectedParent};children=$children;rejected_process_children=$Dialog.RejectedProcessChildren;inventory_truncated=$Dialog.InventoryTruncated;button_candidates=$Dialog.ButtonCandidates}
}

function Assert-PickleInstalledStartupFailure([string]$Application, [string]$DataDirectory, [string]$EvidencePath, [string]$Configuration) {
  if (!(Test-Path -LiteralPath $Application -PathType Leaf)) { throw 'The installed startup probe needs the actual application executable.' }
  if ((Invoke-PickleBoundedProbe $Application '--check-running') -ne 0) { throw 'Startup-failure acceptance requires an idle instance lease.' }
  Initialize-PickleStartupDialogProbe
  New-Item -ItemType Directory -Path $DataDirectory -Force | Out-Null
  $marker = Join-Path $DataDirectory ('startup-smoke-retained-' + [guid]::NewGuid().ToString('N') + '.txt')
  $markerText = 'Fixed owned startup-failure retention fixture.'
  [IO.File]::WriteAllText($marker,$markerText)
  $process = $null
  $evidence = [ordered]@{schema=1;test='windows-installed-startup-failure';passed=$false;configuration=$Configuration;executable=$Application;process_id=$null;dialog=$null;lease_held_during_dialog=$false;acknowledgement_enqueued=$false;dismissed=$false;dialog_closed=$false;exit_code=$null;lease_released=$false;control_removed=$false;bridge_control_removed=$false;retained_data_unchanged=$false}
  try {
    $before = Get-PickleRetainedState $DataDirectory
    $start = New-PickleFailureProcessStart $Application
    $process = [Diagnostics.Process]::Start($start)
    $evidence.process_id = $process.Id
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    $dialog = $null
    $ready = $false
    while ([DateTime]::UtcNow -lt $deadline) {
      $process.Refresh()
      if ($process.HasExited) { throw "The installed process exited before its native error dialog (status $($process.ExitCode))." }
      if (![string]::Equals([IO.Path]::GetFullPath($process.MainModule.FileName),[IO.Path]::GetFullPath($Application),[StringComparison]::OrdinalIgnoreCase)) { throw 'The startup-failure process does not own the expected installed executable.' }
      $dialogs = @([PickleStartupDialogProbe]::Inspect([uint32]$process.Id))
      if ($dialogs.Count -gt 1) { throw 'The owned startup failure displayed more than one matching error dialog.' }
      if ($dialogs.Count -eq 1) {
        $dialog=$dialogs[0]
        $evidence.dialog = ConvertTo-PickleDialogEvidence $dialog
        if ([PickleStartupDialogProbe]::Ready($dialog,[uint32]$process.Id)) { $ready=$true; break }
      }
      Start-Sleep -Milliseconds 100
    }
    if (!$dialog) { throw 'The installed app did not show its exact native startup-failure dialog within 30 seconds.' }
    if (!$ready) {
      $evidence.failure_stage = 'dialog-control-readiness'
      throw 'The exact owned fatal dialog had no unique safe action button within the original 30-second startup deadline. See its bounded child-control inventory.'
    }
    # This maintenance command only inspects the lock; it must not send Reopen.
    # The executable remains loaded until acknowledgement, so NSIS must still
    # observe its running guard even though all background writers have stopped.
    $dialogLease = Invoke-PickleBoundedProbe $Application '--check-running'
    $process.Refresh()
    if ($process.HasExited) { throw 'The startup-failure process exited before its dialog was acknowledged.' }
    $stillVisible = @([PickleStartupDialogProbe]::Inspect([uint32]$process.Id) | Where-Object { $_.Handle -eq $dialog.Handle })
    if ($stillVisible.Count -ne 1) { throw 'The exact owned fatal dialog did not remain visible during its running-lease probe.' }
    $evidence.dialog = ConvertTo-PickleDialogEvidence $stillVisible[0]
    if ($dialogLease -ne 10) { throw 'The visible native fatal dialog released its instance lease before acknowledgement.' }
    $evidence.lease_held_during_dialog = $true
    $exitTimer = [Diagnostics.Stopwatch]::StartNew()
    if (![PickleStartupDialogProbe]::Dismiss($dialog.Handle,[uint32]$process.Id)) {
      $evidence.failure_stage = 'dialog-acknowledgement'
      throw 'The owned startup-failure dialog could not be dismissed safely.'
    }
    $evidence.acknowledgement_enqueued = $true
    while ($exitTimer.ElapsedMilliseconds -lt 15000) {
      $process.Refresh()
      if ($process.HasExited) { $evidence.dialog_closed=$true; break }
      if (![PickleStartupDialogProbe]::StillExists($dialog.Handle,[uint32]$process.Id)) { $evidence.dialog_closed=$true }
      $wait = [Math]::Min(100,[Math]::Max(1,15000-$exitTimer.ElapsedMilliseconds))
      if ($process.WaitForExit([int]$wait)) { $evidence.dialog_closed=$true; break }
    }
    $evidence.dismissed = $evidence.dialog_closed
    $process.Refresh()
    if (!$process.HasExited) {
      if (!$evidence.dialog_closed) {
        $evidence.failure_stage = 'dialog-acknowledgement'
        throw 'The exact owned fatal dialog still existed after its owned-button notification; the process did not exit within 15 seconds.'
      }
      $evidence.failure_stage = 'post-dialog-exit'
      throw 'The exact owned fatal dialog closed, but the installed application did not exit within 15 seconds.'
    }
    $evidence.exit_code = $process.ExitCode
    if ($process.ExitCode -eq 0) { throw 'Native startup failure incorrectly returned a successful exit status.' }
    if ((Invoke-PickleBoundedProbe $Application '--check-running') -ne 0) { throw 'The startup-failure process retained its instance lease.' }
    $evidence.lease_released = $true
    if (Test-Path -LiteralPath (Join-Path $DataDirectory 'instance.json')) { throw 'The startup-failure process left its private reopen endpoint behind.' }
    $evidence.control_removed = $true
    if (Test-Path -LiteralPath (Join-Path $DataDirectory 'BrowserBridge/connection.json')) { throw 'The startup-failure process left its browser bridge endpoint behind.' }
    $evidence.bridge_control_removed = $true
    Assert-PickleRetainedState $before (Get-PickleRetainedState $DataDirectory)
    $evidence.retained_data_unchanged = $true
    $evidence.passed = $true
  } catch {
    $evidence.failure = $_.Exception.Message
    throw
  } finally {
    try {
      if ($null -ne $process) {
        $process.Refresh()
        if (!$process.HasExited) { $process.Kill(); $process.WaitForExit(10000) | Out-Null }
        $process.Dispose()
      }
      if ((Test-Path -LiteralPath $marker -PathType Leaf) -and [IO.File]::ReadAllText($marker) -ceq $markerText) { Remove-Item -LiteralPath $marker }
    } catch {
      $evidence.passed=$false
      $evidence.cleanup_failure='The owned startup-smoke process or marker could not be cleaned up.'
      throw
    } finally {
      # Keep the bounded owned-control inventory even if failure cleanup fails.
      $evidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $EvidencePath -Encoding utf8
    }
  }
}
