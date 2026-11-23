# The dedicated Core process owns this job for its entire remaining lifetime.
# Never explicitly close its assigned handle: that would terminate the owner too.
function Initialize-PickleReadingDelegateJobType {
  if (!('PickleReadingDelegateJob' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PickleReadingDelegateJob {
  [StructLayout(LayoutKind.Sequential)] struct BasicLimits {
    public long ProcessTime, JobTime;
    public uint Flags;
    public UIntPtr MinimumWorkingSet, MaximumWorkingSet;
    public uint ActiveProcesses;
    public UIntPtr Affinity;
    public uint PriorityClass, SchedulingClass;
  }
  [StructLayout(LayoutKind.Sequential)] struct IoCounters {
    public ulong ReadOperations, WriteOperations, OtherOperations;
    public ulong ReadBytes, WriteBytes, OtherBytes;
  }
  [StructLayout(LayoutKind.Sequential)] struct ExtendedLimits {
    public BasicLimits Basic;
    public IoCounters Io;
    public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
  }
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObjectW(IntPtr attributes,string name);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int kind,ref ExtendedLimits limits,uint length);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  static IntPtr lifetimeJob=IntPtr.Zero;
  public static void AssignSelf() {
    if(lifetimeJob!=IntPtr.Zero) throw new InvalidOperationException("Reading delegate containment was already established.");
    // Null security attributes create a non-inheritable handle. The unnamed
    // job has no breakaway flag, so all subsequent helper children stay inside.
    IntPtr job=CreateJobObjectW(IntPtr.Zero,null);
    if(job==IntPtr.Zero) throw new InvalidOperationException("Reading delegate containment could not be created.");
    bool assigned=false;
    try {
      var limits=new ExtendedLimits();limits.Basic.Flags=0x00002000;
      if(!SetInformationJobObject(job,9,ref limits,(uint)Marshal.SizeOf(typeof(ExtendedLimits)))) throw new InvalidOperationException("Reading delegate containment limits could not be set.");
      if(!AssignProcessToJobObject(job,GetCurrentProcess())) throw new InvalidOperationException("Reading delegate containment could not be assigned.");
      lifetimeJob=job;assigned=true;
    } finally { if(!assigned) CloseHandle(job); }
  }
}
'@
  }
}

function Initialize-PickleReadingDelegateContainment {
  if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Reading delegate containment requires Windows.' }
  Initialize-PickleReadingDelegateJobType
  [PickleReadingDelegateJob]::AssignSelf()
}
