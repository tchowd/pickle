// Hosted Windows CI only. Never linked into Pickle; no CDP, DOM evaluation,
// native packet, registry mutation, app launch, or regular browser profile.
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.RegularExpressions;
#if !PICKLE_PREDICATES
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Automation;
#endif

public static class BrowserExtensionPredicates {
    public static bool ExtensionId(string value) { return value!=null && Regex.IsMatch(value,"^[a-p]{32}$"); }
    public static string IdFromLabel(string value) {
        if (value==null || value.Length>4096) return null;
        var match=Regex.Match(value,@"^(?:ID:\s*)?([a-p]{32})$");
        return match.Success ? match.Groups[1].Value : null;
    }
    public static bool Browser(string value) { return value=="chrome" || value=="edge"; }
    public static bool ToggleCandidate(string actualName,string expectedName,bool visible,bool enabled,bool supportsToggle) {
        return actualName==expectedName && visible && enabled && supportsToggle;
    }
    public static string CardId(string containerName,string expectedName,string expectedVersion,IEnumerable<string> labels) {
        if(containerName!=expectedName || labels==null) return null;
        var values=labels.ToArray();if(!values.Contains(expectedVersion)) return null;
        var ids=values.Select(IdFromLabel).Where(v=>v!=null).Distinct().ToArray();
        return ids.Length==1 ? ids[0] : null;
    }
    public static bool Scope(bool loaded,bool drained,bool fullDelivery) { return loaded && drained && !fullDelivery; }
    public static void SelfTest() {
        const string id="abcdefghijklmnopabcdefghijklmnop";
        if (!ExtensionId(id) || IdFromLabel("ID: "+id)!=id || IdFromLabel(id)!=id) throw new Exception("id-positive");
        foreach (string bad in new[]{"",id+"a",id.Substring(1),id.ToUpperInvariant(),"q"+id.Substring(1),"path "+id,"ID: "+id+" unrelated"})
            if (IdFromLabel(bad)!=null || ExtensionId(bad)) throw new Exception("id-negative");
        if (!Browser("chrome") || !Browser("edge") || Browser("chromium") || Browser("chrome-for-testing")) throw new Exception("browser-scope");
        if (!Scope(true,true,false) || Scope(true,false,false) || Scope(false,true,false) || Scope(true,true,true)) throw new Exception("incomplete-scope");
        if(!ToggleCandidate("Developer mode","Developer mode",true,true,true) ||
            ToggleCandidate("Other","Developer mode",true,true,true) ||
            ToggleCandidate("Developer mode","Developer mode",false,true,true) ||
            ToggleCandidate("Developer mode","Developer mode",true,false,true) ||
            ToggleCandidate("Developer mode","Developer mode",true,true,false)) throw new Exception("toggle-contract");
        if(CardId("Pickle","Pickle","0.1.0",new[]{"0.1.0","ID: "+id})!=id) throw new Exception("card-positive");
        // A second card's valid ID must never fill a missing target ID.
        if(CardId("Pickle","Pickle","0.1.0",new[]{"0.1.0"})!=null ||
            CardId("Other extension","Pickle","0.1.0",new[]{"0.1.0",id})!=null ||
            CardId("Pickle","Pickle","0.1.0",new[]{"0.1.1",id})!=null ||
            CardId("Pickle","Pickle","0.1.0",new[]{"0.1.0",id,new string('a',32)})!=null) throw new Exception("card-cross-attribution");
    }
}

#if !PICKLE_PREDICATES
sealed class ExtensionStageFailure : Exception { public readonly string Code; public ExtensionStageFailure(string code) { Code=code; } }
public static class BrowserExtensionFixtureProgram {
    static readonly JavaScriptSerializer Json=new JavaScriptSerializer { MaxJsonLength=32768,RecursionLimit=12 };
    static readonly Dictionary<string,object> Report=new Dictionary<string,object> {
        {"schema",1},{"test","windows-stable-bundled-extension-load"},{"scope","bundled-extension-load-only"},
        {"passed",false},{"full_delivery_verified",false},{"native_host_registered",false},
        {"native_host_executed",false},{"authenticated_bridge_verified",false},{"app_launched",false},
        {"devtools_or_cdp_used",false},{"extension_code_modified",false},{"browser_job_drained",false}};
    static string stage="arguments",evidence;
    static DateTime deadline;
    static BrowserJob browser;
    const int NodeLimit=1024;
    static void Fail(string code) { throw new ExtensionStageFailure(code); }
    static void Check() { if (DateTime.UtcNow>=deadline) Fail("stage-deadline"); }
    static void Wait(Func<bool> test,int seconds,string code) {
        DateTime end=DateTime.UtcNow.AddSeconds(seconds); if (end>deadline) end=deadline;
        do { try { if (test()) return; } catch (ElementNotAvailableException) { } Thread.Sleep(100); } while (DateTime.UtcNow<end);
        Fail(code);
    }
    [MTAThread] public static int Main(string[] args) {
        if (args.Length==1 && args[0]=="--self-test") { BrowserExtensionPredicates.SelfTest(); return 0; }
        bool loaded=false,drained=false;
        try {
            if (Environment.GetEnvironmentVariable("GITHUB_ACTIONS")!="true" || Environment.GetEnvironmentVariable("RUNNER_ENVIRONMENT")!="github-hosted") Fail("requires-hosted-windows-ci");
            string[] names={"--browser","--executable","--profile","--extension","--proxy-port","--evidence"};
            if (args.Length!=1+names.Length*2 || args[0]!="load") Fail("invalid-arguments");
            var options=new Dictionary<string,string>(StringComparer.Ordinal);
            for (int i=1;i<args.Length;i+=2) {
                if (!names.Contains(args[i]) || options.ContainsKey(args[i]) || String.IsNullOrWhiteSpace(args[i+1])) Fail("invalid-arguments");
                options.Add(args[i],args[i+1]);
            }
            evidence=options["--evidence"]; string kind=options["--browser"];
            if (!BrowserExtensionPredicates.Browser(kind)) Fail("unsupported-browser");
            int port; if (!Int32.TryParse(options["--proxy-port"],out port) || port<1 || port>65535) Fail("invalid-proxy-port");
            string exe=RegularPath(options["--executable"]), extension=DirectoryPath(options["--extension"]),profile=DirectoryPath(options["--profile"]);
            if (Directory.EnumerateFileSystemEntries(profile).Any()) Fail("profile-not-empty");
            if (!String.Equals(Path.GetFileName(exe),kind=="chrome" ? "chrome.exe" : "msedge.exe",StringComparison.OrdinalIgnoreCase)) Fail("wrong-browser-executable");
            var version=FileVersionInfo.GetVersionInfo(exe);
            if (version.ProductName!=(kind=="chrome" ? "Google Chrome" : "Microsoft Edge")) Fail("not-installed-stable-brand");
            var manifest=ReadObject(Path.Combine(extension,"manifest.json"),4096);
            object nameValue=null,versionValue=null;
            if (!manifest.TryGetValue("name",out nameValue) || !(nameValue is string) || !manifest.TryGetValue("version",out versionValue) || !(versionValue is string)) Fail("invalid-bundled-manifest");
            string name=(string)nameValue,extensionVersion=(string)versionValue;
            if (name!="Pickle — page and video context" || extensionVersion!="0.1.0") Fail("unexpected-bundled-extension");
            Report["browser"]=kind;
            Report["browser_version"]=String.Format(CultureInfo.InvariantCulture,"{0}.{1}.{2}.{3}",version.FileMajorPart,version.FileMinorPart,version.FileBuildPart,version.FilePrivatePart);
            deadline=DateTime.UtcNow.AddSeconds(90);
            stage="launch-contained-browser";
            browser=new BrowserJob(exe,profile,kind+"://extensions/",port);
            Report["atomic_job_membership_verified"]=true;
            IntPtr window=IntPtr.Zero;
            stage="owned-browser-window";
            Wait(()=> { window=browser.Window(false,IntPtr.Zero); return window!=IntPtr.Zero; },25,"owned-browser-window-unavailable");
            Report["owned_window_verified"]=true;
            stage="developer-mode";
            AutomationElement developer=null;
            Wait(()=> { developer=NamedToggle(window,"Developer mode"); return developer!=null; },20,"developer-toggle-unavailable");
            Report["developer_toggle_found"]=true;
            object pattern;
            if (!developer.TryGetCurrentPattern(TogglePattern.Pattern,out pattern)) Fail("developer-toggle-pattern-unavailable");
            var toggle=(TogglePattern)pattern;
            if (toggle.Current.ToggleState!=ToggleState.On) { Owned(developer); toggle.Toggle(); }
            Wait(()=> { developer=NamedToggle(window,"Developer mode"); if(developer==null) return false; object p; return developer.TryGetCurrentPattern(TogglePattern.Pattern,out p) && ((TogglePattern)p).Current.ToggleState==ToggleState.On; },5,"developer-mode-not-enabled");
            Report["developer_mode_enabled"]=true;
            stage="load-unpacked";
            AutomationElement load=null;
            Wait(()=> { load=Named(window,"Load unpacked",ControlType.Button); return load!=null && load.Current.IsEnabled && !load.Current.IsOffscreen; },10,"load-unpacked-unavailable");
            Invoke(load); Report["load_unpacked_invoked"]=true;
            IntPtr picker=IntPtr.Zero;
            stage="owned-folder-picker";
            Wait(()=> { picker=browser.Window(true,window); return picker!=IntPtr.Zero; },10,"owned-folder-picker-unavailable");
            Report["folder_picker_owned"]=true;
            stage="folder-picker-path";
            AutomationElement field=null;
            Wait(()=> {
                var candidates=Tree(picker).Where(n=>n.Current.ControlType==ControlType.Edit &&
                    new[]{"Folder:","Folder","File name:","File name"}.Contains(n.Current.Name) && n.Current.IsEnabled && !n.Current.IsOffscreen).ToArray();
                Report["folder_field_candidates"]=candidates.Length;
                if(candidates.Length>1) Fail("ambiguous-folder-field"); field=candidates.SingleOrDefault(); return field!=null;
            },5,"folder-field-unavailable");
            Owned(field);
            if (!field.TryGetCurrentPattern(ValuePattern.Pattern,out pattern) || ((ValuePattern)pattern).Current.IsReadOnly) Fail("folder-value-pattern-unavailable");
            ((ValuePattern)pattern).SetValue(extension);
            if (((ValuePattern)pattern).Current.Value!=extension) Fail("folder-path-value-mismatch");
            Report["folder_path_value_verified"]=true;
            var select=Named(picker,"Select Folder",ControlType.Button) ?? Named(picker,"Select folder",ControlType.Button);
            if (select==null) Fail("select-folder-unavailable");
            Invoke(select);
            Wait(()=>!Native.IsWindow(picker) || !Native.IsWindowVisible(picker),10,"folder-picker-did-not-close");
            Report["folder_picker_closed"]=true;
            stage="loaded-extension-card";
            string id=null;
            Wait(()=> {
                // Bind all facts to ONE owned accessible card. A manager-wide
                // scan could attribute another extension's ID to Pickle.
                var cards=Tree(window).Where(n=>n.Current.ControlType==ControlType.Group && n.Current.Name==name && !n.Current.IsOffscreen).ToArray();
                Report["target_card_candidates"]=cards.Length;
                if(cards.Length>1) Fail("ambiguous-target-card");
                Report["extension_name_found"]=cards.Length==1;
                if(cards.Length!=1) return false;
                var card=cards[0];var labels=Tree(card).Select(n=>n.Current.Name).ToArray();
                bool versioned=labels.Contains(extensionVersion);
                var ids=labels.Select(BrowserExtensionPredicates.IdFromLabel).Where(v=>v!=null).Distinct().ToArray();
                Report["extension_version_found"]=versioned;Report["extension_id_candidates"]=ids.Length;
                if(ids.Length>1) Fail("ambiguous-extension-id");
                id=BrowserExtensionPredicates.CardId(card.Current.Name,name,extensionVersion,labels);
                return id!=null;
            },20,"loaded-extension-id-unavailable");
            Report["extension_card_identity_verified"]=true;Report["extension_loaded"]=true;Report["extension_id"]=id;Report["extension_version"]=extensionVersion;
            loaded=true;stage="browser-cleanup";
        } catch (ExtensionStageFailure error) { Report["failure"]=error.Code; }
        catch (Exception error) { Report["failure"]="native-operation-failed";Report["error_kind"]=ErrorKind(error);Report["error_hresult"]=error.HResult; }
        finally {
            if (browser!=null) { try { drained=browser.Close(); } catch { drained=false; } browser.Dispose(); }
            Report["browser_job_drained"]=drained;
            Report["stage"]=stage;Report["passed"]=BrowserExtensionPredicates.Scope(loaded,drained,false);
            if (evidence!=null) { try { WriteEvidence(evidence,Report); } catch { loaded=false; } }
        }
        return loaded && drained ? 0 : 1;
    }
    static string ErrorKind(Exception error) {
        if(error is COMException) return "com";if(error is UnauthorizedAccessException) return "access-denied";
        if(error is IOException) return "io";if(error is ArgumentException) return "argument";return "other";
    }
    static string DirectoryPath(string path) {
        if (!Path.IsPathRooted(path) || !Directory.Exists(path)) { Fail("invalid-owned-directory"); }
        for (var part=new DirectoryInfo(Path.GetFullPath(path));part!=null;part=part.Parent)
            if ((part.Attributes&FileAttributes.ReparsePoint)!=0) Fail("directory-reparse-point");
        return Path.GetFullPath(path);
    }
    static string RegularPath(string path) {
        DirectoryPath(Path.GetDirectoryName(Path.GetFullPath(path)));
        if (!File.Exists(path) || (File.GetAttributes(path)&FileAttributes.ReparsePoint)!=0) Fail("invalid-regular-file"); return Path.GetFullPath(path);
    }
    static Dictionary<string,object> ReadObject(string path,int limit) {
        RegularPath(path);
        using(var stream=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.Read)) {
            if(stream.Length<2 || stream.Length>limit) { Fail("invalid-json-size"); }
            using(var reader=new StreamReader(stream,new UTF8Encoding(false,true))) {
                var value=Json.DeserializeObject(reader.ReadToEnd()) as Dictionary<string,object>;
                if(value==null) Fail("invalid-json-object");return value;
            }
        }
    }
    static void WriteEvidence(string path,object report) {
        string bytes=Json.Serialize(report);if(Encoding.UTF8.GetByteCount(bytes)>16384) Fail("evidence-too-large");
        using(var output=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.None)) {
            byte[] value=new UTF8Encoding(false).GetBytes(bytes);output.Write(value,0,value.Length);output.Flush(true);
        }
    }
    static void Owned(AutomationElement element) {
        Check();int pid=element==null ? 0 : element.Current.ProcessId;
        if(element==null || !browser.Owns(pid)) { Report["refused_uia_pid"]=pid;Fail("uia-element-not-owned"); }
    }
    static List<AutomationElement> Tree(IntPtr window) {
        Check();if(!browser.OwnsWindow(window)) Fail("uia-window-not-owned");
        return Tree(AutomationElement.FromHandle(window));
    }
    static List<AutomationElement> Tree(AutomationElement root) {
        Owned(root);
        var nodes=new List<AutomationElement>();var queue=new Queue<AutomationElement>();queue.Enqueue(root);
        var walker=TreeWalker.RawViewWalker;
        while(queue.Count>0) {
            Check();if(nodes.Count+queue.Count>NodeLimit) Fail("uia-node-bound");
            var node=queue.Dequeue();Owned(node);nodes.Add(node);Report["last_owned_uia_nodes"]=nodes.Count;
            if(node.Current.Name.Length>4096) Fail("uia-label-bound");
            var child=walker.GetFirstChild(node);
            while(child!=null) { if(nodes.Count+queue.Count>=NodeLimit) Fail("uia-node-bound");Owned(child);queue.Enqueue(child);child=walker.GetNextSibling(child); }
        }
        Report["last_owned_uia_nodes"]=nodes.Count;return nodes;
    }
    static AutomationElement Named(IntPtr window,string name,ControlType type) {
        var matches=Tree(window).Where(n=>n.Current.Name==name && n.Current.ControlType==type).ToArray();
        if(matches.Length>1) Fail("ambiguous-owned-control");return matches.SingleOrDefault();
    }
    static AutomationElement NamedToggle(IntPtr window,string name) {
        var matches=Tree(window).Where(n=> {
            if(n.Current.Name!=name) return false;
            object pattern;bool supported=n.TryGetCurrentPattern(TogglePattern.Pattern,out pattern);
            return BrowserExtensionPredicates.ToggleCandidate(n.Current.Name,name,!n.Current.IsOffscreen,n.Current.IsEnabled,supported);
        }).ToArray();
        if(matches.Length>1) Fail("ambiguous-owned-control");return matches.SingleOrDefault();
    }
    static void Invoke(AutomationElement element) {
        Owned(element);if(!element.Current.IsEnabled || element.Current.IsOffscreen) Fail("control-not-actionable");
        object pattern;if(!element.TryGetCurrentPattern(InvokePattern.Pattern,out pattern)) Fail("invoke-pattern-unavailable");((InvokePattern)pattern).Invoke();
    }
    sealed class BrowserJob : IDisposable {
        IntPtr job,process;readonly string executable;bool closed;
        public BrowserJob(string exe,string profile,string url,int port) {
            executable=Path.GetFullPath(exe);job=Native.CreateJobObjectW(IntPtr.Zero,null);
            if(job==IntPtr.Zero) Fail("job-create-failed");
            try {
                var limits=new Native.ExtendedLimits();limits.Basic.Flags=0x2000;
                if(!Native.SetInformationJobObject(job,9,ref limits,(uint)Marshal.SizeOf(typeof(Native.ExtendedLimits)))) Fail("job-limits-failed");
                IntPtr size=IntPtr.Zero;Native.InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref size);
                if(size.ToInt64()<=0 || size.ToInt64()>65536) Fail("attribute-size-failed");
                IntPtr attributes=Marshal.AllocHGlobal(size),jobs=Marshal.AllocHGlobal(IntPtr.Size);bool initialized=false;
                try {
                    if(!Native.InitializeProcThreadAttributeList(attributes,1,0,ref size)) Fail("attribute-initialize-failed");initialized=true;
                    Marshal.WriteIntPtr(jobs,job);
                    if(!Native.UpdateProcThreadAttribute(attributes,0,new IntPtr(0x2000d),jobs,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero)) Fail("job-list-failed");
                    var startup=new Native.StartupEx();startup.Startup.Size=Marshal.SizeOf(typeof(Native.StartupEx));startup.Attributes=attributes;
                    string[] arguments={exe,"--user-data-dir="+profile,"--no-first-run","--no-default-browser-check","--disable-background-networking","--disable-component-update","--disable-sync","--proxy-server=http://127.0.0.1:"+port,"--proxy-bypass-list=<-loopback>","--new-window",url};
                    var command=new StringBuilder(String.Join(" ",arguments.Select(Quote)));
                    Native.ProcessInfo info;
                    if(!Native.CreateProcessW(exe,command,IntPtr.Zero,IntPtr.Zero,false,0x08080000,IntPtr.Zero,null,ref startup,out info)) Fail("browser-create-failed");
                    process=info.Process;Native.CloseHandle(info.Thread);
                    bool contained;if(!Native.IsProcessInJob(process,job,out contained) || !contained) { Native.TerminateProcess(process,1);Native.WaitForSingleObject(process,1000);Fail("browser-job-membership-failed"); }
                } finally { if(initialized) Native.DeleteProcThreadAttributeList(attributes);Marshal.FreeHGlobal(jobs);Marshal.FreeHGlobal(attributes); }
            } catch { Dispose();throw; }
        }
        static string Quote(string value) {
            if(value.IndexOf('\0')>=0 || value.Length>32700) Fail("invalid-browser-argument");
            var result=new StringBuilder("\"");int slashes=0;
            foreach(char c in value) { if(c=='\\') { slashes++;continue; }result.Append('\\',c=='"' ? slashes*2+1 : slashes);result.Append(c);slashes=0; }
            result.Append('\\',slashes*2);result.Append('"');return result.ToString();
        }
        public bool Owns(int pid) {
            if(pid<=0 || job==IntPtr.Zero) return false;
            IntPtr candidate=Native.OpenProcess(0x1000,false,(uint)pid);if(candidate==IntPtr.Zero) return false;
            try { bool inside;if(!Native.IsProcessInJob(candidate,job,out inside) || !inside) return false;
                var path=new StringBuilder(32768);int length=path.Capacity;
                return Native.QueryFullProcessImageNameW(candidate,0,path,ref length) && String.Equals(Path.GetFullPath(path.ToString()),executable,StringComparison.OrdinalIgnoreCase);
            } finally { Native.CloseHandle(candidate); }
        }
        public bool OwnsWindow(IntPtr window) { uint pid;return window!=IntPtr.Zero && Native.IsWindow(window) && Native.GetWindowThreadProcessId(window,out pid)!=0 && Owns((int)pid); }
        public IntPtr Window(bool picker,IntPtr owner) {
            var found=new List<IntPtr>();Native.EnumWindows(delegate(IntPtr window,IntPtr ignored) {
                if(!Native.IsWindowVisible(window) || !OwnsWindow(window)) return true;
                var name=new StringBuilder(128);Native.GetClassNameW(window,name,name.Capacity);
                if(picker ? name.ToString()=="#32770" && Native.GetAncestor(window,3)==owner : name.ToString()=="Chrome_WidgetWin_1" && Native.GetAncestor(window,3)==window) found.Add(window);
                return true;
            },IntPtr.Zero);
            Report[picker ? "last_owned_picker_windows" : "last_owned_browser_windows"]=found.Count;
            if(found.Count>1) Fail("ambiguous-owned-window");return found.SingleOrDefault();
        }
        public bool Close() {
            if(closed) return true;if(job==IntPtr.Zero) return false;
            if(!Native.TerminateJobObject(job,1)) return false;
            DateTime end=DateTime.UtcNow.AddSeconds(8);
            do { var accounting=new Native.Accounting();if(!Native.QueryInformationJobObject(job,1,ref accounting,(uint)Marshal.SizeOf(typeof(Native.Accounting)),IntPtr.Zero)) return false;
                if(accounting.ActiveProcesses==0 && process!=IntPtr.Zero && Native.WaitForSingleObject(process,0)==0) { closed=true;return true; }Thread.Sleep(25);
            } while(DateTime.UtcNow<end);return false;
        }
        public void Dispose() { if(job!=IntPtr.Zero) { Native.TerminateJobObject(job,1);Native.CloseHandle(job);job=IntPtr.Zero; }if(process!=IntPtr.Zero) { Native.CloseHandle(process);process=IntPtr.Zero; } }
    }
    static class Native {
        [StructLayout(LayoutKind.Sequential)] public struct BasicLimits { public long ProcessTime,JobTime;public uint Flags;public UIntPtr Min,Max;public uint Active;public UIntPtr Affinity;public uint Priority,Scheduling; }
        [StructLayout(LayoutKind.Sequential)] public struct Io { public ulong ReadOps,WriteOps,OtherOps,ReadBytes,WriteBytes,OtherBytes; }
        [StructLayout(LayoutKind.Sequential)] public struct ExtendedLimits { public BasicLimits Basic;public Io Io;public UIntPtr ProcessMemory,JobMemory,PeakProcess,PeakJob; }
        [StructLayout(LayoutKind.Sequential)] public struct Accounting { public long User,Kernel,PeriodUser,PeriodKernel;public uint PageFaults,TotalProcesses,ActiveProcesses,Terminated; }
        [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct Startup { public int Size;public string Reserved,Desktop,Title;public uint X,Y,Width,Height,XChars,YChars,Fill,Flags;public ushort Show,ReservedBytes;public IntPtr Reserved2,Input,Output,Error; }
        [StructLayout(LayoutKind.Sequential)] public struct StartupEx { public Startup Startup;public IntPtr Attributes; }
        [StructLayout(LayoutKind.Sequential)] public struct ProcessInfo { public IntPtr Process,Thread;public uint Pid,Tid; }
        public delegate bool EnumWindow(IntPtr window,IntPtr data);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr CreateJobObjectW(IntPtr attributes,string name);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern bool SetInformationJobObject(IntPtr job,int kind,ref ExtendedLimits value,uint bytes);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern bool QueryInformationJobObject(IntPtr job,int kind,ref Accounting value,uint bytes,IntPtr returned);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern bool IsProcessInJob(IntPtr process,IntPtr job,out bool inside);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern bool TerminateJobObject(IntPtr job,uint code);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern bool TerminateProcess(IntPtr process,uint code);
        [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
        [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr size);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr bytes,IntPtr previous,IntPtr returned);
        [DllImport("kernel32.dll")] public static extern void DeleteProcThreadAttributeList(IntPtr list);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern bool CreateProcessW(string application,StringBuilder command,IntPtr processAttributes,IntPtr threadAttributes,bool inherit,uint flags,IntPtr environment,string directory,ref StartupEx startup,out ProcessInfo info);
        [DllImport("kernel32.dll",SetLastError=true)] public static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern bool QueryFullProcessImageNameW(IntPtr process,uint flags,StringBuilder path,ref int length);
        [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindow callback,IntPtr data);
        [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
        [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
        [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
        [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr window,uint flags);
        [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr window,StringBuilder value,int length);
    }
}
#endif
