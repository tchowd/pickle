// Failure-only controls from the fresh hosted Edge internal extensions page.
// This observer never invokes, edits, resizes, navigates, or changes policy.
using System;
using System.Text.RegularExpressions;
using System.Collections.Generic;
#if !PICKLE_PREDICATES
using System.Runtime.InteropServices;
using System.Windows.Automation;
#endif

public static class BrowserExtensionEdgeNavigationPredicates {
    public const string MenuLabel="Extensions menu";
    public static bool MayInvokeMenu(int count,int role,string name,bool enabled,bool visible,int bounds,bool invoke) {
        return count==1 && role==50000 && name==MenuLabel && enabled && visible && bounds==3 && invoke;
    }
    public static bool MayRepeatAfterLoad(string kind,string failure,bool priorMenu,bool developerOn,bool attempted) {
        return kind=="edge" && failure=="load-unpacked-unavailable" && priorMenu && developerOn && !attempted;
    }
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
        BrowserExtensionEdgeIdentityPredicates.SelfTest();
        if(!MayRepeatAfterLoad("edge","load-unpacked-unavailable",true,true,false)) throw new Exception("load-menu-admission");
        foreach(string kind in new[]{"chrome",""}) if(MayRepeatAfterLoad(kind,"load-unpacked-unavailable",true,true,false)) throw new Exception("load-menu-edge-only");
        foreach(string failure in new[]{"stage-deadline","uia-element-not-owned","developer-toggle-unavailable",""}) if(MayRepeatAfterLoad("edge",failure,true,true,false)) throw new Exception("load-menu-exact-failure");
        if(MayRepeatAfterLoad("edge","load-unpacked-unavailable",false,true,false) || MayRepeatAfterLoad("edge","load-unpacked-unavailable",true,false,false) || MayRepeatAfterLoad("edge","load-unpacked-unavailable",true,true,true)) throw new Exception("load-menu-prior-on-once");
        foreach(string good in new[]{"Menu","Open navigation","Settings and more","Expand sidebar","Developer mode","Café (2)","Version 1.0"})
            if(!SafeLabel(good)) throw new Exception("edge-safe-label-positive");
        foreach(string bad in new[]{null,""," leading","trailing ","https://example.invalid","example.invalid","www.example.com","a@b.test","C:\\owned","edge:extensions","line\nnext","a/b","a\\b","a\0b","a\tb",new string('a',81),"%2F","#private","例え.テスト"})
            if(SafeLabel(bad)) throw new Exception("edge-safe-label-refusal");
        if(!SafeLabel(new string('a',80)) || !ControlRole(50000) || !ControlRole(50011) || ControlRole(50004) ||
            ControlRole(50020) || ControlRole(50030)) throw new Exception("edge-control-scope");
        if(!MayInvokeMenu(1,50000,MenuLabel,true,true,3,true)) throw new Exception("edge-menu-observed-control");
        foreach(int count in new[]{0,2,1024}) if(MayInvokeMenu(count,50000,MenuLabel,true,true,3,true)) throw new Exception("edge-menu-unique");
        foreach(string wrong in new[]{null,"Menu","extensions menu","Extensions menu ","Discover extensions and themes from Edge Add-ons"})
            if(MayInvokeMenu(1,50000,wrong,true,true,3,true)) throw new Exception("edge-menu-exact-label");
        foreach(int role in new[]{50004,50005,50011,50020,50030}) if(MayInvokeMenu(1,role,MenuLabel,true,true,3,true)) throw new Exception("edge-menu-exact-button");
        foreach(int bounds in new[]{0,1,2,4,5,6,7}) if(MayInvokeMenu(1,50000,MenuLabel,true,true,bounds,true)) throw new Exception("edge-menu-visible-bounds");
        if(MayInvokeMenu(1,50000,MenuLabel,false,true,3,true) || MayInvokeMenu(1,50000,MenuLabel,true,false,3,true) ||
            MayInvokeMenu(1,50000,MenuLabel,true,true,3,false)) throw new Exception("edge-menu-unsafe-control");
    }
}

// Equality/syntax observations only; never an installed-extension identity rule.
public static class BrowserExtensionEdgeIdentityPredicates {
    public static bool MayObserve(string kind,string stage,string failure,int candidates,bool pickerClosed) {
        return kind=="edge" && pickerClosed && candidates==0 &&
            ((stage=="loaded-extension-details" && failure=="detail-candidate-unavailable") ||
             (stage=="delivery-candidate" && failure=="loaded-extension-id-unavailable"));
    }
    public static string KnownErrorKind(string value) {
        switch(value) {
        case "Failed to load extension": return "failed-to-load";
        case "Error loading extension": return "error-loading";
        case "Manifest file is missing or unreadable": return "manifest-unreadable";
        case "Manifest is not valid JSON": return "manifest-invalid-json";
        case "Could not load manifest": return "manifest-load-failed";
        default: return null;
        }
    }
    public static void FirstException(IDictionary<string,object> observation,string phase,string kind,int hresult,string stageCode) {
        if(observation["first_error_kind"]!=null) return;
        observation["first_error_phase"]=phase;
        if(stageCode!=null) {
            observation["first_error_kind"]="stage-failure";
            observation["first_stage_failure"]=stageCode=="stage-deadline" || stageCode=="uia-element-not-owned" || stageCode=="uia-window-not-owned" ? stageCode : "other-stage-failure";
            // A deliberate fixture refusal has no failed native API HRESULT.
            observation["first_error_hresult"]=null;
        } else {
            observation["first_error_kind"]=kind;
            observation["first_error_hresult"]=hresult;
        }
    }
    public static int LabelFlags(string value) {
        if(value==null || value.Length>4096) return 0;
        int flags=(value=="Pickle — page and video context" ? 1 : 0) | (value=="0.1.0" ? 2 : 0);
        if(BrowserExtensionPredicates.ExtensionId(value)) flags|=4;
        else if(BrowserExtensionPredicates.IdFromLabel(value)!=null) flags|=8;
        if(KnownErrorKind(value)!=null) flags|=16;
        return flags;
    }
    public static void SelfTest() {
        if(!MayObserve("edge","loaded-extension-details","detail-candidate-unavailable",0,true) ||
            !MayObserve("edge","delivery-candidate","loaded-extension-id-unavailable",0,true)) throw new Exception("edge-zero-candidate-observation");
        foreach(string kind in new[]{"chrome",""}) if(MayObserve(kind,"delivery-candidate","loaded-extension-id-unavailable",0,true)) throw new Exception("edge-identity-edge-only");
        if(MayObserve("edge","delivery-candidate","loaded-extension-id-unavailable",1,true) || MayObserve("edge","delivery-candidate","loaded-extension-id-unavailable",0,false) ||
            MayObserve("edge","delivery-connect","delivery-connect-unavailable",0,true) || MayObserve("edge","loaded-extension-details","detail-identity-unavailable",0,true)) throw new Exception("edge-identity-exact-boundary");
        string[] labels={"Failed to load extension","Error loading extension","Manifest file is missing or unreadable","Manifest is not valid JSON","Could not load manifest"};
        string[] errors={"failed-to-load","error-loading","manifest-unreadable","manifest-invalid-json","manifest-load-failed"};
        for(int i=0;i<labels.Length;i++) {
            if(KnownErrorKind(labels[i])!=errors[i] || LabelFlags(labels[i])!=16 || KnownErrorKind(labels[i]+" private suffix")!=null || KnownErrorKind(labels[i].ToUpperInvariant())!=null) throw new Exception("edge-error-exact-kind");
        }
        var first=new Dictionary<string,object>{{"first_error_kind",null},{"first_error_phase",null},{"first_error_hresult",null},{"first_stage_failure",null}};
        FirstException(first,"name","element-not-available",unchecked((int)0x80040201),null);
        FirstException(first,"next-sibling","com",-1,null);
        if((string)first["first_error_kind"]!="element-not-available" || (string)first["first_error_phase"]!="name" || (int)first["first_error_hresult"]!=unchecked((int)0x80040201) || first["first_stage_failure"]!=null) throw new Exception("edge-first-exception-primary");
        foreach(string code in new[]{"stage-deadline","uia-element-not-owned","uia-window-not-owned","private unexpected code"}) {
            var refusal=new Dictionary<string,object>{{"first_error_kind",null},{"first_error_phase",null},{"first_error_hresult",null},{"first_stage_failure",null}};
            FirstException(refusal,"node-binding","other",123,code);
            if((string)refusal["first_error_kind"]!="stage-failure" || refusal["first_error_hresult"]!=null || (string)refusal["first_stage_failure"]!=(code=="private unexpected code" ? "other-stage-failure" : code)) throw new Exception("edge-deliberate-refusal-no-native-code");
        }
        const string id="abcdefghijklmnopabcdefghijklmnop";
        if(LabelFlags("Pickle — page and video context")!=1 || LabelFlags("0.1.0")!=2 || LabelFlags(id)!=4 || LabelFlags("ID: "+id)!=8 || LabelFlags("Failed to load extension")!=16) throw new Exception("edge-identity-closed-flags");
        foreach(string unknown in new[]{null,"private unrelated text","0.1.0 extra","Pickle — page and video context other","Failed to load extension: unknown detail","https://example.test/"+id,"ID: "+id+" private",new string('x',4097)})
            if(LabelFlags(unknown)!=0) throw new Exception("edge-identity-no-arbitrary-string");
    }
}

#if !PICKLE_PREDICATES
public static partial class BrowserExtensionFixtureProgram {
    static void ObserveEdgePostPickerIdentity(IntPtr window) {
        var records=new List<Dictionary<string,object>>();
        var observation=new Dictionary<string,object>{{"stage","before-ownership"},{"incomplete",true},{"proof_verified",false},
            {"nodes",0},{"property_errors",0},{"title_exact_count",0},{"version_exact_count",0},{"id_syntax_count",0},
            {"extension_id_group_count",0},{"known_error_count",0},{"unclassified_text_count",0},{"matching_count",0},{"truncated",false},{"records",records},
            {"first_error_kind",null},{"first_error_hresult",null},{"first_error_phase",null},{"first_stage_failure",null}};
        Report["edge_identity_observation"]=observation;
        IntPtr owner=IntPtr.Zero;string phase="deadline";
        try {
            Check();uint pid;phase="window-ownership";
            if(browser==null || !browser.OwnsWindow(window) || Native.GetWindowThreadProcessId(window,out pid)==0 || pid==0) return;
            phase="owner-process";owner=Native.OpenProcess(0x00101000,false,pid);
            phase="window-binding";if(!LayoutWindowBound(window,owner,pid)) return;
            observation["stage"]="before-document";AutomationElement root,document;string uri;
            // Existing bounded (1024-node) discovery proves the sole committed
            // internal page. The subsequent observer scans only that document.
            phase="document-binding";if(!LayoutPage(window,owner,pid,null,null,null,out root,out document,out uri)) return;
            var queue=new Queue<KeyValuePair<AutomationElement,int>>();queue.Enqueue(new KeyValuePair<AutomationElement,int>(document,0));
            observation["stage"]="traversal";
            while(queue.Count>0) {
                phase="deadline";Check();phase="window-binding";if((int)observation["nodes"]+queue.Count>256 || !LayoutWindowBound(window,owner,pid)) return;
                var entry=queue.Dequeue();var node=entry.Key;observation["nodes"]=(int)observation["nodes"]+1;
                phase="node-binding";if(entry.Value>16 || !EdgeNavigationBound(node,document,root,window,true)) return;
                phase="password";if(node.Current.IsPassword) continue;
                phase="control-type";int role=node.Current.ControlType.Id;
                phase="document-identity";if(role==50004 || (role==50030 && !Automation.Compare(node,document))) continue;
                Func<bool> bound=()=>{phase="node-binding";return LayoutWindowBound(window,owner,pid) && EdgeNavigationBound(node,document,root,window,false);};
                string aid;if(!BrowserExtensionDetailsPredicates.Read(bound,()=>{phase="automation-id";return node.Current.AutomationId;},out aid)) return;
                bool group=role==50026 && aid=="extension-id";int flags=0;string knownError=null;
                if(role!=50030) {
                    string label;if(!BrowserExtensionDetailsPredicates.Read(bound,()=>{phase="name";return node.Current.Name;},out label)) return;
                    flags=BrowserExtensionEdgeIdentityPredicates.LabelFlags(label);
                    knownError=BrowserExtensionEdgeIdentityPredicates.KnownErrorKind(label);
                }
                if(!bound()) return;phase="control-type";if(node.Current.ControlType.Id!=role) return;
                if(flags!=0 || group) {
                    Dictionary<string,object> record=null;
                    if(records.Count<16) {
                        phase="ancestors";bool ancestorsTruncated;var ancestors=EdgeNavigationAncestors(node,document,root,window,out ancestorsTruncated);
                        phase="window-bounds";Native.Rect outer;if(!Native.GetWindowRect(window,out outer) || !bound()) return;
                        phase="node-bounds";var bounds=node.Current.BoundingRectangle;
                        phase="node-offscreen";bool offscreen=node.Current.IsOffscreen;
                        if(!bound() || ancestors.Count==0 || ancestors[0]!=role) return;
                        record=new Dictionary<string,object>{{"role",role},{"title_exact",(flags&1)!=0},{"version_exact",(flags&2)!=0},
                            {"id_syntax",(flags&4)!=0 ? 1 : (flags&8)!=0 ? 2 : 0},{"extension_id_group",group},{"known_error_exact",(flags&16)!=0},{"known_error_kind",knownError},
                            {"offscreen",offscreen},{"bounds_flags",BrowserExtensionPredicates.BoundsFlags(bounds.X,bounds.Y,bounds.Width,bounds.Height,outer.Left,outer.Top,outer.Right,outer.Bottom)},
                            {"ancestor_roles",ancestors},{"ancestors_truncated",ancestorsTruncated}};
                    }
                    // Count only complete observations so retained-record count
                    // cannot disagree after a property failure under the cap.
                    if(record!=null) records.Add(record);
                    observation["matching_count"]=(int)observation["matching_count"]+1;
                    if((flags&1)!=0) observation["title_exact_count"]=(int)observation["title_exact_count"]+1;
                    if((flags&2)!=0) observation["version_exact_count"]=(int)observation["version_exact_count"]+1;
                    if((flags&12)!=0) observation["id_syntax_count"]=(int)observation["id_syntax_count"]+1;
                    if(group) observation["extension_id_group_count"]=(int)observation["extension_id_group_count"]+1;
                    if((flags&16)!=0) observation["known_error_count"]=(int)observation["known_error_count"]+1;
                    observation["truncated"]=(int)observation["matching_count"]>16;
                } else if(role==50020) observation["unclassified_text_count"]=(int)observation["unclassified_text_count"]+1;
                if(entry.Value>=16) return;
                phase="first-child";var child=TreeWalker.RawViewWalker.GetFirstChild(node);
                while(child!=null) {
                    phase="deadline";Check();phase="child-binding";if((int)observation["nodes"]+queue.Count>=256 || !EdgeNavigationBound(child,document,root,window,true)) return;
                    queue.Enqueue(new KeyValuePair<AutomationElement,int>(child,entry.Value+1));phase="next-sibling";child=TreeWalker.RawViewWalker.GetNextSibling(child);
                }
            }
            phase="deadline";Check();observation["stage"]="after-document";AutomationElement afterRoot,afterDocument;string afterUri;
            phase="document-recheck";if(!LayoutPage(window,owner,pid,root,document,uri,out afterRoot,out afterDocument,out afterUri)) return;
            observation["proof_verified"]=true;observation["incomplete"]=false;observation["stage"]="complete";
        } catch(Exception error) {
            observation["property_errors"]=(int)observation["property_errors"]+1;
            var refusal=error as ExtensionStageFailure;
            BrowserExtensionEdgeIdentityPredicates.FirstException(observation,phase,ErrorKind(error),error.HResult,refusal==null ? null : refusal.Code);
        }
        finally {if(owner!=IntPtr.Zero) Native.CloseHandle(owner);}
    }
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
    static List<AutomationElement> EdgeMenus(IntPtr window,IntPtr owner,uint pid,
        AutomationElement root,AutomationElement document,Dictionary<string,object> observation,bool requireDeveloper,out AutomationElement developer) {
        var found=new List<AutomationElement>();int nodes=0,developers=0;developer=null;
        observation["discovery_complete"]=false;observation["candidate_count"]=0;
        var queue=new Queue<KeyValuePair<AutomationElement,int>>();queue.Enqueue(new KeyValuePair<AutomationElement,int>(document,0));
        while(queue.Count>0) {
            Check();if(nodes+queue.Count>1024 || !LayoutWindowBound(window,owner,pid)) return null;
            var entry=queue.Dequeue();var node=entry.Key;nodes++;
            if(entry.Value>16 || !EdgeNavigationBound(node,document,root,window,true)) return null;
            if(node.Current.IsPassword) continue;
            int role=node.Current.ControlType.Id;if(role==50030 && !Automation.Compare(node,document)) continue;
            if(role==50000) {
                if(node.Current.ControlType.Id!=50000 || !EdgeNavigationBound(node,document,root,window,false)) return null;
                string name=node.Current.Name;
                if(!EdgeNavigationBound(node,document,root,window,false) || node.Current.ControlType.Id!=50000) return null;
                if(name==BrowserExtensionEdgeNavigationPredicates.MenuLabel) { found.Add(node);observation["candidate_count"]=found.Count; }
                if(requireDeveloper && name=="Developer mode") { developers++;developer=node; }
            }
            var child=TreeWalker.RawViewWalker.GetFirstChild(node);
            while(child!=null) {
                Check();if(entry.Value>=16 || nodes+queue.Count>=1024 || !EdgeNavigationBound(child,document,root,window,true)) return null;
                queue.Enqueue(new KeyValuePair<AutomationElement,int>(child,entry.Value+1));child=TreeWalker.RawViewWalker.GetNextSibling(child);
            }
        }
        observation["discovery_complete"]=true;return requireDeveloper && developers!=1 ? null : found;
    }
    static bool EdgeDeveloperOn(AutomationElement node,AutomationElement root,AutomationElement document,IntPtr window) {
        Check();if(node==null || !EdgeNavigationBound(node,document,root,window,false) || node.Current.ControlType.Id!=50000) return false;
        string name=node.Current.Name;if(!EdgeNavigationBound(node,document,root,window,false) || name!="Developer mode") return false;
        object pattern;bool supported=node.TryGetCurrentPattern(TogglePattern.Pattern,out pattern);
        if(!EdgeNavigationBound(node,document,root,window,false) || !supported) return false;
        var current=node.Current;var bounds=current.BoundingRectangle;Native.Rect outer;
        if(!Native.GetWindowRect(window,out outer) || !current.IsEnabled || current.IsOffscreen ||
            BrowserExtensionPredicates.BoundsFlags(bounds.X,bounds.Y,bounds.Width,bounds.Height,outer.Left,outer.Top,outer.Right,outer.Bottom)!=3) return false;
        if(!EdgeNavigationBound(node,document,root,window,false) || ((TogglePattern)pattern).Current.ToggleState!=ToggleState.On) return false;
        return EdgeNavigationBound(node,document,root,window,false);
    }
    static bool PriorEdgeMenuVerified() {
        object raw; if(!Report.TryGetValue("initial_developer_failure",out raw) || !Object.Equals(raw,"developer-toggle-unavailable") || !Report.TryGetValue("edge_menu_observation",out raw)) return false;
        var prior=raw as Dictionary<string,object>;if(prior==null || !prior.TryGetValue("stage",out raw) || !Object.Equals(raw,"complete")) return false;
        foreach(string key in new[]{"action_performed","safely_verified","before_verified","discovery_complete","control_verified","invoke_completed","after_verified"})
            if(!prior.TryGetValue(key,out raw) || !(raw is bool) || !(bool)raw) return false;
        return prior.TryGetValue("candidate_count",out raw) && Object.Equals(raw,1) && prior.TryGetValue("unavailable",out raw) && Object.Equals(raw,false);
    }
    static bool EdgeMenuControl(AutomationElement node,AutomationElement root,AutomationElement document,
        IntPtr window,out InvokePattern invoke) {
        invoke=null;Check();
        if(!EdgeNavigationBound(node,document,root,window,false) || node.Current.ControlType.Id!=50000) return false;
        if(!EdgeNavigationBound(node,document,root,window,false)) return false;
        string name=node.Current.Name;
        if(!EdgeNavigationBound(node,document,root,window,false) || node.Current.ControlType.Id!=50000) return false;
        object pattern;bool supported=node.TryGetCurrentPattern(InvokePattern.Pattern,out pattern);
        if(!EdgeNavigationBound(node,document,root,window,false)) return false;
        var current=node.Current;var bounds=current.BoundingRectangle;Native.Rect outer;
        if(!Native.GetWindowRect(window,out outer)) return false;
        int flags=BrowserExtensionPredicates.BoundsFlags(bounds.X,bounds.Y,bounds.Width,bounds.Height,outer.Left,outer.Top,outer.Right,outer.Bottom);
        if(!BrowserExtensionEdgeNavigationPredicates.MayInvokeMenu(1,current.ControlType.Id,name,current.IsEnabled,!current.IsOffscreen,flags,supported) ||
            !EdgeNavigationBound(node,document,root,window,false)) return false;
        invoke=(InvokePattern)pattern;return true;
    }
    // Only the observed internal-page Button is actionable. An Invoke return
    // proves no menu state. Main must repeat its original target discovery;
    // the Load-phase path only reads Developer mode On and never toggles it.
    static bool TryOpenEdgeExtensionsMenu(IntPtr window,out bool actionPerformed) {
        return TryInvokeEdgeExtensionsMenu(window,false,out actionPerformed);
    }
    static bool TryRepeatEdgeMenuAfterLoad(IntPtr window,out bool actionPerformed) {
        return TryInvokeEdgeExtensionsMenu(window,true,out actionPerformed);
    }
    static bool TryInvokeEdgeExtensionsMenu(IntPtr window,bool afterLoad,out bool actionPerformed) {
        actionPerformed=false;IntPtr owner=IntPtr.Zero;
        string key=afterLoad ? "edge_load_menu_observation" : "edge_menu_observation";
        bool prior=PriorEdgeMenuVerified();object reportedOn;
        if(afterLoad && (!Report.ContainsKey("browser") || !Report.ContainsKey("initial_load_failure") ||
            !BrowserExtensionEdgeNavigationPredicates.MayRepeatAfterLoad((string)Report["browser"],(string)Report["initial_load_failure"],prior,
                Report.TryGetValue("developer_mode_enabled",out reportedOn) && Object.Equals(reportedOn,true),Report.ContainsKey(key)))) return false;
        var observation=new Dictionary<string,object> {{"stage","before-ownership"},{"action_performed",false},{"safely_verified",false},
            {"before_verified",false},{"discovery_complete",false},{"candidate_count",0},{"control_verified",false},
            {"invoke_completed",false},{"after_verified",false},{"unavailable",false}};
        if(afterLoad) { observation["prior_menu_verified"]=prior;observation["developer_mode_on_verified"]=false; }
        Report[key]=observation;
        try {
            Check();uint pid;
            if(browser==null || !Report.ContainsKey("browser") || (string)Report["browser"]!="edge" ||
                !browser.OwnsWindow(window) || Native.GetWindowThreadProcessId(window,out pid)==0 || pid==0) return false;
            owner=Native.OpenProcess(0x00101000,false,pid);if(!LayoutWindowBound(window,owner,pid)) return false;
            observation["stage"]="before-document";AutomationElement root,document;string uri;
            if(!LayoutPage(window,owner,pid,null,null,null,out root,out document,out uri)) return false;
            observation["before_verified"]=true;observation["stage"]="discovery";
            AutomationElement developer;
            var found=EdgeMenus(window,owner,pid,root,document,observation,afterLoad,out developer);if(found==null || found.Count!=1) return false;
            if(afterLoad && !EdgeDeveloperOn(developer,root,document,window)) return false;
            var selected=found[0];InvokePattern invoke;if(!EdgeMenuControl(selected,root,document,window,out invoke)) return false;
            observation["stage"]="before-invoke";
            AutomationElement freshRoot,freshDocument;string freshUri;
            AutomationElement freshDeveloper;
            found=EdgeMenus(window,owner,pid,root,document,observation,afterLoad,out freshDeveloper);
            if(found==null || found.Count!=1 || !Automation.Compare(selected,found[0])) return false;
            if(afterLoad) {
                if(developer==null || freshDeveloper==null || !Automation.Compare(developer,freshDeveloper) || !EdgeDeveloperOn(freshDeveloper,root,document,window)) return false;
                observation["developer_mode_on_verified"]=true;
            }
            if(!LayoutPage(window,owner,pid,root,document,uri,out freshRoot,out freshDocument,out freshUri) ||
                !EdgeMenuControl(selected,root,document,window,out invoke) || !LayoutWindowBound(window,owner,pid)) return false;
            observation["control_verified"]=true;Check();
            actionPerformed=true;observation["action_performed"]=true;invoke.Invoke();observation["invoke_completed"]=true;
            observation["stage"]="after-document";
            if(!LayoutPage(window,owner,pid,root,document,uri,out freshRoot,out freshDocument,out freshUri)) return false;
            observation["after_verified"]=true;observation["safely_verified"]=true;observation["stage"]="complete";return true;
        } catch { observation["unavailable"]=true;return false; }
        finally { if(owner!=IntPtr.Zero) Native.CloseHandle(owner); }
    }
    static partial class Native {
        [StructLayout(LayoutKind.Sequential)] public struct EdgeNavigationMonitorInfo { public int Size;public Rect Monitor,Work;public uint Flags; }
        [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr window,uint flags);
        [DllImport("user32.dll")] public static extern bool GetMonitorInfoW(IntPtr monitor,ref EdgeNavigationMonitorInfo info);
        [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    }
}
#endif
