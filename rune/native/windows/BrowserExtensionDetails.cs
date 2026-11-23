// Chromium157.0.8103.79 detail_view.html.ts / navigation_helper.ts / path_util.cc.
// An ID seen in the list is only a candidate. No card identity is inferred.
using System;
using System.Collections.Generic;
using System.Linq;
#if !PICKLE_PREDICATES
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Automation;
#endif

public static class BrowserExtensionDetailsPredicates {
    public static bool Read(Func<bool> bound,Func<string> read,out string value) {
        value=null;if(!bound()) return false;string candidate=read();
        if(candidate==null || candidate.Length>4096 || !bound()) return false;value=candidate;return true;
    }
    public static bool OwnPath(string observed,IEnumerable<string> known) {
        return observed!=null && known.Any(path=>String.Equals(observed,path,StringComparison.OrdinalIgnoreCase));
    }
    public static IEnumerable<string> Displays(string path,string home) {
        yield return path;
        string prefix=home.TrimEnd('\\')+"\\";
        if(String.Equals(path,home,StringComparison.OrdinalIgnoreCase)) yield return "~";
        else if(path.StartsWith(prefix,StringComparison.OrdinalIgnoreCase)) yield return "~\\"+path.Substring(prefix.Length);
    }
    public static bool VersionSection(IEnumerable<string> labels,string expected) {
        var values=labels.ToArray();return values.Length==2 && values.Contains("Version") && values.Contains(expected);
    }
    public static void SelfTest() {
        int reads=0;string value;bool bound=true;
        if(Read(()=>false,()=> { reads++;return "forbidden"; },out value) || reads!=0) throw new Exception("details-no-foreign-read");
        if(Read(()=>bound,()=> { reads++;bound=false;return "public"; },out value) || value!=null) throw new Exception("details-route-changed-during-read");
        if(!OwnPath(@"~\AppData\Local\Temp\owned",Displays(@"C:\Users\runner\AppData\Local\Temp\owned",@"C:\Users\runner")) ||
            OwnPath(@"~\other",Displays(@"C:\Users\runner\AppData\Local\Temp\owned",@"C:\Users\runner")) ||
            OwnPath(@"C:\outside\owned",new[]{@"C:\known\owned"})) throw new Exception("details-own-path-only");
        if(!VersionSection(new[]{"Version","0.1.0"},"0.1.0") || VersionSection(new[]{"Version","0.1.0","Other extension"},"0.1.0") || VersionSection(new[]{"Version","0.2.0"},"0.1.0")) throw new Exception("details-version-section-no-manager-mix");
    }
}

#if !PICKLE_PREDICATES
public static partial class BrowserExtensionFixtureProgram {
    sealed class DetailObservation {
        public AutomationElement Document,Name,Version,IdSection,PathSection,PathLink;
        public string Path;
    }
    sealed class KnownExtensionFolder : IDisposable {
        readonly string input,canonical,name,version;readonly string[] displays;IntPtr retained;
        public KnownExtensionFolder(string folder,string expectedName,string expectedVersion) {
            input=DirectoryPath(folder);canonical=Canonical(input);name=expectedName;version=expectedVersion;
            string home=Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),longHome=Dos(Canonical(home));
            displays=BrowserExtensionDetailsPredicates.Displays(input,home).Concat(BrowserExtensionDetailsPredicates.Displays(Dos(canonical),longHome)).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        }
        static string Dos(string path) { return path.StartsWith(@"\\?\UNC\",StringComparison.OrdinalIgnoreCase) ? @"\\"+path.Substring(8) : path.StartsWith(@"\\?\",StringComparison.Ordinal) ? path.Substring(4) : path; }
        public bool Match(string observed) {
            // Never feed a UI string to a filesystem API. Only after exact
            // lexical match to trusted-path displays do we pin/reprove input.
            if(!BrowserExtensionDetailsPredicates.OwnPath(observed,displays)) return false;
            if(retained==IntPtr.Zero) { retained=Native.CreateFileW(input,0,3,IntPtr.Zero,3,0x02000000,IntPtr.Zero);if(retained==new IntPtr(-1)) { retained=IntPtr.Zero;return false; } }
            DirectoryPath(input);var path=new StringBuilder(32768);uint length=Native.GetFinalPathNameByHandleW(retained,path,(uint)path.Capacity,0);
            if(length==0 || length>=path.Capacity || !String.Equals(path.ToString(),canonical,StringComparison.OrdinalIgnoreCase) || !String.Equals(Canonical(input),canonical,StringComparison.OrdinalIgnoreCase)) return false;
            var manifest=ReadObject(System.IO.Path.Combine(input,"manifest.json"),4096);
            return manifest.ContainsKey("name") && manifest["name"] as string==name && manifest.ContainsKey("version") && manifest["version"] as string==version;
        }
        public void Dispose() { if(retained!=IntPtr.Zero) Native.CloseHandle(retained);retained=IntPtr.Zero; }
    }
    sealed class DetailReader {
        readonly IntPtr window;readonly AutomationElement root,document;readonly string route;
        public DetailReader(IntPtr value,AutomationElement doc,string url) { window=value;root=AutomationElement.FromHandle(window);document=doc;route=url; }
        public bool Committed() {
            Check();if(!browser.OwnsWindow(window) || !Automation.Compare(root,AutomationElement.FromHandle(window)) || !DiagnosticBound(document,window)) return false;
            object value;if(!document.TryGetCurrentPattern(ValuePattern.Pattern,out value) || !DiagnosticBound(document,window)) return false;
            string current=((ValuePattern)value).Current.Value;
            return current==route && DiagnosticBound(document,window) && browser.OwnsWindow(window) && Automation.Compare(root,AutomationElement.FromHandle(window));
        }
        public bool Bound(AutomationElement node) {
            if(node==null || !Committed() || !DiagnosticBound(node,window)) return false;
            for(int depth=0;node!=null && depth<32;depth++) {
                Check();Owned(node);if(Automation.Compare(node,document)) return Committed();
                if(node.Current.ControlType==ControlType.Document) return false;node=TreeWalker.RawViewWalker.GetParent(node);
            }return false;
        }
        public bool ReadName(AutomationElement node,out string value) { return BrowserExtensionDetailsPredicates.Read(()=>Bound(node),()=>node.Current.Name,out value); }
        public bool ReadId(AutomationElement node,out string value) { return BrowserExtensionDetailsPredicates.Read(()=>Bound(node),()=>node.Current.AutomationId,out value); }
        public List<AutomationElement> Nodes(AutomationElement scope) { if(!Bound(scope)) return new List<AutomationElement>();return Tree(scope,false); }
        public bool Visible(AutomationElement node) {
            if(!Bound(node) || node.Current.IsOffscreen) return false;Native.Rect rect;if(!Native.GetWindowRect(window,out rect)) return false;
            var bounds=node.Current.BoundingRectangle;int flags=BrowserExtensionPredicates.BoundsFlags(bounds.X,bounds.Y,bounds.Width,bounds.Height,rect.Left,rect.Top,rect.Right,rect.Bottom);
            return (flags&3)==3;
        }
        public bool Reveal(AutomationElement node) {
            if(Visible(node)) return true;if(!Bound(node)) return false;object pattern;
            if(!node.TryGetCurrentPattern(ScrollItemPattern.Pattern,out pattern) || !Bound(node)) return false;
            ((ScrollItemPattern)pattern).ScrollIntoView();var end=DateTime.UtcNow.AddMilliseconds(500);if(end>deadline) end=deadline;
            do { Check();if(Visible(node)) return true;Thread.Sleep(50); } while(DateTime.UtcNow<end);return false;
        }
        public AutomationElement[] ById(string expected,ControlType role) {
            var result=new List<AutomationElement>();foreach(var node in Nodes(document)) {
                if(node.Current.ControlType!=role) continue;string id;if(!ReadId(node,out id)) return new AutomationElement[0];
                if(id==expected && Reveal(node)) { string current;if(!ReadId(node,out current)) return new AutomationElement[0];if(current==expected && node.Current.ControlType==role) result.Add(node); }
            }return result.ToArray();
        }
        public AutomationElement Document { get { return document; } }
    }
    static string CandidateExtensionId(IntPtr window,string kind) {
        var document=OwnedExtensionsDocument(window);if(document==null) return null;
        var reader=new DetailReader(window,document,kind+"://extensions/");if(!reader.Committed()) return null;
        var groups=new List<AutomationElement>();foreach(var node in reader.Nodes(document)) {
            if(node.Current.ControlType!=ControlType.Group) continue;string aid;if(!reader.ReadId(node,out aid)) return null;if(aid=="extension-id") groups.Add(node);
        }
        Report["candidate_extension_id_count"]=groups.Count;if(groups.Count!=1) return null;
        var ids=new List<string>();foreach(var node in reader.Nodes(groups[0])) {
            if(node.Current.ControlType!=ControlType.Text) continue;string label;if(!reader.ReadName(node,out label)) return null;string id=BrowserExtensionPredicates.IdFromLabel(label);if(id!=null) ids.Add(id);
        }
        var unique=ids.Distinct().ToArray();return unique.Length==1 && reader.Committed() ? unique[0] : null;
    }
    static void NavigateExtensionDetails(IntPtr window,string kind,string candidate) {
        string route=kind+"://extensions/?id="+candidate;
        if(!BrowserExtensionPredicates.ExtensionId(candidate) || !browser.OwnsWindow(window)) Fail("detail-candidate-invalid");
        Native.SetForegroundWindow(window);Wait(()=>Native.GetForegroundWindow()==window && browser.OwnsWindow(window),3,"navigation-foreground-unavailable");
        Native.AddressShortcut(window);AutomationElement address=null;Wait(()=> { address=FocusedAddress(window);return address!=null; },5,"owned-address-bar-unavailable");
        object value;if(!address.TryGetCurrentPattern(ValuePattern.Pattern,out value)) Fail("address-value-pattern-unavailable");
        ((ValuePattern)value).SetValue(route);if(((ValuePattern)value).Current.Value!=route) Fail("detail-route-unavailable");Native.Enter(window);
        Wait(()=> { var doc=OwnedExtensionsDocument(window);return doc!=null && new DetailReader(window,doc,route).Committed(); },10,"detail-route-unavailable");
    }
    static DetailObservation ObserveDetails(DetailReader reader,string candidate,string name,string version,KnownExtensionFolder folder) {
        if(!reader.Committed()) return null;var result=new DetailObservation {Document=reader.Document};
        Report["detail_identity_stage"]="name";
        var names=reader.ById("name",ControlType.Text);var matchedNames=new List<AutomationElement>();
        foreach(var node in names) { string label;if(!reader.ReadName(node,out label)) return null;if(label==name) matchedNames.Add(node); }
        if(matchedNames.Count!=1) return null;result.Name=matchedNames[0];
        Report["detail_identity_stage"]="id";var idSections=reader.ById("id-section",ControlType.Group);if(idSections.Length!=1) return null;result.IdSection=idSections[0];
        var ids=new List<string>();foreach(var node in reader.Nodes(result.IdSection)) {
            if(node.Current.ControlType!=ControlType.Text) continue;string label;if(!reader.Reveal(node) || !reader.ReadName(node,out label)) return null;string parsed=BrowserExtensionPredicates.IdFromLabel(label);if(parsed!=null) ids.Add(parsed);
        }
        if(ids.Distinct().Count()!=1 || ids[0]!=candidate) return null;
        Report["detail_identity_stage"]="version";var versionSections=new List<AutomationElement>();foreach(var node in reader.Nodes(reader.Document)) {
            if(node.Current.ControlType!=ControlType.Text) continue;string label;if(!reader.ReadName(node,out label)) return null;if(label!="Version" || !reader.Reveal(node)) continue;
            if(!reader.ReadName(node,out label) || label!="Version") return null;
            var parent=TreeWalker.RawViewWalker.GetParent(node);if(parent==null || !reader.Bound(parent) || parent.Current.ControlType!=ControlType.Group) continue;
            var children=new List<string>();var child=TreeWalker.RawViewWalker.GetFirstChild(parent);int count=0;
            while(child!=null) { if(++count>64) return null;if(child.Current.ControlType==ControlType.Text) { string text;if(!reader.Reveal(child) || !reader.ReadName(child,out text)) return null;children.Add(text); }child=TreeWalker.RawViewWalker.GetNextSibling(child); }
            if(BrowserExtensionDetailsPredicates.VersionSection(children,version)) versionSections.Add(parent);
        }
        if(versionSections.Count!=1) return null;result.Version=versionSections[0];
        Report["detail_identity_stage"]="path";var paths=reader.ById("load-path",ControlType.Group);if(paths.Length!=1) return null;result.PathSection=paths[0];
        var links=new List<AutomationElement>();foreach(var node in reader.Nodes(result.PathSection)) if(node.Current.ControlType==ControlType.Hyperlink && reader.Reveal(node)) links.Add(node);
        if(links.Count!=1) return null;string path;if(!reader.ReadName(links[0],out path) || !folder.Match(path)) return null;result.PathLink=links[0];result.Path=path;
        Report["detail_identity_stage"]="recheck";return reader.Committed() && reader.Bound(result.Name) && reader.Bound(result.IdSection) && reader.Bound(result.Version) && reader.Bound(result.PathSection) && reader.Bound(result.PathLink) ? result : null;
    }
    static string ProveLoadedExtensionDetails(IntPtr window,string kind,string extension,string name,string version) {
        Report["detail_identity_stage"]="candidate";string candidate=null;Wait(()=> { candidate=CandidateExtensionId(window,kind);return candidate!=null; },10,"detail-candidate-unavailable");
        // No loaded/identity flag is set from the candidate or list-wide labels.
        Report["detail_identity_stage"]="route";NavigateExtensionDetails(window,kind,candidate);
        using(var folder=new KnownExtensionFolder(extension,name,version)) {
            DetailObservation first=null,last=null;string route=kind+"://extensions/?id="+candidate;
            Wait(()=> { var doc=OwnedExtensionsDocument(window);if(doc==null) return false;var reader=new DetailReader(window,doc,route);
                first=ObserveDetails(reader,candidate,name,version,folder);if(first==null) return false;
                last=ObserveDetails(reader,candidate,name,version,folder);return last!=null && Automation.Compare(first.Document,last.Document) && Automation.Compare(first.Name,last.Name) &&
                    Automation.Compare(first.Version,last.Version) && Automation.Compare(first.IdSection,last.IdSection) && Automation.Compare(first.PathSection,last.PathSection) && Automation.Compare(first.PathLink,last.PathLink) &&
                    first.Path==last.Path && folder.Match(last.Path) && reader.Committed();
            },20,"detail-identity-unavailable");
        }
        Report["extension_identity_proof"]="detail-view";Report["extension_card_identity_verified"]=false;Report["extension_details_identity_verified"]=true;
        Report["detail_identity_stage"]="verified";
        Report["extension_name_found"]=true;Report["extension_version_found"]=true;Report["extension_id_candidates"]=1;Report["owned_extension_folder_verified"]=true;
        return candidate;
    }
}
#endif
