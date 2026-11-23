// Failure-only controls from the fresh hosted Edge internal extensions page.
// This observer never invokes, edits, resizes, navigates, or changes policy.
using System;
using System.Text.RegularExpressions;
#if !PICKLE_PREDICATES
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Windows.Automation;
#endif

public static class BrowserExtensionEdgeNavigationPredicates {
    public static bool SafeLabel(string value) {
        if(String.IsNullOrEmpty(value) || value.Length>80 || value!=value.Trim()) return false;
        foreach(char c in value)
            if(!Char.IsLetterOrDigit(c) && " .,!?()[]'\"-–—&+".IndexOf(c)<0) return false;
        // Reject bare domains too, not merely URL schemes or email separators.
        return !Regex.IsMatch(value,@"[\p{L}\p{Nd}-]+(?:\.[\p{L}\p{Nd}-]+)*\.\p{L}{2,}",RegexOptions.CultureInvariant);
    }
    public static bool ControlRole(int value) {
        return Array.IndexOf(new[]{50000,50002,50003,50005,50009,50011,50013,50019,50024,50031},value)>=0;
    }
    public static void SelfTest() {
        foreach(string good in new[]{"Menu","Open navigation","Settings and more","Expand sidebar","Developer mode","Café (2)","Version 1.0"})
            if(!SafeLabel(good)) throw new Exception("edge-safe-label-positive");
        foreach(string bad in new[]{null,""," leading","trailing ","https://example.invalid","example.invalid","www.example.com","a@b.test","C:\\owned","edge:extensions","line\nnext","a/b","a\\b","a\0b","a\tb",new string('a',81),"%2F","#private","例え.テスト"})
            if(SafeLabel(bad)) throw new Exception("edge-safe-label-refusal");
        if(!SafeLabel(new string('a',80)) || !ControlRole(50000) || !ControlRole(50011) || ControlRole(50004) ||
            ControlRole(50020) || ControlRole(50030)) throw new Exception("edge-control-scope");
    }
}

#if !PICKLE_PREDICATES
public static partial class BrowserExtensionFixtureProgram {
    static bool EdgeNavigationBound(AutomationElement node,AutomationElement document,
        AutomationElement root,IntPtr window,bool passwordLeaf) {
        bool first=true;
        for(int depth=0;node!=null && depth<32;depth++) {
            Check();if(!browser.Owns(node.Current.ProcessId) || (node.Current.IsPassword && !(first && passwordLeaf))) return false;
            if(Automation.Compare(node,document)) return LayoutElementBound(document,root,window);
            if(node.Current.ControlType==ControlType.Document && !first) return false;
            first=false;node=TreeWalker.RawViewWalker.GetParent(node);
        }return false;
    }
    static List<int> EdgeNavigationAncestors(AutomationElement node,AutomationElement document,
        AutomationElement root,IntPtr window,out bool truncated) {
        var roles=new List<int>();truncated=true;
        for(int depth=0;node!=null && depth<8;depth++) {
            if(!EdgeNavigationBound(node,document,root,window,false)) Fail("uia-element-not-owned");
            roles.Add(node.Current.ControlType.Id);
            if(Automation.Compare(node,document)) { truncated=false;break; }
            node=TreeWalker.RawViewWalker.GetParent(node);
        }return roles;
    }
    static void ObserveEdgeNavigation(IntPtr window) {
        var records=new List<Dictionary<string,object>>();
        var observation=new Dictionary<string,object> {
            {"stage","before-ownership"},{"incomplete",true},{"proof_verified",false},
            {"nodes",0},{"controls",0},{"property_errors",0},{"truncated",false},
            {"records",new object[0]},{"work_area_width",null},{"work_area_height",null},{"window_dpi",null}
        };
        Report["edge_navigation_observation"]=observation;
        IntPtr owner=IntPtr.Zero;int nodes=0,controls=0,errors=0;
        try {
            Check();uint pid;
            if(browser==null || !Report.ContainsKey("browser") || (string)Report["browser"]!="edge" ||
                !browser.OwnsWindow(window) || Native.GetWindowThreadProcessId(window,out pid)==0 || pid==0) return;
            owner=Native.OpenProcess(0x00101000,false,pid);
            if(!LayoutWindowBound(window,owner,pid)) return;
            observation["stage"]="before-document";
            AutomationElement root,document;string uri;
            if(!LayoutPage(window,owner,pid,null,null,null,out root,out document,out uri)) return;
            // Metadata only; no display/DPI context or window mutation.
            try {
                var monitor=Native.MonitorFromWindow(window,2);var info=new Native.EdgeNavigationMonitorInfo();
                info.Size=Marshal.SizeOf(typeof(Native.EdgeNavigationMonitorInfo));
                if(monitor!=IntPtr.Zero && Native.GetMonitorInfoW(monitor,ref info)) {
                    long width=(long)info.Work.Right-info.Work.Left,height=(long)info.Work.Bottom-info.Work.Top;
                    if(width>0 && width<=32768 && height>0 && height<=32768) { observation["work_area_width"]=(int)width;observation["work_area_height"]=(int)height; }
                }
                uint dpi=Native.GetDpiForWindow(window);if(dpi>=48 && dpi<=960) observation["window_dpi"]=(int)dpi;
            } catch { /* Optional native metadata is unknown, never a default. */ }
            observation["stage"]="traversal";
            var queue=new Queue<KeyValuePair<AutomationElement,int>>();queue.Enqueue(new KeyValuePair<AutomationElement,int>(document,0));
            while(queue.Count>0) {
                Check();if(nodes+queue.Count>1024 || !LayoutWindowBound(window,owner,pid)) return;
                var entry=queue.Dequeue();var node=entry.Key;nodes++;observation["nodes"]=nodes;
                if(entry.Value>16 || !EdgeNavigationBound(node,document,root,window,true)) return;
                if(node.Current.IsPassword) continue; // No label, pattern, or children.
                try {
                    int role=node.Current.ControlType.Id;
                    if(role==50030 && !Automation.Compare(node,document)) continue;
                    if(BrowserExtensionEdgeNavigationPredicates.ControlRole(role)) {
                        if(records.Count>=16) { controls++;observation["controls"]=controls; }
                        else {
                            if(node.Current.ControlType.Id!=role || !EdgeNavigationBound(node,document,root,window,false)) return;
                            string label=node.Current.Name;
                            if(!EdgeNavigationBound(node,document,root,window,false) || node.Current.ControlType.Id!=role) return;
                            if(!BrowserExtensionEdgeNavigationPredicates.SafeLabel(label)) label=null;
                            object value;bool invoke=node.TryGetCurrentPattern(InvokePattern.Pattern,out value);
                            bool toggle=node.TryGetCurrentPattern(TogglePattern.Pattern,out value);
                            if(!EdgeNavigationBound(node,document,root,window,false)) return;
                            int toggleState=toggle ? (int)((TogglePattern)value).Current.ToggleState : -1;
                            bool expand=node.TryGetCurrentPattern(ExpandCollapsePattern.Pattern,out value);
                            if(!EdgeNavigationBound(node,document,root,window,false)) return;
                            int expandState=expand ? (int)((ExpandCollapsePattern)value).Current.ExpandCollapseState : -1;
                            var current=node.Current;var bounds=current.BoundingRectangle;Native.Rect outer;
                            if(!Native.GetWindowRect(window,out outer)) return;
                            int flags=BrowserExtensionPredicates.BoundsFlags(bounds.X,bounds.Y,bounds.Width,bounds.Height,outer.Left,outer.Top,outer.Right,outer.Bottom);
                            bool ancestorsTruncated;var ancestors=EdgeNavigationAncestors(node,document,root,window,out ancestorsTruncated);
                            if(!EdgeNavigationBound(node,document,root,window,false) || ancestors.Count==0 || ancestors[0]!=role) return;
                            records.Add(new Dictionary<string,object> {{"role",role},{"label",label},{"enabled",current.IsEnabled},{"offscreen",current.IsOffscreen},
                                {"bounds_flags",flags},{"invoke_supported",invoke},{"toggle_supported",toggle},{"toggle_state",toggleState},
                                {"expand_supported",expand},{"expand_state",expandState},{"ancestor_roles",ancestors},{"ancestors_truncated",ancestorsTruncated}});
                            controls++;observation["controls"]=controls;
                        }
                    }
                    if(entry.Value==16) {
                        if(TreeWalker.RawViewWalker.GetFirstChild(node)!=null) return;
                        continue;
                    }
                    var child=TreeWalker.RawViewWalker.GetFirstChild(node);
                    while(child!=null) {
                        Check();if(nodes+queue.Count>=1024) return;
                        if(!EdgeNavigationBound(child,document,root,window,true)) return;
                        queue.Enqueue(new KeyValuePair<AutomationElement,int>(child,entry.Value+1));
                        child=TreeWalker.RawViewWalker.GetNextSibling(child);
                    }
                } catch(ElementNotAvailableException) { errors++; }
                catch(COMException) { errors++; }
                observation["property_errors"]=errors;
            }
            observation["stage"]="after-document";
            AutomationElement afterRoot,afterDocument;string afterUri;
            if(!LayoutPage(window,owner,pid,root,document,uri,out afterRoot,out afterDocument,out afterUri)) return;
            // Publish labels only after the same committed page is reverified.
            observation["records"]=records;observation["proof_verified"]=true;
            observation["truncated"]=controls>16;observation["incomplete"]=errors!=0;observation["stage"]="complete";
        } catch { /* Preserve primary action failure, with unavailable prefix. */ }
        finally {
            if(owner!=IntPtr.Zero) Native.CloseHandle(owner);
            if(!(bool)observation["proof_verified"]) { observation["controls"]=0;observation["truncated"]=false; }
        }
    }
    static partial class Native {
        [StructLayout(LayoutKind.Sequential)] public struct EdgeNavigationMonitorInfo { public int Size;public Rect Monitor,Work;public uint Flags; }
        [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr window,uint flags);
        [DllImport("user32.dll")] public static extern bool GetMonitorInfoW(IntPtr monitor,ref EdgeNavigationMonitorInfo info);
        [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    }
}
#endif
