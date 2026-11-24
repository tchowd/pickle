// Closed hosted-only continuation of the real Stable loader. Never part of
// Pickle. No native-message/bridge-token injection, provider action or CDP.
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.RegularExpressions;
#if !PICKLE_PREDICATES
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Windows.Automation;
#endif

public static class BrowserExtensionDeliveryPredicates {
    public const string ArticleUrl="http://native-extension.test/owned-extension-article";
    public const string ArticleTitle="Owned extension article";
    public const string ArticleText="Café shuttle service may reduce waiting on this route in dry weather. Evidence from one route does not establish reliability in rain.";
    public static string Normalize(string value) { return Regex.Replace(value??"",@"\s+"," ").Trim(); }
    public static bool EligibleReader(bool exactOwner,bool visible,bool geometryKnown,bool cloakKnown,int cloaked,long width,long height) {
        return exactOwner && visible && geometryKnown && cloakKnown && cloaked==0 && width>=480 && height>=180;
    }
    public static bool BrowserNeverStarted(bool attempted,bool hasJob,bool drained) { return !attempted && !hasJob && !drained; }
    public static bool UniqueEligibleReader(int count) { return count==1; }
    public static bool Article(IEnumerable<string> labels) {
        var values=labels.Select(Normalize).ToArray();
        return values.Contains(ArticleTitle+" · article") && values.Contains(ArticleText);
    }
    public static bool HostShape(IDictionary<string,object> manifest,string origin) {
        object name,type,path,origins;
        if(manifest.Count!=5 || !manifest.ContainsKey("description") || !manifest.TryGetValue("name",out name) || !manifest.TryGetValue("type",out type) ||
            !manifest.TryGetValue("path",out path) || !manifest.TryGetValue("allowed_origins",out origins)) return false;
        var allowed=origins as object[];
        return name as string=="com.pickle.reader" && manifest["description"] as string=="Pickle page context" && type as string=="stdio" && path is string && allowed!=null && allowed.Length==1 && allowed[0] as string==origin;
    }
    public static void SelfTest() {
        if(!EligibleReader(true,true,true,true,0,480,180) || EligibleReader(false,true,true,true,0,480,180) ||
            EligibleReader(true,false,true,true,0,480,180) || EligibleReader(true,true,false,true,0,480,180) ||
            EligibleReader(true,true,true,false,0,480,180) || EligibleReader(true,true,true,true,1,480,180) ||
            EligibleReader(true,true,true,true,0,479,180) || EligibleReader(true,true,true,true,0,480,179)) throw new Exception("delivery-reader-eligibility");
        if(!UniqueEligibleReader(1) || UniqueEligibleReader(0) || UniqueEligibleReader(2) || UniqueEligibleReader(256)) throw new Exception("delivery-reader-uniqueness");
        if(!BrowserNeverStarted(false,false,false) || BrowserNeverStarted(true,false,false) ||
            BrowserNeverStarted(false,true,false) || BrowserNeverStarted(false,false,true) || BrowserNeverStarted(true,true,true)) throw new Exception("delivery-browser-not-started");
        if(!Article(new[]{ArticleTitle+" · article",ArticleText}) || Article(new[]{ArticleTitle+" · article"}) || Article(new[]{ArticleText}) ||
            Article(new[]{ArticleTitle+" · video transcript",ArticleText}) || Article(new[]{ArticleTitle+" · article",ArticleText+" unexpected"})) throw new Exception("delivery-exact-context");
        const string origin="chrome-extension://abcdefghijklmnopabcdefghijklmnop/";
        var manifest=new Dictionary<string,object> {{"name","com.pickle.reader"},{"description","Pickle page context"},{"path","owned-host"},{"type","stdio"},{"allowed_origins",new object[]{origin}}};
        if(!HostShape(manifest,origin)) throw new Exception("delivery-real-manifest-shape");
        manifest["allowed_origins"]=new object[]{origin,"chrome-extension://other/"};if(HostShape(manifest,origin)) throw new Exception("delivery-extra-origin");
        manifest["allowed_origins"]=new object[]{"chrome-extension://other/"};if(HostShape(manifest,origin)) throw new Exception("delivery-wrong-origin");
        manifest["allowed_origins"]=new object[]{origin};manifest["unexpected"]="not retained";if(HostShape(manifest,origin)) throw new Exception("delivery-manifest-extra-field");
    }
}

#if !PICKLE_PREDICATES
public static partial class BrowserExtensionFixtureProgram {
    static DeliveryApplication deliveryApp;
    static string deliverySupport,deliveryKind,deliveryId;
    static bool deliveryConnected,deliveryDisconnected,deliveryOrdinaryExit;
    static DateTime deliveryCleanupDeadline;
    static bool DeliveryStopped { get { return deliveryOrdinaryExit && deliveryDisconnected; } }
    static void DeliveryCheck() { if(deliveryCleanupDeadline==default(DateTime)) Check();else if(DateTime.UtcNow>=deliveryCleanupDeadline) Fail("delivery-cleanup-deadline"); }
    static void DeliveryWait(Func<bool> test,int seconds,string failure) {
        var end=DateTime.UtcNow.AddSeconds(seconds);var limit=deliveryCleanupDeadline==default(DateTime) ? deadline : deliveryCleanupDeadline;if(end>limit) end=limit;
        do { DeliveryCheck();try { if(test()) return; } catch(ElementNotAvailableException) { }Thread.Sleep(100); } while(DateTime.UtcNow<end);
        Fail(failure);
    }
    static void InitializeDeliveryReport() {
        Report["test"]="windows-stable-extension-reference-delivery";Report["scope"]="extension-native-reference-delivery";
        Report.Remove("native_host_executed");Report["native_host_process_start_observed"]=false;Report["native_host_route_verified"]=false;
        Report["app_launched"]=false;Report["inference_action_invoked"]=false;Report["bridge_token_read"]=false;
        Report["native_window_pixels_verified"]=false;Report["selected_text_capture_verified"]=false;Report["visible_url_verified"]=false;
        Report["extension_loaded"]=false;Report["extension_card_identity_verified"]=false;
        Report["extension_details_identity_verified"]=false;Report["owned_extension_folder_verified"]=false;
        Report["extension_name_found"]=false;Report["extension_version_found"]=false;
        Report["browser_launch_attempted"]=false;Report["browser_not_started_cleanup_verified"]=false;
    }
    static void PrepareDelivery(Dictionary<string,string> options) {
        deliverySupport=DirectoryPath(options["--support"]);
        string actual=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),@"Pickle\PickleRune\data");
        if(!String.Equals(Canonical(deliverySupport),Canonical(actual),StringComparison.OrdinalIgnoreCase)) Fail("delivery-wrong-profile");
        int pid=0;long birth=0;
        if(!Int32.TryParse(options["--application-pid"],out pid) || pid<=0 || !Int64.TryParse(options["--application-start-ticks"],out birth) || birth<=0) Fail("delivery-invalid-app-owner");
        deliveryApp=new DeliveryApplication(pid,birth,RegularPath(options["--application"]));
        Report["app_launched"]=true;
        Report["application_pid"]=pid;
        DeliveryWait(()=>deliveryApp.Window()!=IntPtr.Zero && deliveryApp.Find("passage-input")!=null,15,"delivery-app-not-ready");
        if(deliveryApp.Find("source-preview")!=null || deliveryApp.Find("session-context")!=null) Fail("delivery-app-not-empty");
        DeliveryWait(()=> {
            string descriptor=Path.Combine(deliverySupport,@"BrowserBridge\connection.json");
            if(!File.Exists(descriptor)) return false;RegularPath(descriptor);
            return File.GetLastWriteTimeUtc(descriptor)>=deliveryApp.StartUtc;
        },5,"delivery-bridge-not-ready");
        // Presence only. The generated secret is never opened/read/copied here.
        Report["initial_compose_ready"]=true;
    }
    static void ContinueDelivery(IntPtr window,string kind,string id) {
        deliveryKind=kind;deliveryId=id;
        stage="delivery-connect";
        deliveryApp.Invoke("open-settings");deliveryApp.SetField("extension-id",id);
        deliveryApp.Invoke(kind=="edge" ? "connect-edge" : "connect-chrome");
        DeliveryWait(()=>deliveryApp.HasText("Browser host connected. Enable browser references and use the extension's Use this page action."),10,"delivery-connect-unavailable");
        VerifyRegistration(kind,id);deliveryConnected=true;Report["native_host_registered"]=true;Report["owned_registration_verified"]=true;
        stage="delivery-article";
        NavigateArticle(window);
        stage="delivery-shortcut";Native.BrowserSendPage(window);Report["delivery_shortcut_events"]=6;
        stage="delivery-context";
        DeliveryWait(()=>deliveryApp.Find("session-context")!=null,15,"delivery-context-unavailable");
        // Ordinary disclosure action only: reveal the whole retained reference
        // and scroll its section into view, without requesting any AI work.
        deliveryApp.Invoke("preview-full-context");Report["context_preview_revealed"]=true;
        DeliveryWait(()=> {
            var context=deliveryApp.Find("session-context");
            return context!=null && !context.Current.IsOffscreen && BrowserExtensionDeliveryPredicates.Article(deliveryApp.Texts(context,true));
        },10,"delivery-context-unavailable");
        if(deliveryApp.Find("answer")!=null || deliveryApp.Find("answer-draft")!=null || deliveryApp.Find("consent-continue")!=null) Fail("delivery-unexpected-reading");
        Report["accepted_article_verified"]=true;Report["native_host_route_verified"]=true;Report["authenticated_bridge_verified"]=true;
        // Preserve accepted route facts even if later Disconnect/Quit fails.
        // This establishes origin-bound delivery, not browser details metadata.
        Report["extension_id"]=id;Report["extension_identity_proof"]="authenticated-delivery";Report["extension_loaded"]=true;
        // This transitive route proof does not invent an observation of the
        // short-lived native host's PID or its process-start event.
        stage="delivery-disconnect";DisconnectDelivery();
        stage="delivery-quit";QuitDelivery();
    }
    static void DisconnectDelivery() {
        if(!deliveryConnected) { deliveryDisconnected=true;return; }
        if(deliveryDisconnected) return;
        deliveryApp.Invoke("open-settings");deliveryApp.SetField("extension-id",deliveryId);
        deliveryApp.Invoke(deliveryKind=="edge" ? "disconnect-edge" : "disconnect-chrome");
        DeliveryWait(()=>deliveryApp.HasText("Pickle's browser host was disconnected. Existing saved answers are kept."),10,"delivery-disconnect-unavailable");
        string directory=Path.Combine(deliverySupport,"NativeHost",deliveryKind);
        foreach(string name in new[]{"registration.json","com.pickle.reader.json","pickle-browser-host.exe"}) if(File.Exists(Path.Combine(directory,name))) Fail("delivery-host-files-remain");
        if(Directory.Exists(directory) && Directory.EnumerateFileSystemEntries(directory).Any()) Fail("delivery-host-files-remain");
        foreach(int view in new[]{32,64}) if(RegistryManifest(deliveryKind,view)!=null) Fail("delivery-registry-remains");
        deliveryDisconnected=true;Report["ordinary_disconnect_verified"]=true;
    }
    static void QuitDelivery() {
        if(deliveryApp==null || deliveryApp.Exited) return;
        deliveryApp.Focus();Native.DeliveryKey(deliveryApp.Handle,deliveryApp.OwnsWindow,0x51,true,false);
        DeliveryWait(()=>deliveryApp.Exited,10,"delivery-app-did-not-quit");
        if(deliveryApp.ExitCode!=0) Fail("delivery-app-quit-nonzero");deliveryOrdinaryExit=true;Report["ordinary_quit_verified"]=true;
    }
    static void FinishDelivery() {
        // A failed scenario gets only bounded Disconnect/Quit recovery, never
        // another extraction/inference attempt or a reset action deadline.
        deliveryCleanupDeadline=DateTime.UtcNow.AddSeconds(10);
        try { if(deliveryApp!=null && !deliveryApp.Exited) { try { DisconnectDelivery(); } finally { QuitDelivery(); } } }
        catch { Report["delivery_cleanup_unavailable"]=true; }
        finally { if(deliveryApp!=null) deliveryApp.Dispose(); }
    }
    static string Canonical(string path) {
        var handle=Native.CreateFileW(path,0,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero);if(handle==new IntPtr(-1)) Fail("delivery-path-unavailable");
        try { var value=new StringBuilder(32768);uint size=Native.GetFinalPathNameByHandleW(handle,value,(uint)value.Capacity,0);if(size==0 || size>=value.Capacity) Fail("delivery-path-unavailable");return value.ToString(); }
        finally { Native.CloseHandle(handle); }
    }
    static string HashFile(string path) {
        RegularPath(path);using(var stream=File.OpenRead(path)) {
            if(stream.Length<1 || stream.Length>64*1024*1024) Fail("delivery-host-provenance-unavailable");
            using(var hash=SHA256.Create()) {
                var buffer=new byte[65536];long total=0;int count;
                while((count=stream.Read(buffer,0,buffer.Length))>0) { DeliveryCheck();total+=count;if(total>64*1024*1024) Fail("delivery-host-provenance-unavailable");hash.TransformBlock(buffer,0,count,buffer,0); }
                hash.TransformFinalBlock(new byte[0],0,0);return BitConverter.ToString(hash.Hash);
            }
        }
    }
    static string RegistryManifest(string kind,int view) {
        string key=@"SOFTWARE\"+(kind=="edge" ? @"Microsoft\Edge" : @"Google\Chrome")+@"\NativeMessagingHosts\com.pickle.reader";
        uint type,bytes=0;uint flags=2U | (view==32 ? 0x20000U : 0x10000U);
        int code=Native.RegGetValueWString(new IntPtr(unchecked((int)0x80000001)),key,null,flags,out type,IntPtr.Zero,ref bytes);
        if(code==2 || code==3) return null;
        if(code!=0 || type!=1 || bytes<2 || bytes>65536 || bytes%2!=0) Fail("delivery-registry-unavailable");
        var buffer=Marshal.AllocHGlobal((int)bytes);
        try { code=Native.RegGetValueWString(new IntPtr(unchecked((int)0x80000001)),key,null,flags,out type,buffer,ref bytes);
            if(code!=0 || type!=1 || bytes<2 || bytes>65536 || bytes%2!=0) Fail("delivery-registry-unavailable");
            string result=Marshal.PtrToStringUni(buffer,(int)bytes/2);if(result.Length<1 || result[result.Length-1]!='\0') Fail("delivery-registry-unavailable");
            result=result.Substring(0,result.Length-1);if(result.IndexOf('\0')>=0) Fail("delivery-registry-unavailable");return result;
        } finally { Marshal.FreeHGlobal(buffer); }
    }
    static void VerifyRegistration(string kind,string id) {
        string directory=Path.Combine(deliverySupport,"NativeHost",kind),host=Path.Combine(directory,"pickle-browser-host.exe"),manifest=Path.Combine(directory,"com.pickle.reader.json"),origin="chrome-extension://"+id+"/";
        var value=ReadObject(manifest,16384);if(!BrowserExtensionDeliveryPredicates.HostShape(value,origin) || !String.Equals(Canonical((string)value["path"]),Canonical(host),StringComparison.OrdinalIgnoreCase)) Fail("delivery-host-manifest-mismatch");
        var registration=ReadObject(Path.Combine(directory,"registration.json"),16384);
        string app=registration.ContainsKey("application") ? registration["application"] as string : null,bridge=registration.ContainsKey("bridge_directory") ? registration["bridge_directory"] as string : null;
        if(registration.Count!=4 || !registration.ContainsKey("browser") || registration["browser"] as string!=(kind=="edge" ? "Edge" : "Chrome") ||
            !registration.ContainsKey("allowed_origin") || registration["allowed_origin"] as string!=origin || app==null || bridge==null ||
            !String.Equals(Canonical(app),deliveryApp.Executable,StringComparison.OrdinalIgnoreCase) || !String.Equals(Canonical(bridge),Canonical(Path.Combine(deliverySupport,"BrowserBridge")),StringComparison.OrdinalIgnoreCase)) Fail("delivery-host-registration-mismatch");
        if(HashFile(host)!=HashFile(Path.Combine(Path.GetDirectoryName(deliveryApp.InputExecutable),"pickle-browser-host.exe"))) Fail("delivery-host-provenance-unavailable");
        int found=0;foreach(int view in new[]{32,64}) { string path=RegistryManifest(kind,view);if(path==null) continue;if(!String.Equals(Canonical(path),Canonical(manifest),StringComparison.OrdinalIgnoreCase)) Fail("delivery-host-registry-mismatch");found++; }
        if(found==0) Fail("delivery-host-registry-mismatch");
    }
    static void NavigateArticle(IntPtr window) {
        if(!browser.OwnsWindow(window)) Fail("delivery-browser-not-owned");Native.SetForegroundWindow(window);
        Wait(()=>browser.OwnsWindow(window) && Native.GetForegroundWindow()==window,5,"delivery-browser-not-foreground");
        Native.AddressShortcut(window);AutomationElement address=null;
        Wait(()=> { address=FocusedAddress(window);return address!=null; },5,"owned-address-bar-unavailable");
        object pattern;if(!address.TryGetCurrentPattern(ValuePattern.Pattern,out pattern)) Fail("address-value-pattern-unavailable");
        ((ValuePattern)pattern).SetValue(BrowserExtensionDeliveryPredicates.ArticleUrl);
        if(((ValuePattern)pattern).Current.Value!=BrowserExtensionDeliveryPredicates.ArticleUrl) Fail("delivery-article-url-mismatch");
        Native.Enter(window);
        Wait(()=> {
            var documents=new List<AutomationElement>();foreach(var node in Tree(window,false)) {
                if(!DiagnosticBound(node,window)) return false;
                if(node.Current.ControlType==ControlType.Document && node.Current.Name==BrowserExtensionDeliveryPredicates.ArticleTitle && !node.Current.IsOffscreen) documents.Add(node);
            }
            if(documents.Count!=1) return false;var document=documents[0];object value,text;
            if(!DiagnosticBound(document,window) || !document.TryGetCurrentPattern(ValuePattern.Pattern,out value) ||
                !DiagnosticBound(document,window) || ((ValuePattern)value).Current.Value!=BrowserExtensionDeliveryPredicates.ArticleUrl || !document.TryGetCurrentPattern(TextPattern.Pattern,out text)) return false;
            if(!DiagnosticBound(document,window)) return false;
            var range=((TextPattern)text).DocumentRange;
            if(BrowserExtensionDeliveryPredicates.Normalize(range.GetText(2048))!=BrowserExtensionDeliveryPredicates.ArticleText) return false;
            foreach(var selected in ((TextPattern)text).GetSelection()) { if(!DiagnosticBound(document,window) || !String.IsNullOrWhiteSpace(selected.GetText(2048))) return false; }
            return browser.OwnsWindow(window) && Native.GetForegroundWindow()==window && DiagnosticBound(document,window) && ((ValuePattern)value).Current.Value==BrowserExtensionDeliveryPredicates.ArticleUrl;
        },15,"delivery-owned-article-unavailable");
        Report["owned_article_verified"]=true;
    }
    sealed class DeliveryApplication : IDisposable {
        readonly Process process;readonly long birth;public readonly string Executable,InputExecutable;IntPtr window;
        public DeliveryApplication(int pid,long ticks,string path) { process=Process.GetProcessById(pid);var retained=process.Handle;birth=ticks;InputExecutable=path;Executable=Canonical(path);Verify(); }
        public bool Exited { get { return process.HasExited; } }public int ExitCode { get { return process.ExitCode; } }public IntPtr Handle { get { return window; } }
        public DateTime StartUtc { get { return new DateTime(birth,DateTimeKind.Utc); } }
        void Verify() {
            DeliveryCheck();var path=new StringBuilder(32768);int length=path.Capacity;
            if(process.HasExited || process.StartTime.ToUniversalTime().Ticks!=birth || Native.WaitForSingleObject(process.Handle,0)!=258 ||
                !Native.QueryFullProcessImageNameW(process.Handle,0,path,ref length) || !String.Equals(Canonical(path.ToString()),Executable,StringComparison.OrdinalIgnoreCase)) Fail("delivery-app-owner-changed");
        }
        public bool OwnsWindow(IntPtr value) { uint pid;if(value==IntPtr.Zero || !Native.IsWindow(value) || Native.GetWindowThreadProcessId(value,out pid)==0 || pid!=(uint)process.Id || Native.GetAncestor(value,2)!=value) return false;Verify();return true; }
        public IntPtr Window() {
            Verify();var found=new List<IntPtr>();bool unavailable=false;int observed=0,visible=0;
            bool enumerated=Native.EnumWindows(delegate(IntPtr candidate,IntPtr ignored) { try {
                if(++observed>256) { unavailable=true;return false; }
                if(Native.IsWindowVisible(candidate) && OwnsWindow(candidate)) {
                    visible++;Native.Rect rect;int cloaked;
                    // Match the already exercised ordinary-reading selector,
                    // while refusing unknown native geometry/cloaking outright.
                    if(!Native.GetWindowRect(candidate,out rect) || Native.DeliveryWindowCloak(candidate,14,out cloaked,4)!=0) {unavailable=true;return false;}
                    if(BrowserExtensionDeliveryPredicates.EligibleReader(true,true,true,true,cloaked,(long)rect.Right-rect.Left,(long)rect.Bottom-rect.Top)) found.Add(candidate);
                }
                return true;
            } catch { unavailable=true;return false; } },IntPtr.Zero);
            Report["application_visible_root_windows"]=visible;Report["application_eligible_reader_windows"]=found.Count;
            if(!enumerated) unavailable=true;
            if(unavailable) Fail("delivery-app-owner-changed");
            Verify();
            if(found.Count>1) Fail("delivery-app-window-ambiguous");window=BrowserExtensionDeliveryPredicates.UniqueEligibleReader(found.Count) ? found[0] : IntPtr.Zero;return window;
        }
        bool Bound(AutomationElement node,bool passwordMetadata=false) {
            if(node==null || !OwnsWindow(window)) return false;var root=AutomationElement.FromHandle(window);
            for(int depth=0;node!=null && depth<64;depth++) { DeliveryCheck();if(node.Current.ProcessId!=process.Id || (!passwordMetadata && node.Current.IsPassword)) return false;if(Automation.Compare(node,root)) return true;node=TreeWalker.RawViewWalker.GetParent(node); }return false;
        }
        List<AutomationElement> Nodes(AutomationElement root) {
            var queue=new Queue<AutomationElement>();var result=new List<AutomationElement>();queue.Enqueue(root);
            while(queue.Count>0) {
                DeliveryCheck();if(result.Count+queue.Count>1024) Fail("delivery-app-tree-bound");var node=queue.Dequeue();if(!Bound(node,true)) Fail("delivery-app-element-not-owned");
                // Credentials are not part of this scenario. Prune the exact
                // owned password branch before strings, values or children.
                if(node.Current.IsPassword) continue;result.Add(node);
                var child=TreeWalker.RawViewWalker.GetFirstChild(node);while(child!=null) { DeliveryCheck();if(result.Count+queue.Count>=1024) Fail("delivery-app-tree-bound");queue.Enqueue(child);child=TreeWalker.RawViewWalker.GetNextSibling(child); }
            }return result;
        }
        public AutomationElement Find(string id) {
            if(window==IntPtr.Zero && Window()==IntPtr.Zero) return null;
            var matches=Nodes(AutomationElement.FromHandle(window)).Where(n=>Bound(n) && n.Current.AutomationId==id).ToArray();if(matches.Length>1) Fail("delivery-app-control-ambiguous");return matches.SingleOrDefault();
        }
        public IEnumerable<string> Texts(AutomationElement root,bool visibleOnly=false) {
            foreach(var node in Nodes(root)) { if(!Bound(node)) Fail("delivery-app-element-not-owned");if(node.Current.ControlType!=ControlType.Text) continue;
                if(visibleOnly && node.Current.IsOffscreen) continue;
                if(!Bound(node)) Fail("delivery-app-element-not-owned");string name=node.Current.Name;if(name.Length>4096) Fail("delivery-app-label-bound");yield return name;
            }
        }
        public bool HasText(string expected) { return Texts(AutomationElement.FromHandle(window)).Any(value=>value==expected); }
        public void Focus() { if(!OwnsWindow(window)) Fail("delivery-app-owner-changed");Native.SetForegroundWindow(window);DeliveryWait(()=>OwnsWindow(window) && Native.GetForegroundWindow()==window,3,"delivery-app-focus-unavailable"); }
        AutomationElement Control(string id) {
            Focus();
            for(int page=0;page<16;page++) {
                var node=Find(id);if(node!=null && !node.Current.IsOffscreen && node.Current.IsEnabled) return node;
                if(node!=null) { object scroll;if(node.TryGetCurrentPattern(ScrollItemPattern.Pattern,out scroll)) { ((ScrollItemPattern)scroll).ScrollIntoView();Thread.Sleep(50);continue; } }
                var header=Find("open-settings");if(header==null || !Bound(header)) Fail("delivery-app-control-unavailable");header.SetFocus();
                DeliveryWait(()=> { var focused=AutomationElement.FocusedElement;return focused!=null && Bound(focused) && focused.Current.AutomationId=="open-settings"; },2,"delivery-app-control-unavailable");
                Native.DeliveryKey(window,OwnsWindow,page==0 ? (ushort)0x24 : (ushort)0x22,false,true);Thread.Sleep(50);
            }Fail("delivery-app-control-unavailable");return null;
        }
        public void Invoke(string id) { var node=Control(id);if(!Bound(node)) Fail("delivery-app-element-not-owned");object pattern;if(!node.TryGetCurrentPattern(InvokePattern.Pattern,out pattern)) Fail("delivery-app-invoke-unavailable");((InvokePattern)pattern).Invoke(); }
        public void SetField(string id,string value) {
            var node=Control(id);if(!Bound(node)) Fail("delivery-app-element-not-owned");object pattern;if(!node.TryGetCurrentPattern(ValuePattern.Pattern,out pattern) || ((ValuePattern)pattern).Current.IsReadOnly) Fail("delivery-app-value-unavailable");((ValuePattern)pattern).SetValue(value);
            DeliveryWait(()=> { var field=Find(id);object current;return field!=null && Bound(field) && field.TryGetCurrentPattern(ValuePattern.Pattern,out current) && ((ValuePattern)current).Current.Value==value; },3,"delivery-app-value-not-applied");
        }
        public void Dispose() { process.Dispose(); }
    }
    static partial class Native {
        [DllImport("dwmapi.dll",EntryPoint="DwmGetWindowAttribute")] public static extern int DeliveryWindowCloak(IntPtr window,uint attribute,out int value,int size);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr CreateFileW(string path,uint access,uint sharing,IntPtr security,uint creation,uint flags,IntPtr template);
        [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] public static extern uint GetFinalPathNameByHandleW(IntPtr handle,StringBuilder value,uint length,uint flags);
        [DllImport("advapi32.dll",EntryPoint="RegGetValueW",CharSet=CharSet.Unicode)] public static extern int RegGetValueWString(IntPtr key,string subkey,string value,uint flags,out uint type,IntPtr data,ref uint bytes);
        public static void BrowserSendPage(IntPtr window) {
            Unheld(0x50);Send(window,new[]{Key(0x12,false),Key(0x10,false),Key(0x50,false),Key(0x50,true),Key(0x10,true),Key(0x12,true)},new[]{Key(0x50,true),Key(0x10,true),Key(0x12,true)});
        }
        public static void DeliveryKey(IntPtr window,Func<IntPtr,bool> owned,ushort key,bool control,bool extended) {
            DeliveryCheck();if(!owned(window) || GetForegroundWindow()!=window) Fail("delivery-input-target-changed");Unheld(key);
            var down=Key(key,false);var up=Key(key,true);if(extended) { down.Data.Keyboard.Flags|=1;up.Data.Keyboard.Flags|=1; }
            var inputs=control ? new[]{Key(0x11,false),down,up,Key(0x11,true)} : new[]{down,up};
            if(SendInput((uint)inputs.Length,inputs,Marshal.SizeOf(typeof(Input)))!=(uint)inputs.Length) {
                if(owned(window) && GetForegroundWindow()==window) SendInput(control ? 2U : 1U,control ? new[]{up,Key(0x11,true)} : new[]{up},Marshal.SizeOf(typeof(Input)));
                Fail("delivery-input-incomplete");
            }
        }
    }
}
#endif
