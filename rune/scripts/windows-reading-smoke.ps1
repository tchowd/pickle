# Explicit installed-app acceptance using only owned synthetic text and a fixed HTTP fixture.
function Assert-PickleReadingDirectory([string]$Directory) {
  if (![IO.Path]::IsPathRooted($Directory)) { throw 'Reading smoke paths must be absolute.' }
  $full=[IO.Path]::GetFullPath($Directory)
  $part=$full
  while ($part) {
    if (Test-Path -LiteralPath $part) {
      $item=Get-Item -LiteralPath $part -Force
      if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Reading smoke directories must not traverse links or other file types.' }
    }
    $parent=[IO.Path]::GetDirectoryName($part)
    if ($parent -eq $part) { break };$part=$parent
  }
  return $full
}

function Read-PickleReadingBytes([string]$Path,[long]$Maximum) {
  $item=Get-Item -LiteralPath $Path -Force
  if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt $Maximum) { throw 'Reading smoke refused an unsafe or oversized file.' }
  $reader=[IO.File]::OpenRead($Path);$output=[IO.MemoryStream]::new()
  try {
    $buffer=[byte[]]::new(65536)
    while (($read=$reader.Read($buffer,0,$buffer.Length)) -gt 0) {
      if ($output.Length+$read -gt $Maximum) { throw 'Reading smoke file grew beyond its byte limit.' }
      $output.Write($buffer,0,$read)
    }
    return ,$output.ToArray()
  } finally { $reader.Dispose();$output.Dispose() }
}

function ConvertTo-PickleReadingRelative([string]$Path) {
  return $Path.Replace([char]92,[char]47)
}

function Get-PickleReadingFiles([string]$Directory) {
  $root=Assert-PickleReadingDirectory $Directory
  $files=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::OrdinalIgnoreCase)
  if (!(Test-Path -LiteralPath $root)) { return ,$files }
  $pending=[Collections.Generic.Queue[string]]::new();$pending.Enqueue($root)
  $count=0;$bytes=0L
  while ($pending.Count) {
    foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
      $count++
      if ($count -gt 512 -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Reading profile inventory exceeded its bound or found a link.' }
      if ($item.PSIsContainer) { $pending.Enqueue($item.FullName);continue }
      $relative=ConvertTo-PickleReadingRelative ($item.FullName.Substring($root.TrimEnd([IO.Path]::DirectorySeparatorChar).Length+1))
      if ($relative -cin @('instance.lock','instance.json','window.lock','store.lock','BrowserBridge/connection.json')) { continue }
      $bytes+=$item.Length
      if ($bytes -gt 64MB) { throw 'Reading profile inventory exceeds 64 MiB.' }
      $files.Add($relative,$item)
    }
  }
  return ,$files
}

function Save-PickleReadingProfile([string]$Directory,[string]$BackupDirectory) {
  $root=Assert-PickleReadingDirectory $Directory
  $backup=Assert-PickleReadingDirectory $BackupDirectory
  if ($backup.StartsWith($root.TrimEnd([IO.Path]::DirectorySeparatorChar)+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or $backup -eq $root) { throw 'The private backup must be outside the app profile.' }
  if (Test-Path -LiteralPath $backup) { throw 'Reading profile backup must be new.' }
  New-Item -ItemType Directory -Path $backup | Out-Null
  $saved=[Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
  $remaining=64MB
  foreach ($entry in (Get-PickleReadingFiles $root).GetEnumerator()) {
    $bytes=Read-PickleReadingBytes $entry.Value.FullName $remaining
    $remaining-=$bytes.Length
    $destination=Join-Path $backup $entry.Key
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
    [IO.File]::WriteAllBytes($destination,$bytes)
    $saved.Add($entry.Key,[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)))
  }
  return [pscustomobject]@{Directory=$root;Backup=$backup;Hashes=$saved}
}

function Write-PickleReadingAtomic([string]$Path,[byte[]]$Bytes) {
  $directory=Assert-PickleReadingDirectory ([IO.Path]::GetDirectoryName($Path))
  New-Item -ItemType Directory -Path $directory -Force | Out-Null
  if (Test-Path -LiteralPath $Path) {
    $item=Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Reading profile destination is unsafe.' }
  }
  $temporary=Join-Path $directory ('.reading-'+[guid]::NewGuid().ToString('N')+'.tmp')
  try { [IO.File]::WriteAllBytes($temporary,$Bytes);[IO.File]::Move($temporary,$Path,$true) }
  finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary } }
}

function Restore-PickleReadingProfile($Snapshot,[scriptblock]$IsIdle) {
  if (!(& $IsIdle)) { throw 'The owned app must be dead and its lease idle before restoring any profile bytes.' }
  $current=Get-PickleReadingFiles $Snapshot.Directory
  $verified=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::OrdinalIgnoreCase)
  $remaining=64MB
  foreach ($entry in $Snapshot.Hashes.GetEnumerator()) {
    $bytes=Read-PickleReadingBytes (Join-Path $Snapshot.Backup $entry.Key) $remaining
    $remaining-=$bytes.Length
    if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)) -cne $entry.Value) { throw 'The owned profile backup changed; no restoration mutation was performed.' }
    $verified.Add($entry.Key,$bytes)
  }
  # Only the known app outputs may be new. Preserve unexpected files and fail
  # restoration instead of deleting another actor's newly introduced data.
  foreach ($entry in $current.GetEnumerator()) {
    if (!$Snapshot.Hashes.ContainsKey($entry.Key) -and $entry.Key -cnotin @('settings.json','window.json')) { throw 'Unexpected new profile data was preserved; restoration cannot be certified.' }
  }
  foreach ($entry in $current.GetEnumerator()) {
    if (!$Snapshot.Hashes.ContainsKey($entry.Key)) { Remove-Item -LiteralPath $entry.Value.FullName }
  }
  foreach ($entry in $verified.GetEnumerator()) {
    Write-PickleReadingAtomic (Join-Path $Snapshot.Directory $entry.Key) $entry.Value
  }
  $restored=Get-PickleReadingFiles $Snapshot.Directory
  if ($restored.Count -ne $Snapshot.Hashes.Count) { throw 'The restored profile inventory differs.' }
  $remaining=64MB
  foreach ($entry in $restored.GetEnumerator()) {
    $bytes=Read-PickleReadingBytes $entry.Value.FullName $remaining;$remaining-=$bytes.Length
    if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)) -cne $Snapshot.Hashes[$entry.Key]) { throw 'Restored profile bytes differ.' }
  }
}

function Initialize-PickleReadingWindowsPaths {
  if (!$IsWindows) { throw 'Native reading acceptance runs only on Windows.' }
  if (!('PickleReadingKnownFolder' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
public static class PickleReadingKnownFolder {
  [DllImport("shell32.dll")] static extern int SHGetKnownFolderPath(ref Guid id,uint flags,IntPtr token,out IntPtr path);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetFinalPathNameByHandleW(IntPtr file,StringBuilder path,uint capacity,uint flags);
  public static string LocalData() {
    Guid id=new Guid("F1B32785-6FBA-4FCF-9D55-7B8E7F157091");IntPtr path;
    int result=SHGetKnownFolderPath(ref id,0,IntPtr.Zero,out path);
    if(result!=0) throw new InvalidOperationException("Could not locate the real per-user data directory.");
    try { return Marshal.PtrToStringUni(path); } finally { Marshal.FreeCoTaskMem(path); }
  }
  public static string FinalPath(string path) {
    using(var file=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.ReadWrite|FileShare.Delete)) {
      var buffer=new StringBuilder(512);
      uint count=GetFinalPathNameByHandleW(file.SafeFileHandle.DangerousGetHandle(),buffer,(uint)buffer.Capacity,0);
      if(count>=buffer.Capacity && count<32768) {
        buffer=new StringBuilder((int)count+1);
        count=GetFinalPathNameByHandleW(file.SafeFileHandle.DangerousGetHandle(),buffer,(uint)buffer.Capacity,0);
      }
      if(count==0 || count>=buffer.Capacity) throw new IOException("Could not resolve the owned executable file identity.");
      return buffer.ToString();
    }
  }
}
'@
  }
}
function Get-PickleReadingDataDirectory {
  Initialize-PickleReadingWindowsPaths
  return Join-Path ([PickleReadingKnownFolder]::LocalData()) 'Pickle\PickleRune\data'
}
function Get-PickleReadingCanonicalExecutable([string]$Executable) {
  if (!$IsWindows) { return [IO.Path]::GetFullPath($Executable) }
  Initialize-PickleReadingWindowsPaths
  return [PickleReadingKnownFolder]::FinalPath($Executable)
}

function Start-PickleReadingChild([string]$Executable,[string[]]$Arguments) {
  $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=$Executable;$start.UseShellExecute=$false
  foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
  return [Diagnostics.Process]::Start($start)
}
function Throw-PickleReadingFailure([string]$Code) {
  $error=[InvalidOperationException]::new('Controlled reading harness failed.')
  $error.Data['PickleReadingCode']=$Code
  throw $error
}
function Get-PickleReadingFailureCategory($Record) {
  $allowed=@('owned-child-exited','owned-child-executable-mismatch','owned-child-executable-unreadable','ready-file-invalid','ready-pid-mismatch','ready-timeout','ready-native-handle-invalid')
  $error=$Record.Exception
  for ($depth=0;$error -and $depth -lt 8;$depth++) {
    if ($error.Data.Contains('PickleReadingCode') -and $error.Data['PickleReadingCode'] -cin $allowed) { return [string]$error.Data['PickleReadingCode'] }
    $error=$error.InnerException
  }
  return 'harness-operation-failed'
}
function ConvertTo-PickleReadingFixtureFailure($Value,[int]$ExpectedPid) {
  $required=@('schema','test','passed','pid','stage','failure','error_kind','error_hresult')
  $allowed=$required+@('native_error_code','inner_error_kind','inner_error_hresult')
  $keys=@($Value.PSObject.Properties.Name)
  if (@($keys | Where-Object { $_ -cnotin $allowed }).Count -or @($required | Where-Object { $_ -cnotin $keys }).Count -or $Value.schema -ne 1 -or $Value.test -cne 'windows-owned-selection-source' -or $Value.passed -ne $false -or $Value.pid -ne $ExpectedPid) { throw 'Unsafe owned fixture diagnostic.' }
  $stages=@('contract-read','fixture-sta-thread','fixture-exception-mode','fixture-visual-styles','fixture-controls','fixture-content','fixture-shown','fixture-selection','fixture-ready-write','fixture-message-loop')
  $codes=@('invalid-json-size','invalid-json-object','invalid-contract-field','invalid-contract-fields','invalid-contract-text','fixture-selection-unavailable','evidence-too-large','unexpected-fixture-error')
  $kinds=@('type-initialization','native-library-unavailable','win32','com','access-denied','security','io','argument','invalid-operation','other')
  if ($Value.stage -cnotin $stages -or $Value.failure -cnotin $codes -or $Value.error_kind -cnotin $kinds) { throw 'Unrecognized owned fixture diagnostic category.' }
  $clean=[ordered]@{schema=1;test='windows-owned-selection-source';passed=$false;pid=$ExpectedPid;stage=$Value.stage;failure=$Value.failure;error_kind=$Value.error_kind}
  foreach ($name in @('error_hresult','native_error_code','inner_error_hresult')) {
    if ($name -cin $keys) {
      $number=$Value.$name
      if (!(($number -is [int]) -or ($number -is [long])) -or $number -lt [int]::MinValue -or $number -gt [int]::MaxValue) { throw 'Invalid owned fixture numeric diagnostic.' }
      $clean[$name]=[int]$number
    }
  }
  if ('inner_error_kind' -cin $keys) {
    if ($Value.inner_error_kind -cnotin $kinds) { throw 'Unrecognized owned fixture inner error category.' }
    $clean.inner_error_kind=$Value.inner_error_kind
  }
  return $clean
}
function Add-PickleReadingCleanupFailure($Evidence,[ref]$Primary,[string]$Code) {
  $Evidence.cleanup_categories=@($Evidence.cleanup_categories)+$Code
  if (!$Primary.Value) { $Primary.Value='Owned reading cleanup failed.' }
}
function Save-PickleReadingProcessState($Evidence,[string]$Name,[Diagnostics.Process]$Process,[string]$Executable,[switch]$BeforeCleanup) {
  if ($null -eq $Process) { return }
  $Process.Refresh()
  if (!$Evidence.owned_processes.Contains($Name)) {
    $Evidence.owned_processes[$Name]=[ordered]@{pid=$Process.Id;expected_executable=$Executable;exited_before_cleanup=$null;exit_code=$null;forced_cleanup=$false}
  }
  $record=$Evidence.owned_processes[$Name]
  if ($BeforeCleanup) { $record.exited_before_cleanup=$Process.HasExited }
  if ($Process.HasExited) { $record.exit_code=$Process.ExitCode }
  elseif ($BeforeCleanup) {
    try {
      $reported=$Process.MainModule.FileName
      $record.reported_executable=$reported
      $record.raw_path_match=[string]::Equals([IO.Path]::GetFullPath($reported),[IO.Path]::GetFullPath($Executable),[StringComparison]::OrdinalIgnoreCase)
      $record.expected_canonical=Get-PickleReadingCanonicalExecutable $Executable
      $record.reported_canonical=Get-PickleReadingCanonicalExecutable $reported
      $record.canonical_path_match=[string]::Equals($record.expected_canonical,$record.reported_canonical,[StringComparison]::OrdinalIgnoreCase)
    } catch { $record.path_inspection_category='owned-child-executable-unreadable' }
  }
}
function Assert-PickleReadingChild([Diagnostics.Process]$Process,[string]$Executable) {
  $Process.Refresh()
  if ($Process.HasExited) { Throw-PickleReadingFailure 'owned-child-exited' }
  try {
    $actual=Get-PickleReadingCanonicalExecutable $Process.MainModule.FileName
    $expected=Get-PickleReadingCanonicalExecutable $Executable
  }
  catch { Throw-PickleReadingFailure 'owned-child-executable-unreadable' }
  if (![string]::Equals($actual,$expected,[StringComparison]::OrdinalIgnoreCase)) { Throw-PickleReadingFailure 'owned-child-executable-mismatch' }
}
function Read-PickleReadingJson([string]$Path) {
  $bytes=Read-PickleReadingBytes $Path 65536
  return ([Text.Encoding]::UTF8.GetString($bytes).TrimStart([char]0xfeff) | ConvertFrom-Json)
}
function Wait-PickleReadingReady([Diagnostics.Process]$Process,[string]$Executable,[string]$Ready) {
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  while ([DateTime]::UtcNow -lt $deadline) {
    Assert-PickleReadingChild $Process $Executable
    if (Test-Path -LiteralPath $Ready -PathType Leaf) {
      try { $value=Read-PickleReadingJson $Ready }
      catch { Throw-PickleReadingFailure 'ready-file-invalid' }
      if ($value.pid -ne $Process.Id) { Throw-PickleReadingFailure 'ready-pid-mismatch' }
      return $value
    }
    Start-Sleep -Milliseconds 100
  }
  Throw-PickleReadingFailure 'ready-timeout'
}
function Invoke-PickleReadingLease([string]$Application) {
  $probe=Start-PickleReadingChild $Application @('--check-running')
  try {
    if (!$probe.WaitForExit(10000)) { throw 'Read-only instance inspection exceeded ten seconds.' }
    return $probe.ExitCode
  } finally {
    if (!$probe.HasExited) { $probe.Kill();$probe.WaitForExit(10000) | Out-Null }
    $probe.Dispose()
  }
}

function Build-PickleReadingFixture([string]$Directory) {
  if (!$IsWindows) { throw 'The native fixture is compiled only on Windows.' }
  $framework=Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::Windows)) 'Microsoft.NET\Framework64\v4.0.30319'
  $compiler=Join-Path $framework 'csc.exe';$executable=Join-Path $Directory 'ReadingFixture.exe'
  $source=Join-Path $PSScriptRoot '../native/windows/ReadingFixture.cs'
  $arguments=@('/nologo','/target:exe','/platform:x64','/optimize+','/langversion:5',('/out:'+$executable))
  foreach ($reference in @('System.dll','System.Core.dll','System.Windows.Forms.dll','System.Drawing.dll','System.Web.Extensions.dll','WPF/UIAutomationClient.dll','WPF/UIAutomationTypes.dll','WPF/WindowsBase.dll')) {
    $path=Join-Path $framework $reference
    if (!(Test-Path -LiteralPath $path -PathType Leaf)) { throw 'The runner lacks required built-in Framework references.' }
    $arguments+=('/reference:'+$path)
  }
  $arguments+=[IO.Path]::GetFullPath($source)
  $compile=Start-PickleReadingChild $compiler $arguments
  try { if (!$compile.WaitForExit(30000) -or $compile.ExitCode -ne 0) { throw 'Owned reading fixture compilation failed or timed out.' } }
  finally { if (!$compile.HasExited) { $compile.Kill();$compile.WaitForExit(10000) | Out-Null };$compile.Dispose() }
  return $executable
}

function Assert-PickleInstalledReadingWorkflow([string]$Application,[string]$DataDirectory,[string]$EvidencePath,[string]$Configuration) {
  if (!$IsWindows) { throw 'Native installed reading acceptance runs only on Windows; no app was launched.' }
  $actualData=Get-PickleReadingDataDirectory
  if (![string]::Equals([IO.Path]::GetFullPath($DataDirectory),[IO.Path]::GetFullPath($actualData),[StringComparison]::OrdinalIgnoreCase)) { throw 'Reading acceptance requires the app actual KnownFolder data directory, not an environment override.' }
  if ((Invoke-PickleReadingLease $Application) -ne 0) { throw 'Refusing to replace a live app profile.' }
  $work=Join-Path ([IO.Path]::GetTempPath()) ('pickle-reading-'+[guid]::NewGuid().ToString('N'))
  Assert-PickleReadingDirectory $work | Out-Null
  New-Item -ItemType Directory -Path $work | Out-Null
  $evidence=[ordered]@{schema=1;test='windows-installed-reading-workflow';passed=$false;configuration=$Configuration;driver=$null;server=$null;fixture=$null;application_pid=$null;ordinary_exit=$false;lease_released=$false;control_removed=$false;profile_restored=$false;owned_children_stopped=$false;owned_processes=[ordered]@{};cleanup_categories=@()}
  $applicationProcess=$null;$fixture=$null;$server=$null;$driver=$null;$snapshot=$null;$failure=$null;$stage='prepare'
  try {
    $snapshot=Save-PickleReadingProfile $DataDirectory (Join-Path $work 'profile-backup')
    $evidence.original_profile_files=$snapshot.Hashes.Count
    $contractPath=Join-Path $work 'contract.json';$serverState=Join-Path $work 'server-state.json';$serverReady=Join-Path $work 'server-ready.json';$fixtureReady=Join-Path $work 'fixture-ready.json';$serverStop=Join-Path $work 'server-stop';$driverEvidence=Join-Path $work 'driver-evidence.json'
    $contract=[ordered]@{before='BEFORE owned fixture marker is outside the selected paragraph.';selected='Café shuttle service may reduce waiting on this route in dry weather. Evidence from one route does not establish reliability in rain.';after='AFTER owned fixture marker is outside the selected paragraph.';answer='The Café shuttle may reduce waiting on this route in dry weather. This does not establish reliability in rain.'}
    [IO.File]::WriteAllText($contractPath,($contract | ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    $stage='compile-owned-fixture'
    $fixtureExe=Build-PickleReadingFixture $work
    $stage='start-owned-provider'
    $python=(Get-Command python -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $provider=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'windows-reading-provider.py'))
    $server=Start-PickleReadingChild $python @($provider,'--contract',$contractPath,'--ready',$serverReady,'--state',$serverState,'--stop-file',$serverStop)
    $serverInfo=Wait-PickleReadingReady $server $python $serverReady
    if ($serverInfo.port -lt 1 -or $serverInfo.port -gt 65535) { throw 'Owned provider returned an invalid ephemeral port.' }
    $address='http://127.0.0.1:'+$serverInfo.port
    $settings=[ordered]@{version=1;data=[ordered]@{mode='Local';local_address=$address;local_model='pickle-reading-fixture:local';same_device=$true;strict_offline=$true;modifier_gesture=$false;shortcut='ctrl+alt+p';automatic_menu=$false;history_enabled=$false;screen_context_enabled=$false;web_context_enabled=$false;jev_enabled=$false;cloud_consent=$false;jev_consent=$false;local_consent='';context_consent_destination='';streaming=$true;transcriber_executable='';transcriber_model='';local_vision_model='';exclusions=@()}}
    Write-PickleReadingAtomic (Join-Path $DataDirectory 'settings.json') ([Text.UTF8Encoding]::new($false).GetBytes(($settings | ConvertTo-Json -Depth 5)))
    $stage='start-owned-source'
    $fixture=Start-PickleReadingChild $fixtureExe @('fixture','--contract',$contractPath,'--ready',$fixtureReady)
    $fixtureInfo=Wait-PickleReadingReady $fixture $fixtureExe $fixtureReady
    foreach ($handle in @($fixtureInfo.hwnd,$fixtureInfo.edit_hwnd)) { if ($handle -notmatch '^0x[0-9a-fA-F]{1,16}$' -or [Convert]::ToUInt64($handle.Substring(2),16) -eq 0) { Throw-PickleReadingFailure 'ready-native-handle-invalid' } }
    $evidence.fixture=[ordered]@{pid=$fixture.Id;hwnd=$fixtureInfo.hwnd;edit_hwnd=$fixtureInfo.edit_hwnd}
    $stage='drive-installed-reader'
    $applicationProcess=Start-PickleReadingChild $Application @()
    $evidence.application_pid=$applicationProcess.Id
    Assert-PickleReadingChild $applicationProcess $Application
    $driver=Start-PickleReadingChild $fixtureExe @('drive','--contract',$contractPath,'--application-pid',([string]$applicationProcess.Id),'--fixture-pid',([string]$fixture.Id),'--fixture-hwnd',([string]$fixtureInfo.hwnd),'--server-state',$serverState,'--evidence',$driverEvidence)
    if (!$driver.WaitForExit(90000) -or $driver.ExitCode -ne 0) { throw 'Owned reading driver failed or exceeded 90 seconds.' }
    $evidence.driver=Read-PickleReadingJson $driverEvidence
    if ($evidence.driver.passed -ne $true) { throw 'Owned reading driver did not attest its checks.' }
    foreach ($attestation in @('initial_compose_ready','fixture_selection_verified','chooser_source_verified','consent_before_network_verified','provider_source_verified','completed_answer_verified','ordinary_quit_verified')) {
      if ($evidence.driver.$attestation -ne $true) { throw 'Owned reading driver omitted a required attestation.' }
    }
    if ($evidence.driver.application_pid -ne $applicationProcess.Id -or $evidence.driver.fixture_pid -ne $fixture.Id -or $evidence.driver.app_exit_code -ne 0 -or $evidence.driver.shortcut_events_inserted -ne 6) { throw 'Owned driver process identity, shortcut delivery or ordinary exit evidence differed.' }
    if (!$applicationProcess.WaitForExit(10000) -or $applicationProcess.ExitCode -ne 0) { throw 'The installed app did not exit normally after the driver quit.' }
    $evidence.ordinary_exit=$true
    if ((Invoke-PickleReadingLease $Application) -ne 0) { throw 'The installed reading process left its lease active.' }
    $evidence.lease_released=$true
    if ((Test-Path -LiteralPath (Join-Path $DataDirectory 'instance.json')) -or (Test-Path -LiteralPath (Join-Path $DataDirectory 'BrowserBridge/connection.json'))) { throw 'The normally quit app retained a private control descriptor.' }
    $evidence.control_removed=$true
    $evidence.server=Read-PickleReadingJson $serverState
    if ($evidence.server.total_requests -ne 2 -or $evidence.server.show_count -ne 1 -or $evidence.server.chat_count -ne 1 -or $evidence.server.source_verified -ne $true -or $evidence.server.rejected -ne $false) { throw 'Owned provider did not attest the exact source and request contract.' }
  } catch {
    $failure='Installed reading acceptance failed at '+$stage+'.'
    $evidence.failure_stage=$stage
    $evidence.failure_category=Get-PickleReadingFailureCategory $_
  } finally {
    try {
      Save-PickleReadingProcessState $evidence 'fixture' $fixture $fixtureExe -BeforeCleanup
      Save-PickleReadingProcessState $evidence 'server' $server $python -BeforeCleanup
      Save-PickleReadingProcessState $evidence 'driver' $driver $fixtureExe -BeforeCleanup
      Save-PickleReadingProcessState $evidence 'application' $applicationProcess $Application -BeforeCleanup
      # Keep every process object until cleanup; never kill by name or a PID
      # reopened from an external source. A successful path uses normal quits.
      if ($null -ne $driver -and !$driver.HasExited) { Add-PickleReadingCleanupFailure $evidence ([ref]$failure) 'driver-forced-stop';$evidence.owned_processes.driver.forced_cleanup=$true;$driver.Kill();if (!$driver.WaitForExit(10000)) { throw 'Owned driver remained alive.' } }
      if ($null -ne $applicationProcess -and !$applicationProcess.HasExited) { Add-PickleReadingCleanupFailure $evidence ([ref]$failure) 'application-forced-stop';$evidence.owned_processes.application.forced_cleanup=$true;$applicationProcess.Kill();if (!$applicationProcess.WaitForExit(10000)) { throw 'Owned app remained alive.' } }
      if ($null -ne $fixture -and !$fixture.HasExited) {
        if (!$fixture.CloseMainWindow() -or !$fixture.WaitForExit(5000)) { Add-PickleReadingCleanupFailure $evidence ([ref]$failure) 'fixture-forced-stop';$evidence.owned_processes.fixture.forced_cleanup=$true;$fixture.Kill();if (!$fixture.WaitForExit(10000)) { throw 'Owned source fixture remained alive.' } }
      }
      if ($null -ne $server -and !$server.HasExited) {
        [IO.File]::WriteAllText((Join-Path $work 'server-stop'),'stop owned fixture')
        if (!$server.WaitForExit(5000)) { Add-PickleReadingCleanupFailure $evidence ([ref]$failure) 'server-forced-stop';$evidence.owned_processes.server.forced_cleanup=$true;$server.Kill();if (!$server.WaitForExit(10000)) { throw 'Owned server remained alive.' } }
      }
      if ($null -ne $fixture -and $fixture.ExitCode -ne 0) { Add-PickleReadingCleanupFailure $evidence ([ref]$failure) 'fixture-exit-nonzero' }
      if ($null -ne $server -and $server.ExitCode -ne 0) { Add-PickleReadingCleanupFailure $evidence ([ref]$failure) 'server-exit-nonzero' }
      $evidence.owned_children_stopped=$true
      if ($snapshot) {
        $stage='restore-profile'
        Restore-PickleReadingProfile $snapshot { (($null -eq $applicationProcess -or $applicationProcess.HasExited) -and (Invoke-PickleReadingLease $Application) -eq 0) }
        $evidence.profile_restored=$true
      }
      if ($failure) { $evidence.failure=$failure }
      else { $evidence.passed=$evidence.ordinary_exit -and $evidence.lease_released -and $evidence.control_removed -and $evidence.profile_restored -and $evidence.owned_children_stopped }
    } catch {
      $evidence.passed=$false;$evidence.cleanup_failure='Owned processes or profile could not be restored safely; private backup retained.'
      if (!$failure) { $failure='Installed reading cleanup or profile restoration failed.' }
    } finally {
      # Failure evidence is deliberately limited to fixed fixture/driver metadata.
      if (!$evidence.driver -and (Test-Path -LiteralPath (Join-Path $work 'driver-evidence.json') -PathType Leaf)) { try { $evidence.driver=Read-PickleReadingJson (Join-Path $work 'driver-evidence.json') } catch {} }
      if (!$evidence.server -and (Test-Path -LiteralPath (Join-Path $work 'server-state.json') -PathType Leaf)) { try { $evidence.server=Read-PickleReadingJson (Join-Path $work 'server-state.json') } catch {} }
      Save-PickleReadingProcessState $evidence 'fixture' $fixture $fixtureExe
      Save-PickleReadingProcessState $evidence 'server' $server $python
      Save-PickleReadingProcessState $evidence 'driver' $driver $fixtureExe
      Save-PickleReadingProcessState $evidence 'application' $applicationProcess $Application
      $sourceFailure=Join-Path $work 'fixture-ready.json.failure.json'
      if ($null -ne $fixture -and (Test-Path -LiteralPath $sourceFailure -PathType Leaf)) {
        try {
          $evidence.fixture_diagnostic=ConvertTo-PickleReadingFixtureFailure (Read-PickleReadingJson $sourceFailure) $fixture.Id
          $evidence.fixture_diagnostic_status='validated'
        } catch { $evidence.fixture_diagnostic_status='invalid-or-unreadable' }
        $evidence.passed=$false
        if (!$evidence.failure) { $evidence.failure='The owned source fixture reported a controlled failure.' }
      }
      if (!$evidence.profile_restored) { $evidence.recovery_directory=$work }
      if ($failure -and !$evidence.failure) { $evidence.failure=$failure }
      $evidence | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $EvidencePath -Encoding utf8
      foreach ($child in @($driver,$applicationProcess,$fixture,$server)) { if ($null -ne $child) { $child.Dispose() } }
      if ($evidence.profile_restored -and $evidence.owned_children_stopped) { Remove-Item -LiteralPath $work -Recurse -Force }
    }
  }
  if (!$evidence.passed) { throw 'Installed synthetic reading workflow did not pass; inspect its fixed evidence.' }
}
