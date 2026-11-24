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
    public static bool ShouldInvokeDisconnect(bool connectAttempted,bool disconnectAttempted) { return connectAttempted && !disconnectAttempted; }
    public static bool ReaderFocusCandidate(string id,int role,bool inside,bool visible,bool enabled,bool focusable,bool bounds) {
        return (id=="simplify" || id=="explain-term-submit") && role==50000 && inside && visible && enabled && focusable && bounds;
    }
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
        BrowserExtensionChromeActionPredicates.SelfTest();
        BrowserExtensionPopupPredicates.SelfTest();
        if(!EligibleReader(true,true,true,true,0,480,180) || EligibleReader(false,true,true,true,0,480,180) ||
            EligibleReader(true,false,true,true,0,480,180) || EligibleReader(true,true,false,true,0,480,180) ||
            EligibleReader(true,true,true,false,0,480,180) || EligibleReader(true,true,true,true,1,480,180) ||
            EligibleReader(true,true,true,true,0,479,180) || EligibleReader(true,true,true,true,0,480,179)) throw new Exception("delivery-reader-eligibility");
        if(!UniqueEligibleReader(1) || UniqueEligibleReader(0) || UniqueEligibleReader(2) || UniqueEligibleReader(256)) throw new Exception("delivery-reader-uniqueness");
        if(!ShouldInvokeDisconnect(true,false) || ShouldInvokeDisconnect(false,false) || ShouldInvokeDisconnect(true,true)) throw new Exception("delivery-disconnect-after-attempt-once");
        if(!ReaderFocusCandidate("simplify",50000,true,true,true,true,true) || !ReaderFocusCandidate("explain-term-submit",50000,true,true,true,true,true) ||
            ReaderFocusCandidate("open-settings",50000,true,true,true,true,true) || ReaderFocusCandidate("simplify",50004,true,true,true,true,true) ||
            ReaderFocusCandidate("simplify",50000,false,true,true,true,true) || ReaderFocusCandidate("simplify",50000,true,false,true,true,true) ||
            ReaderFocusCandidate("simplify",50000,true,true,false,true,true) || ReaderFocusCandidate("simplify",50000,true,true,true,false,true) ||
            ReaderFocusCandidate("simplify",50000,true,true,true,true,false)) throw new Exception("delivery-reader-focus-candidate");
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

public static class BrowserExtensionPopupPredicates {
    public const string ExtensionTitle="Pickle — page and video context",Heading="Bring context to Pickle",UsePage="Use this page";
    public static string Status(string value) {
        switch(value) {
        case "Ready in Pickle.":return "ready";
        case "Browser extension origin rejected.":return "origin-rejected";
        case "Connect the browser extension in Pickle Settings first.":return "registration-missing";
        case "Invalid browser host registration.":return "registration-invalid";
        case "Open Pickle and check that browser context is enabled.":return "context-unavailable";
        case "Could not connect. Check the Pickle browser setup.":return "connection-error";
        case "No article found. Try selecting a passage.":return "no-article";
        case "The page changed. Try again.":return "page-changed";
        case "Private tabs are not supported.":return "private-tab";
        case "Open a webpage first.":return "open-webpage";
        case "For YouTube, open “Show transcript” first. Embedded players may require opening the video’s own page.":return "initial-help";
        default:return "unknown";
        }
    }
    public static bool Role(bool toolbar,bool popup,int role) {return role==50000 || (!toolbar && !popup && (role==50011 || role==50031));}
    public static bool Action(int count,bool bound,bool visible,bool enabled,bool geometry,bool invoke) {return count==1 && bound && visible && enabled && geometry && invoke;}
    public static void SelfTest() {
        if(Status("Browser extension origin rejected.")!="origin-rejected" || Status("Connect the browser extension in Pickle Settings first.")!="registration-missing" || Status("Invalid browser host registration.")!="registration-invalid" || Status("Open Pickle and check that browser context is enabled.")!="context-unavailable") throw new Exception("popup-closed-host-status");
        if(Status("Ready in Pickle.")!="ready" || Status("The page changed. Try again.")!="page-changed" || Status("private unknown error")!="unknown" || Status("Ready in Pickle. private")!="unknown") throw new Exception("popup-closed-status");
        if(!Role(true,false,50000) || !Role(false,false,50011) || !Role(false,false,50031) || !Role(false,true,50000) || Role(true,false,50011) || Role(false,true,50011) || Role(false,false,50004) || Role(false,false,50030)) throw new Exception("popup-closed-action-roles");
        if(!Action(1,true,true,true,true,true)) throw new Exception("popup-action");
        foreach(int count in new[]{0,2,512}) if(Action(count,true,true,true,true,true)) throw new Exception("popup-unique-action");
        for(int i=0;i<5;i++) {var facts=new[]{true,true,true,true,true};facts[i]=false;if(Action(1,facts[0],facts[1],facts[2],facts[3],facts[4])) throw new Exception("popup-no-action-without-proof");}
    }
}

public static class BrowserExtensionChromeActionPredicates {
    public static bool MayObserve(bool full,string kind,string stage,string failure,bool registered,bool article,int shortcut,bool routeKnown,bool readerObserved) {
        return full && kind=="chrome" && stage=="delivery-context" && failure=="delivery-context-unavailable" && registered && article && shortcut==6 && routeKnown && !readerObserved;
    }
    public static bool Role(int value) { return value==50000 || value==50009 || value==50011 || value==50031; }
    public static int Label(string value) { return value=="Extensions" ? 1 : value=="Pickle — page and video context" ? 2 : 0; }
    public static void SelfTest() {
        if(!MayObserve(true,"chrome","delivery-context","delivery-context-unavailable",true,true,6,true,false)) throw new Exception("chrome-action-boundary");
        if(MayObserve(false,"chrome","delivery-context","delivery-context-unavailable",true,true,6,true,false) ||
            MayObserve(true,"edge","delivery-context","delivery-context-unavailable",true,true,6,true,false) ||
            MayObserve(true,"chrome","delivery-connect","delivery-connect-unavailable",true,true,6,true,false) ||
            MayObserve(true,"chrome","delivery-context","stage-deadline",true,true,6,true,false) ||
            MayObserve(true,"chrome","delivery-context","delivery-context-unavailable",false,true,6,true,false) ||
            MayObserve(true,"chrome","delivery-context","delivery-context-unavailable",true,false,6,true,false) ||
            MayObserve(true,"chrome","delivery-context","delivery-context-unavailable",true,true,5,true,false) ||
            MayObserve(true,"chrome","delivery-context","delivery-context-unavailable",true,true,6,false,false) ||
            MayObserve(true,"chrome","delivery-context","delivery-context-unavailable",true,true,6,true,true)) throw new Exception("chrome-action-no-unproved-trigger");
        if(Label("Extensions")!=1 || Label("Pickle — page and video context")!=2 || Label("Extensions menu")!=0 || Label("Extensions ")!=0 || Label("unknown private label")!=0 || Label(null)!=0) throw new Exception("chrome-action-closed-labels");
        if(!Role(50000) || !Role(50009) || !Role(50011) || !Role(50031) || Role(50004) || Role(50020) || Role(50030)) throw new Exception("chrome-action-chrome-controls-only");
    }
}

#if !PICKLE_PREDICATES
public static partial class BrowserExtensionFixtureProgram {
    sealed class ChromePopupSession : IDisposable {
        sealed class Surface : IDisposable {
            public readonly IntPtr Window,Owner;public readonly uint Pid;public readonly AutomationElement Root;readonly ChromePopupSession session;
            public Surface(ChromePopupSession context,IntPtr window) {session=context;Window=window;uint pid;Native.GetWindowThreadProcessId(window,out pid);Pid=pid;Owner=Native.OpenProcess(0x00101000,false,pid);try {Root=AutomationElement.FromHandle(window);if(!Check()) Fail("delivery-context-unavailable");} catch {if(Owner!=IntPtr.Zero) Native.CloseHandle(Owner);throw;}}
            public bool Check() {uint pid;return session.Time() && session.Foreground() && session.Related(Window) && browser.OwnsLiveProcess(Owner) && Native.GetProcessId(Owner)==Pid && Native.GetWindowThreadProcessId(Window,out pid)!=0 && pid==Pid && Automation.Compare(Root,AutomationElement.FromHandle(Window));}
            public bool Bound(AutomationElement node,AutomationElement document=null,bool passwordLeaf=false) {
                bool first=true,seen=document==null;
                for(int depth=0;node!=null && depth<32;depth++) {
                    if(!Check() || !browser.Owns(node.Current.ProcessId) || ((node.Current.IsPassword || node.Current.ControlType==ControlType.Edit) && !(first && passwordLeaf))) return false;
                    if(node.Current.ControlType==ControlType.Document) {if(document==null || !Automation.Compare(node,document)) return false;seen=true;}
                    if(Automation.Compare(node,Root)) return seen && Check();
                    IntPtr native=new IntPtr(node.Current.NativeWindowHandle);
                    if(native!=IntPtr.Zero && Native.GetAncestor(native,2)==native && native!=Window) return false;
                    first=false;node=TreeWalker.RawViewWalker.GetParent(node);
                }return false;
            }
            public List<AutomationElement> Nodes(AutomationElement scope=null) {
                int visited=0;var result=new List<AutomationElement>();var queue=new Queue<KeyValuePair<AutomationElement,int>>();queue.Enqueue(new KeyValuePair<AutomationElement,int>(scope??Root,0));
                while(queue.Count>0) {
                    if(!Check() || ++visited>512) Fail("delivery-context-unavailable");var entry=queue.Dequeue();var node=entry.Key;
                    var document=scope??(node.Current.ControlType==ControlType.Document ? node : null);
                    if(entry.Value>24 || !Bound(node,document,true)) Fail("delivery-context-unavailable");
                    if(node.Current.IsPassword || node.Current.ControlType==ControlType.Edit) continue;result.Add(node);
                    if(scope==null && node.Current.ControlType==ControlType.Document) continue;
                    if(entry.Value>=24) Fail("delivery-context-unavailable");var child=TreeWalker.RawViewWalker.GetFirstChild(node);
                    while(child!=null) {if(!session.Time() || visited+queue.Count>=512) Fail("delivery-context-unavailable");queue.Enqueue(new KeyValuePair<AutomationElement,int>(child,entry.Value+1));child=TreeWalker.RawViewWalker.GetNextSibling(child);}
                }return result;
            }
            public bool Read(AutomationElement node,AutomationElement document,Func<string> get,out string value,Func<bool> eligible=null) {return BrowserExtensionDetailsPredicates.Read(()=>Bound(node,document) && (eligible==null || eligible()) && Bound(node,document),get,out value);}
            public AutomationElement Document(string uri,string title) {
                var found=new List<AutomationElement>();foreach(var node in Nodes()) {
                    if(node.Current.ControlType!=ControlType.Document || node.Current.IsOffscreen || !Bound(node,node)) continue;
                    object value;if(!node.TryGetCurrentPattern(ValuePattern.Pattern,out value) || !Bound(node,node)) continue;
                    string actual;if(!Read(node,node,()=>((ValuePattern)value).Current.Value,out actual)) Fail("delivery-context-unavailable");if(actual!=uri) continue;
                    string name;if(!Read(node,node,()=>node.Current.Name,out name)) Fail("delivery-context-unavailable");if(name==title) found.Add(node);
                }if(found.Count>1) Fail("delivery-context-unavailable");return found.SingleOrDefault();
            }
            public bool Actionable(AutomationElement node,AutomationElement doc) {
                if(!Bound(node,doc) || !node.Current.IsEnabled || node.Current.IsOffscreen) return false;Native.Rect rect;if(!Native.GetWindowRect(Window,out rect)) return false;
                var b=node.Current.BoundingRectangle;object invoke;bool supported=node.TryGetCurrentPattern(InvokePattern.Pattern,out invoke);return BrowserExtensionPopupPredicates.Action(1,Bound(node,doc),!node.Current.IsOffscreen,node.Current.IsEnabled,BrowserExtensionPredicates.BoundsFlags(b.X,b.Y,b.Width,b.Height,rect.Left,rect.Top,rect.Right,rect.Bottom)==3,supported);
            }
            public void Invoke(AutomationElement node,AutomationElement doc) {if(!session.Foreground() || !Actionable(node,doc)) Fail("delivery-context-unavailable");object value;if(!node.TryGetCurrentPattern(InvokePattern.Pattern,out value) || !Bound(node,doc) || !session.Foreground()) Fail("delivery-context-unavailable");((InvokePattern)value).Invoke();}
            public void Dispose() {if(Owner!=IntPtr.Zero) Native.CloseHandle(Owner);}
        }
        sealed class Target {public Surface Surface;public AutomationElement Node,Document;}
        readonly IntPtr main;readonly DateTime end;readonly Dictionary<IntPtr,Surface> surfaces=new Dictionary<IntPtr,Surface>();readonly Dictionary<string,object> observation;
        Surface primary;AutomationElement article,popupDocument;Surface popupSurface;readonly string popupUri;
        public ChromePopupSession(IntPtr window,string id,DateTime limit) {
            main=window;end=limit;popupUri="chrome-extension://"+id+"/popup.html";
            observation=new Dictionary<string,object>{{"stage","toolbar"},{"article_verified",false},{"toolbar_observed",false},{"toolbar_attempts",0},{"toolbar_invocations",0},
                {"extension_action_observed",false},{"extension_action_attempts",0},{"extension_action_invocations",0},{"related_surface_verified",false},
                {"popup_origin_verified",false},{"popup_title_verified",false},{"popup_heading_verified",false},{"use_page_observed",false},{"use_page_attempts",0},{"use_page_invocations",0},{"status","unavailable"}};
            Report["popup_delivery"]=observation;
            try {if(!Foreground()) Fail("delivery-context-unavailable");primary=new Surface(this,main);surfaces.Add(main,primary);article=primary.Document(BrowserExtensionDeliveryPredicates.ArticleUrl,BrowserExtensionDeliveryPredicates.ArticleTitle);if(article==null) Fail("delivery-context-unavailable");observation["article_verified"]=true;}
            catch {Dispose();throw;}
        }
        bool Time() {DeliveryCheck();return DateTime.UtcNow<end;}
        bool Related(IntPtr window) {
            if(!Time() || window==IntPtr.Zero || !Native.IsWindow(window) || !Native.IsWindowVisible(window) || !browser.OwnsWindow(window)) return false;
            if(window==main) return Native.GetAncestor(window,2)==main;
            var seen=new HashSet<IntPtr>();for(int depth=0;window!=IntPtr.Zero && depth<16 && seen.Add(window);depth++) {if(!browser.OwnsWindow(window)) return false;window=Native.GetWindow(window,4);if(window==main) return true;}
            return false;
        }
        bool Foreground() {return Time() && Related(Native.GetForegroundWindow());}
        List<Surface> Surfaces() {
            if(!Foreground() || !primary.Check()) Fail("delivery-context-unavailable");var windows=new List<IntPtr>{main};int examined=0;bool unavailable=false;
            bool complete=Native.EnumWindows(delegate(IntPtr window,IntPtr ignored) {try {if(++examined>256 || !Time()) {unavailable=true;return false;}if(window!=main && Related(window)) {windows.Add(window);if(windows.Count>8) {unavailable=true;return false;}}return true;} catch {unavailable=true;return false;}},IntPtr.Zero);
            DeliveryCheck();if(!complete || unavailable) Fail("delivery-context-unavailable");var result=new List<Surface>();
            foreach(var window in windows) {Surface surface;if(surfaces.TryGetValue(window,out surface) && !surface.Check()) {surface.Dispose();surfaces.Remove(window);surface=null;}if(surface==null) {surface=new Surface(this,window);surfaces.Add(window,surface);}result.Add(surface);}return result;
        }
        bool Article() {if(!Foreground() || !primary.Check()) return false;var current=primary.Document(BrowserExtensionDeliveryPredicates.ArticleUrl,BrowserExtensionDeliveryPredicates.ArticleTitle);return current!=null && Automation.Compare(current,article);}
        Target FindAction(string name,bool toolbar,AutomationElement document=null,Surface scoped=null) {
            var found=new List<Target>();foreach(var surface in scoped!=null ? new List<Surface>{scoped} : toolbar ? new List<Surface>{primary} : Surfaces()) {
                foreach(var node in surface.Nodes(document)) {
                    int role=node.Current.ControlType.Id;if(!BrowserExtensionPopupPredicates.Role(toolbar,document!=null,role)) continue;
                    string label;if(!surface.Read(node,document,()=>node.Current.Name,out label,()=>BrowserExtensionPopupPredicates.Role(toolbar,document!=null,node.Current.ControlType.Id))) Fail("delivery-context-unavailable");
                    if(label==name && surface.Actionable(node,document)) found.Add(new Target{Surface=surface,Node=node,Document=document});
                }
            }if(found.Count>1) Fail("delivery-context-unavailable");return found.SingleOrDefault();
        }
        Target WaitAction(string name,bool toolbar,AutomationElement doc=null,Surface surface=null) {do {if(!Time()) break;var found=FindAction(name,toolbar,doc,surface);if(found!=null) return found;Thread.Sleep(100);} while(Time());Fail("delivery-context-unavailable");return null;}
        void InvokeOnce(Target target,string name,string prefix,bool toolbar) {
            if(!Article()) Fail("delivery-context-unavailable");var fresh=FindAction(name,toolbar,target.Document,target.Document!=null ? target.Surface : null);
            if(fresh==null || !Automation.Compare(fresh.Node,target.Node) || fresh.Surface.Window!=target.Surface.Window || !Foreground()) Fail("delivery-context-unavailable");
            if(prefix=="use_page" && (!Automation.Compare(popupSurface.Document(popupUri,"Pickle"),popupDocument) || !Heading())) Fail("delivery-context-unavailable");
            observation[prefix+"_observed"]=true;observation[prefix+"_attempts"]=1;fresh.Surface.Invoke(fresh.Node,fresh.Document);observation[prefix+"_invocations"]=1;
        }
        public void Send() {
            InvokeOnce(WaitAction("Extensions",true),"Extensions","toolbar",true);
            observation["stage"]="extension-menu";InvokeOnce(WaitAction(BrowserExtensionPopupPredicates.ExtensionTitle,false),BrowserExtensionPopupPredicates.ExtensionTitle,"extension_action",false);
            observation["stage"]="popup-document";
            do {if(!Time()) break;var found=new List<Target>();foreach(var surface in Surfaces()) {var doc=surface.Document(popupUri,"Pickle");if(doc!=null) found.Add(new Target{Surface=surface,Document=doc});}if(found.Count>1) Fail("delivery-context-unavailable");if(found.Count==1) {popupSurface=found[0].Surface;popupDocument=found[0].Document;break;}Thread.Sleep(100);} while(Time());
            if(popupDocument==null || !Article()) Fail("delivery-context-unavailable");observation["related_surface_verified"]=true;observation["popup_origin_verified"]=true;observation["popup_title_verified"]=true;
            if(!Heading()) Fail("delivery-context-unavailable");observation["popup_heading_verified"]=true;observation["stage"]="use-page";
            var action=WaitAction(BrowserExtensionPopupPredicates.UsePage,false,popupDocument,popupSurface);
            if(!popupSurface.Check() || !Automation.Compare(popupSurface.Document(popupUri,"Pickle"),popupDocument) || !Heading()) Fail("delivery-context-unavailable");
            InvokeOnce(action,BrowserExtensionPopupPredicates.UsePage,"use_page",false);observation["stage"]="sent";PollStatus();
        }
        bool Heading() {int count=0;foreach(var node in popupSurface.Nodes(popupDocument)) {if(node.Current.ControlType!=ControlType.Text) continue;string label;if(!popupSurface.Read(node,popupDocument,()=>node.Current.Name,out label,()=>node.Current.ControlType==ControlType.Text)) return false;if(label==BrowserExtensionPopupPredicates.Heading) count++;}return count==1;}
        public void PollStatus() {
            try {if(!Foreground() || popupSurface==null || !popupSurface.Check()) return;var doc=popupSurface.Document(popupUri,"Pickle");if(doc==null || !Automation.Compare(doc,popupDocument)) return;
                foreach(var node in popupSurface.Nodes(doc)) {if(node.Current.ControlType!=ControlType.Text) continue;string value;if(!popupSurface.Read(node,doc,()=>node.Current.Name,out value,()=>node.Current.ControlType==ControlType.Text)) return;string category=BrowserExtensionPopupPredicates.Status(value);if(category!="unknown" && (category!="initial-help" || (string)observation["status"]=="unavailable")) observation["status"]=category;}
            } catch { /* Status may disappear when accepted context opens Pickle. */ }
        }
        public void Dispose() {foreach(var surface in surfaces.Values) surface.Dispose();surfaces.Clear();}
    }
    sealed class ChromeActionReader {
        readonly IntPtr window,owner;readonly uint pid;readonly DateTime end;readonly AutomationElement root;
        public ChromeActionReader(IntPtr windowValue,IntPtr ownerValue,uint pidValue,DateTime limit) {window=windowValue;owner=ownerValue;pid=pidValue;end=limit;root=AutomationElement.FromHandle(window);}
        public bool Within() { return DateTime.UtcNow<end && LayoutWindowBound(window,owner,pid) && Automation.Compare(root,AutomationElement.FromHandle(window)); }
        public bool Bound(AutomationElement node,bool documentSelf=false,bool passwordLeaf=false) {
            bool first=true;
            for(int depth=0;node!=null && depth<32;depth++) {
                if(!Within() || !browser.Owns(node.Current.ProcessId) || (node.Current.IsPassword && !(first && passwordLeaf))) return false;
                if(node.Current.ControlType==ControlType.Document && !(first && documentSelf)) return false;
                if(Automation.Compare(node,root)) return Within();
                first=false;node=TreeWalker.RawViewWalker.GetParent(node);
            }return false;
        }
        public bool ArticleProof(AutomationElement expected,out AutomationElement document,out List<AutomationElement> chrome,Action<int> count) {
            document=null;chrome=new List<AutomationElement>();var documents=new List<AutomationElement>();
            var queue=new Queue<KeyValuePair<AutomationElement,int>>();queue.Enqueue(new KeyValuePair<AutomationElement,int>(root,0));int nodes=0;
            while(queue.Count>0) {
                if(!Within() || nodes+queue.Count>512) return false;
                var entry=queue.Dequeue();var node=entry.Key;count(++nodes);
                if(entry.Value>24 || !Bound(node,true,true)) return false;
                if(node.Current.IsPassword || node.Current.ControlType==ControlType.Edit) continue;
                if(node.Current.ControlType==ControlType.Document) {
                    // Read only the fixed article URL/title proof, never page
                    // text, descendants, links, inputs, or arbitrary tab titles.
                    if(node.Current.IsOffscreen) continue;
                    object pattern;if(!Bound(node,true) || !node.TryGetCurrentPattern(ValuePattern.Pattern,out pattern) || !Bound(node,true)) return false;
                    string uri=((ValuePattern)pattern).Current.Value;if(uri==null || uri.Length>4096 || !Bound(node,true)) return false;
                    if(uri==BrowserExtensionDeliveryPredicates.ArticleUrl) {
                        string title;if(!BrowserExtensionDetailsPredicates.Read(()=>Bound(node,true),()=>node.Current.Name,out title)) return false;
                        if(title==BrowserExtensionDeliveryPredicates.ArticleTitle && !node.Current.IsOffscreen && Bound(node,true)) documents.Add(node);
                    }
                    continue; // Always prune all document-content subtrees.
                }
                chrome.Add(node);
                if(entry.Value>=24) return false;
                var child=TreeWalker.RawViewWalker.GetFirstChild(node);
                while(child!=null) {
                    if(!Within() || nodes+queue.Count>=512) return false;
                    queue.Enqueue(new KeyValuePair<AutomationElement,int>(child,entry.Value+1));child=TreeWalker.RawViewWalker.GetNextSibling(child);
                }
            }
            if(documents.Count!=1 || !Within() || (expected!=null && !Automation.Compare(expected,documents[0]))) return false;
            document=documents[0];return true;
        }
        public List<int> Ancestors(AutomationElement node,out bool truncated) {
            var roles=new List<int>();truncated=true;
            for(int depth=0;node!=null && depth<8;depth++) {
                if(!Bound(node)) Fail("uia-element-not-owned");roles.Add(node.Current.ControlType.Id);
                if(Automation.Compare(node,root)) {truncated=false;break;}
                node=TreeWalker.RawViewWalker.GetParent(node);
            }return roles;
        }
    }
    static bool DeliveryReportTrue(string key) {object value;return Report.TryGetValue(key,out value) && value is bool && (bool)value;}
    static void ObserveChromeActionBoundary(IntPtr window,bool full,string failure) {
        object kind,rawReveal,rawRoute,rawShortcut;var reveal=Report.TryGetValue("reader_reveal",out rawReveal) ? rawReveal as Dictionary<string,object> : null;
        bool routeKnown=reveal!=null && reveal.TryGetValue("reader_observed",out rawRoute) && rawRoute is bool;
        bool readerObserved=routeKnown && (bool)reveal["reader_observed"];
        int shortcut=Report.TryGetValue("delivery_shortcut_events",out rawShortcut) && rawShortcut is int ? (int)rawShortcut : 0;
        if(!Report.TryGetValue("browser",out kind) || !(kind is string) || !BrowserExtensionChromeActionPredicates.MayObserve(full,(string)kind,stage,failure,DeliveryReportTrue("native_host_registered") && DeliveryReportTrue("owned_registration_verified"),DeliveryReportTrue("owned_article_verified"),shortcut,routeKnown,readerObserved)) return;
        var records=new List<Dictionary<string,object>>();var observation=new Dictionary<string,object>{{"stage","before-ownership"},{"incomplete",true},{"unavailable",true},
            {"before_article_verified",false},{"after_article_verified",false},{"before_nodes",0},{"after_nodes",0},{"nodes",0},{"controls",0},
            {"matching_count",0},{"extensions_exact_count",0},{"extension_title_exact_count",0},{"unknown_control_count",0},{"property_errors",0},{"truncated",false},{"records",records}};
        Report["chrome_action_observation"]=observation;IntPtr owner=IntPtr.Zero;
        // Cooperative allowance only: individual COM calls can outlast it.
        DateTime end=DateTime.UtcNow.AddMilliseconds(750);if(end>deadline) end=deadline;
        try {
            Check();uint pid;if(browser==null || !browser.OwnsWindow(window) || Native.GetWindowThreadProcessId(window,out pid)==0 || pid==0) return;
            owner=Native.OpenProcess(0x00101000,false,pid);if(DateTime.UtcNow>=end || !LayoutWindowBound(window,owner,pid)) return;
            var reader=new ChromeActionReader(window,owner,pid,end);AutomationElement document;List<AutomationElement> chrome;
            observation["stage"]="before-article";
            if(!reader.ArticleProof(null,out document,out chrome,n=>observation["before_nodes"]=n)) return;
            observation["before_article_verified"]=true;observation["stage"]="controls";
            foreach(var node in chrome) {
                if(!reader.Within() || !reader.Bound(node)) return;observation["nodes"]=(int)observation["nodes"]+1;
                int role=node.Current.ControlType.Id;if(!BrowserExtensionChromeActionPredicates.Role(role)) continue;
                string name;if(!BrowserExtensionDetailsPredicates.Read(()=>reader.Bound(node) && node.Current.ControlType.Id==role,()=>node.Current.Name,out name)) return;
                int flags=BrowserExtensionChromeActionPredicates.Label(name);
                if(flags==0) {observation["controls"]=(int)observation["controls"]+1;observation["unknown_control_count"]=(int)observation["unknown_control_count"]+1;continue;}
                Dictionary<string,object> record=null;
                if(records.Count<16) {
                    object ignored,pattern;bool invoke=node.TryGetCurrentPattern(InvokePattern.Pattern,out ignored);if(!reader.Bound(node)) return;
                    bool toggle=node.TryGetCurrentPattern(TogglePattern.Pattern,out pattern);if(!reader.Bound(node)) return;
                    int toggleState=toggle ? (int)((TogglePattern)pattern).Current.ToggleState : -1;
                    bool cut;var ancestors=reader.Ancestors(node,out cut);Native.Rect outer;if(!Native.GetWindowRect(window,out outer) || !reader.Bound(node)) return;
                    var current=node.Current;var bounds=current.BoundingRectangle;if(current.ControlType.Id!=role || !reader.Bound(node)) return;
                    record=new Dictionary<string,object>{{"role",role},{"extensions_exact",flags==1},{"extension_title_exact",flags==2},{"invoke_supported",invoke},{"toggle_supported",toggle},{"toggle_state",toggleState},
                        {"enabled",current.IsEnabled},{"offscreen",current.IsOffscreen},{"bounds_flags",BrowserExtensionPredicates.BoundsFlags(bounds.X,bounds.Y,bounds.Width,bounds.Height,outer.Left,outer.Top,outer.Right,outer.Bottom)},
                        {"ancestor_roles",ancestors},{"ancestors_truncated",cut}};
                }
                if(record!=null) records.Add(record);observation["controls"]=(int)observation["controls"]+1;observation["matching_count"]=(int)observation["matching_count"]+1;
                string field=flags==1 ? "extensions_exact_count" : "extension_title_exact_count";observation[field]=(int)observation[field]+1;observation["truncated"]=(int)observation["matching_count"]>16;
            }
            observation["stage"]="after-article";AutomationElement after;List<AutomationElement> unused;
            if(!reader.ArticleProof(document,out after,out unused,n=>observation["after_nodes"]=n)) return;
            observation["after_article_verified"]=true;observation["incomplete"]=false;observation["unavailable"]=false;observation["stage"]="complete";
        } catch {observation["property_errors"]=(int)observation["property_errors"]+1;}
        finally {if(owner!=IntPtr.Zero) Native.CloseHandle(owner);}
    }
    static DeliveryApplication deliveryApp;
    static string deliverySupport,deliveryKind,deliveryId;
    static bool deliveryPopup;
    static bool deliveryConnectAttempted,deliveryDisconnectAttempted,deliveryDisconnectInvoked,deliveryDisconnected,deliveryOrdinaryExit;
    static DateTime deliveryCleanupDeadline;
    static bool DeliveryStopped { get { return deliveryOrdinaryExit && deliveryDisconnected; } }
    static void DeliveryCheck() { if(deliveryCleanupDeadline==default(DateTime)) Check();else if(DateTime.UtcNow>=deliveryCleanupDeadline) Fail("delivery-cleanup-deadline"); }
    static void DeliveryWait(Func<bool> test,int seconds,string failure) {
        var end=DateTime.UtcNow.AddSeconds(seconds);var limit=deliveryCleanupDeadline==default(DateTime) ? deadline : deliveryCleanupDeadline;if(end>limit) end=limit;
        do { DeliveryCheck();try { if(test()) return; } catch(ElementNotAvailableException) { }Thread.Sleep(100); } while(DateTime.UtcNow<end);
        Fail(failure);
    }
    static void InitializeDeliveryReport(bool popup) {
        deliveryPopup=popup;Report["delivery_trigger"]=popup ? "popup" : "shortcut";Report["delivery_shortcut_events"]=0;
        Report["test"]="windows-stable-extension-reference-delivery";Report["scope"]="extension-native-reference-delivery";
        Report.Remove("native_host_executed");Report["native_host_process_start_observed"]=false;Report["native_host_route_verified"]=false;
        Report["app_launched"]=false;Report["inference_action_invoked"]=false;Report["bridge_token_read"]=false;
        Report["native_window_pixels_verified"]=false;Report["selected_text_capture_verified"]=false;Report["visible_url_verified"]=false;
        Report["extension_loaded"]=false;Report["extension_card_identity_verified"]=false;
        Report["extension_details_identity_verified"]=false;Report["owned_extension_folder_verified"]=false;
        Report["extension_name_found"]=false;Report["extension_version_found"]=false;
        Report["browser_launch_attempted"]=false;Report["browser_not_started_cleanup_verified"]=false;
        Report["connect_attempted"]=false;Report["disconnect_attempted"]=false;
        Report["connect_notice_observed"]=false;Report["disconnect_notice_observed"]=false;Report["disconnect_no_match_notice_observed"]=false;
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
        deliveryConnectAttempted=true;Report["connect_attempted"]=true;
        deliveryApp.Invoke(kind=="edge" ? "connect-edge" : "connect-chrome");
        DeliveryWait(()=>VerifyRegistration(kind,id),10,"delivery-connect-unavailable");
        Report["connect_notice_observed"]=ObserveDeliveryNotice("Browser host connected. Enable browser references and use the extension's Use this page action.");
        Report["native_host_registered"]=true;Report["owned_registration_verified"]=true;
        stage="delivery-article";
        NavigateArticle(window);
        if(!deliveryPopup) {stage="delivery-shortcut";Native.BrowserSendPage(window);Report["delivery_shortcut_events"]=6;}
        stage="delivery-context";
        // One context deadline. Popup discovery/actions share it; no replay of
        // either delivery trigger and no renewed wait after opening the popup.
        DateTime contextEnd=DateTime.UtcNow.AddSeconds(15);if(contextEnd>deadline) contextEnd=deadline;
        using(var popup=deliveryPopup ? new ChromePopupSession(window,id,contextEnd) : null) {
            try {
                if(popup!=null) popup.Send();
                deliveryApp.RevealReaderContext(contextEnd,popup==null ? (Action)null : popup.PollStatus);
                // Ordinary disclosure only; retain the existing separate 10s
                // full-reference proof. Status never substitutes for admission.
                deliveryApp.Invoke("preview-full-context");Report["context_preview_revealed"]=true;
                DeliveryWait(()=> {
                    if(popup!=null) popup.PollStatus();
                    var context=deliveryApp.Find("session-context");
                    return context!=null && !context.Current.IsOffscreen && BrowserExtensionDeliveryPredicates.Article(deliveryApp.Texts(context,true));
                },10,"delivery-context-unavailable");
                if(deliveryApp.Find("answer")!=null || deliveryApp.Find("answer-draft")!=null || deliveryApp.Find("consent-continue")!=null) Fail("delivery-unexpected-reading");
            } finally {if(popup!=null) popup.PollStatus();}
        }
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
        if(!deliveryConnectAttempted) { deliveryDisconnected=true;return; }
        if(deliveryDisconnected) return;
        // At most one ordinary Disconnect, including recovery after an attempted
        // Connect whose acknowledgement/proof was never obtained.
        if(BrowserExtensionDeliveryPredicates.ShouldInvokeDisconnect(deliveryConnectAttempted,deliveryDisconnectAttempted)) {
            deliveryApp.Invoke("open-settings");deliveryApp.SetField("extension-id",deliveryId);
            deliveryDisconnectAttempted=true;Report["disconnect_attempted"]=true;
            deliveryApp.Invoke(deliveryKind=="edge" ? "disconnect-edge" : "disconnect-chrome");deliveryDisconnectInvoked=true;
        }
        if(!deliveryDisconnectInvoked) Fail("delivery-disconnect-unavailable");
        DeliveryWait(()=>DeliveryHostAbsent(),10,"delivery-disconnect-unavailable");
        Report["disconnect_notice_observed"]=ObserveDeliveryNotice("Pickle's browser host was disconnected. Existing saved answers are kept.");
        Report["disconnect_no_match_notice_observed"]=ObserveDeliveryNotice("No matching browser host is installed.");
        // This is provisional until the parent proves absence again after the
        // real app exits/drains workers and its lease is idle. Connect and
        // Disconnect are independent async jobs; no-match is not completion.
        deliveryDisconnected=true;Report["ordinary_disconnect_verified"]=true;
    }
    static bool ObserveDeliveryNotice(string expected) {
        try {DeliveryCheck();return deliveryApp.HasText(expected);} catch {return false;}
    }
    static bool DeliveryEntryExists(string path,bool directory) {
        FileAttributes attributes;
        try {attributes=File.GetAttributes(path);}
        catch(FileNotFoundException) {return false;}
        catch(DirectoryNotFoundException) {return false;}
        if((attributes&FileAttributes.ReparsePoint)!=0 || ((attributes&FileAttributes.Directory)!=0)!=directory) Fail("delivery-host-path-unsafe");
        return true;
    }
    static bool DeliveryHostAbsent() {
        DeliveryCheck();DirectoryPath(deliverySupport);
        string native=Path.Combine(deliverySupport,"NativeHost");
        bool nativeExists=DeliveryEntryExists(native,true);
        if(nativeExists) foreach(string child in Directory.EnumerateFileSystemEntries(native)) {
            DeliveryCheck();if(Path.GetFileName(child)!=deliveryKind) Fail("delivery-host-files-remain");DeliveryEntryExists(child,true);
        }
        string directory=Path.Combine(deliverySupport,"NativeHost",deliveryKind);
        bool exists=nativeExists && DeliveryEntryExists(directory,true),remaining=false;
        if(exists) foreach(string path in Directory.EnumerateFileSystemEntries(directory)) {
            DeliveryCheck();string leaf=Path.GetFileName(path);
            if(leaf!="registration.json" && leaf!="com.pickle.reader.json" && leaf!="pickle-browser-host.exe") Fail("delivery-host-files-remain");
            DeliveryEntryExists(path,false);remaining=true;
        }
        foreach(int view in new[]{32,64}) if(RegistryManifest(deliveryKind,view)!=null) remaining=true;
        return !remaining;
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
    static bool VerifyRegistration(string kind,string id) {
        DeliveryCheck();DirectoryPath(deliverySupport);
        string directory=Path.Combine(deliverySupport,"NativeHost",kind),host=Path.Combine(directory,"pickle-browser-host.exe"),manifest=Path.Combine(directory,"com.pickle.reader.json"),origin="chrome-extension://"+id+"/";
        bool exists=DeliveryEntryExists(Path.Combine(deliverySupport,"NativeHost"),true) && DeliveryEntryExists(directory,true);
        bool hasHost=exists && DeliveryEntryExists(host,false),hasManifest=exists && DeliveryEntryExists(manifest,false);
        string registrationPath=Path.Combine(directory,"registration.json");bool hasRegistration=exists && DeliveryEntryExists(registrationPath,false);
        // Only absent not-yet-published components are pending. Any present
        // wrong schema/origin/path/hash/registry or unreadable data fails now.
        if(hasManifest) {
            var value=ReadObject(manifest,16384);if(!BrowserExtensionDeliveryPredicates.HostShape(value,origin) || !String.Equals(Canonical((string)value["path"]),Canonical(host),StringComparison.OrdinalIgnoreCase)) Fail("delivery-host-manifest-mismatch");
        }
        if(hasRegistration) {
            var registration=ReadObject(registrationPath,16384);
            string app=registration.ContainsKey("application") ? registration["application"] as string : null,bridge=registration.ContainsKey("bridge_directory") ? registration["bridge_directory"] as string : null;
            if(registration.Count!=4 || !registration.ContainsKey("browser") || registration["browser"] as string!=(kind=="edge" ? "Edge" : "Chrome") ||
                !registration.ContainsKey("allowed_origin") || registration["allowed_origin"] as string!=origin || app==null || bridge==null ||
                !String.Equals(Canonical(app),deliveryApp.Executable,StringComparison.OrdinalIgnoreCase) || !String.Equals(Canonical(bridge),Canonical(Path.Combine(deliverySupport,"BrowserBridge")),StringComparison.OrdinalIgnoreCase)) Fail("delivery-host-registration-mismatch");
        }
        if(hasHost && HashFile(host)!=HashFile(Path.Combine(Path.GetDirectoryName(deliveryApp.InputExecutable),"pickle-browser-host.exe"))) Fail("delivery-host-provenance-unavailable");
        int found=0;foreach(int view in new[]{32,64}) { string path=RegistryManifest(kind,view);if(path==null) continue;if(!String.Equals(Canonical(path),Canonical(manifest),StringComparison.OrdinalIgnoreCase)) Fail("delivery-host-registry-mismatch");found++; }
        return hasHost && hasManifest && hasRegistration && found>0;
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
        void Until(DateTime? end) {DeliveryCheck();if(end.HasValue && DateTime.UtcNow>=end.Value) Fail("delivery-context-unavailable");}
        bool Bound(AutomationElement node,bool passwordMetadata=false,DateTime? end=null) {
            Until(end);
            if(node==null || !OwnsWindow(window)) return false;var root=AutomationElement.FromHandle(window);
            for(int depth=0;node!=null && depth<64;depth++) { Until(end);if(node.Current.ProcessId!=process.Id || (!passwordMetadata && node.Current.IsPassword)) return false;if(Automation.Compare(node,root)) return true;node=TreeWalker.RawViewWalker.GetParent(node); }return false;
        }
        List<AutomationElement> Nodes(AutomationElement root,DateTime? end=null) {
            var queue=new Queue<AutomationElement>();var result=new List<AutomationElement>();queue.Enqueue(root);
            while(queue.Count>0) {
                Until(end);if(result.Count+queue.Count>1024) Fail("delivery-app-tree-bound");var node=queue.Dequeue();if(!Bound(node,true,end)) Fail("delivery-app-element-not-owned");
                // Credentials are not part of this scenario. Prune the exact
                // owned password branch before strings, values or children.
                if(node.Current.IsPassword) continue;result.Add(node);
                var child=TreeWalker.RawViewWalker.GetFirstChild(node);while(child!=null) { Until(end);if(result.Count+queue.Count>=1024) Fail("delivery-app-tree-bound");queue.Enqueue(child);child=TreeWalker.RawViewWalker.GetNextSibling(child); }
            }return result;
        }
        public AutomationElement Find(string id,DateTime? end=null) {
            Until(end);
            if(window==IntPtr.Zero && Window()==IntPtr.Zero) return null;
            var matches=Nodes(AutomationElement.FromHandle(window),end).Where(n=>Bound(n,false,end) && n.Current.AutomationId==id).ToArray();if(matches.Length>1) Fail("delivery-app-control-ambiguous");return matches.SingleOrDefault();
        }
        public IEnumerable<string> Texts(AutomationElement root,bool visibleOnly=false) {
            foreach(var node in Nodes(root)) { if(!Bound(node)) Fail("delivery-app-element-not-owned");if(node.Current.ControlType!=ControlType.Text) continue;
                if(visibleOnly && node.Current.IsOffscreen) continue;
                if(!Bound(node)) Fail("delivery-app-element-not-owned");string name=node.Current.Name;if(name.Length>4096) Fail("delivery-app-label-bound");yield return name;
            }
        }
        public bool HasText(string expected) { return Texts(AutomationElement.FromHandle(window)).Any(value=>value==expected); }
        bool InsideReader(AutomationElement node,AutomationElement reader,DateTime end) {
            if(DateTime.UtcNow>=end || !Bound(node,false,end) || !Bound(reader,false,end)) return false;
            for(int depth=0;node!=null && depth<64;depth++) {
                if(DateTime.UtcNow>=end || !Bound(node,false,end)) return false;
                if(Automation.Compare(node,reader)) return true;
                node=TreeWalker.RawViewWalker.GetParent(node);
            }
            return false;
        }
        bool ReaderLabel(AutomationElement reader,DateTime end) {
            int found=0;
            foreach(var node in Nodes(reader,end)) {
                if(DateTime.UtcNow>=end) return false;
                if(node.Current.ControlType!=ControlType.Text) continue;
                if(!InsideReader(node,reader,end)) return false;string name=node.Current.Name;
                if(!InsideReader(node,reader,end)) return false;
                if(name=="Browser reference") found++;
            }
            return found==1;
        }
        bool ReaderScope(AutomationElement reader,DateTime end) {
            if(DateTime.UtcNow>=end || !Bound(reader,false,end)) return false;
            var current=Find("reader-scroll",end);return current!=null && Automation.Compare(current,reader) && Bound(current,false,end);
        }
        bool FocusedReader(AutomationElement control,AutomationElement reader,DateTime end) {
            if(!ReaderScope(reader,end) || Native.GetForegroundWindow()!=window || !InsideReader(control,reader,end)) return false;
            if(control.Current.ControlType!=ControlType.Button || !control.Current.IsEnabled || !control.Current.IsKeyboardFocusable) return false;
            var focused=AutomationElement.FocusedElement;
            return focused!=null && Automation.Compare(focused,control) && InsideReader(focused,reader,end) && Native.GetForegroundWindow()==window;
        }
        public void RevealReaderContext(DateTime end,Action observe=null) {
            var observation=new Dictionary<string,object>{{"stage","route"},{"reader_observed",false},{"source_label_observed",false},
                {"control_verified",false},{"focused_control_verified",false},{"home_events",0},{"page_down_events",0},
                {"context_visible",false},{"preview_visible",false},{"scope_verified",false},{"polls",0},{"unavailable",false}};
            Report["reader_reveal"]=observation;
            AutomationElement reader=null,control=null;
            try {
                while(DateTime.UtcNow<end && (int)observation["polls"]<150) {
                    DeliveryCheck();if(observe!=null) observe();observation["polls"]=(int)observation["polls"]+1;
                    Verify();if(!OwnsWindow(window)) Fail("delivery-app-owner-changed");
                    if(reader==null) {
                        reader=Find("reader-scroll",end);
                        if(reader==null) {Thread.Sleep(100);continue;}
                        observation["reader_observed"]=true;
                        if(!ReaderLabel(reader,end)) {reader=null;Thread.Sleep(100);continue;}
                        observation["source_label_observed"]=true;
                    }
                    if(!ReaderScope(reader,end)) Fail("delivery-app-element-not-owned");
                    var context=Find("session-context",end);var preview=Find("preview-full-context",end);
                    bool contextVisible=context!=null && InsideReader(context,reader,end) && !context.Current.IsOffscreen;
                    bool previewVisible=preview!=null && InsideReader(preview,reader,end) && !preview.Current.IsOffscreen && preview.Current.IsEnabled;
                    observation["context_visible"]=contextVisible;observation["preview_visible"]=previewVisible;
                    if(contextVisible && previewVisible && ReaderScope(reader,end)) {
                        observation["scope_verified"]=true;observation["stage"]="complete";return;
                    }
                    observation["stage"]="control";
                    if(control==null) {
                        foreach(string id in new[]{"simplify","explain-term-submit"}) {
                            var candidate=Find(id,end);if(candidate==null || !InsideReader(candidate,reader,end)) continue;
                            Native.Rect outer;if(!Native.GetWindowRect(window,out outer)) Fail("delivery-app-owner-changed");
                            var bounds=candidate.Current.BoundingRectangle;
                            bool insideBounds=BrowserExtensionPredicates.BoundsFlags(bounds.X,bounds.Y,bounds.Width,bounds.Height,outer.Left,outer.Top,outer.Right,outer.Bottom)==3;
                            if(!BrowserExtensionDeliveryPredicates.ReaderFocusCandidate(id,candidate.Current.ControlType.Id,InsideReader(candidate,reader,end),!candidate.Current.IsOffscreen,candidate.Current.IsEnabled,candidate.Current.IsKeyboardFocusable,insideBounds)) continue;
                            control=candidate;observation["control_verified"]=true;break;
                        }
                        if(control==null) {Thread.Sleep(100);continue;}
                        if(!ReaderScope(reader,end) || !InsideReader(control,reader,end)) Fail("delivery-app-element-not-owned");
                        Native.SetForegroundWindow(window);
                        if(Native.GetForegroundWindow()!=window || !InsideReader(control,reader,end)) Fail("delivery-app-focus-unavailable");
                        control.SetFocus();
                    }
                    if(!FocusedReader(control,reader,end)) {Thread.Sleep(100);continue;}
                    observation["focused_control_verified"]=true;observation["stage"]="scroll";
                    // Retain real focus in this scroll ancestor while paging;
                    // focus may legitimately become offscreen after PageDown.
                    if((int)observation["home_events"]==0) {
                        if(!FocusedReader(control,reader,end)) Fail("delivery-input-target-changed");if(DateTime.UtcNow>=end) Fail("delivery-context-unavailable");
                        Native.DeliveryKey(window,OwnsWindow,0x24,false,true);observation["home_events"]=2;
                    } else if((int)observation["page_down_events"]<12) {
                        if(!FocusedReader(control,reader,end)) Fail("delivery-input-target-changed");if(DateTime.UtcNow>=end) Fail("delivery-context-unavailable");
                        Native.DeliveryKey(window,OwnsWindow,0x22,false,true);observation["page_down_events"]=(int)observation["page_down_events"]+2;
                    }
                    Thread.Sleep(100);
                }
                Fail("delivery-context-unavailable");
            } catch {observation["unavailable"]=true;throw;}
        }
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
