// Synthetic UIA provider ONLY. Not a browser and never linked into Pickle.
// Framework C#5 references: System, System.Core, System.Drawing,
// System.Windows.Forms, WindowsBase, UIAutomationClient/Types/Provider.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Automation;
using System.Windows.Automation.Provider;
using System.Windows.Forms;
using WpfRect = System.Windows.Rect;

[StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
public struct FixturePropertyInfo {
    public Guid Guid;
    [MarshalAs(UnmanagedType.LPWStr)] public string Name;
    public int Type;
}
[ComImport, Guid("8609C4EC-4A1A-4D88-A357-5A66E060E1CF"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IFixtureRegistrar {
    [PreserveSig] int RegisterProperty(ref FixturePropertyInfo property, out int id);
}

public sealed class DocumentFixtureForm : Form {
    public const int CommandMessage = 0x8000 + 200;
    public const string Expected = "https://example.invalid/Path%2Fpart?q=Caf%C3%A9&b=2#route-one";
    public const string Changed = "https://example.invalid/Path%2Fpart?q=Caf%C3%A9&b=2#route-two";
    public readonly string Mode, Entered;
    public readonly int Pid = Process.GetCurrentProcess().Id;
    public readonly int WebRootProperty;
    public RootNode Root;
    public Node MainDocument, FocusedNode;
    public int DocumentReads, NestedReads, EditReads, LinkReads, ForeignChildNavigations,
        UnanchoredForeignChildNavigations, RootChildNavigations;
    private int nextId = 100;
    [DllImport("user32.dll")] static extern IntPtr SetFocus(IntPtr window);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);

    public DocumentFixtureForm(string mode, string entered) {
        Mode=mode; Entered=entered;
        Text="Pickle synthetic document provider";
        Width=600; Height=320; Left=100; Top=100;
        var registrar = (IFixtureRegistrar)Activator.CreateInstance(Type.GetTypeFromCLSID(
            new Guid("6E29FABF-9977-42D1-8D0E-CA7E61AD87E6")));
        var property = new FixturePropertyInfo {
            Guid=new Guid("C5FDC049-4EEC-4299-87BB-CA9FC718A681"),
            Name="IsWebContentRoot", Type=2 // UIAutomationType_Bool, not string/name heuristics.
        };
        int id;
        Marshal.ThrowExceptionForHR(registrar.RegisterProperty(ref property,out id));
        Marshal.FinalReleaseComObject(registrar);
        if (id<=0) throw new InvalidOperationException("property-registration");
        WebRootProperty=id;
        Root=new RootNode(this);
        BuildTree();
    }
    private Node Document(string name, bool web, string value) {
        return new Node(this,nextId++,ControlType.Document,name,web,value);
    }
    private void BuildTree() {
        if (Mode.StartsWith("native-",StringComparison.Ordinal) &&
            Mode!="native-helper" && Mode!="native-helper-owner-mismatch") {
            // Rust starts and owns the helper separately, then installs its
            // real Win32 PID/HWND through command6. No provider process spawn.
            FocusedNode=Root; return;
        }
        var edit=new Node(this,2,ControlType.Edit,"fixture-omnibox",false,Expected);
        var link=new Node(this,3,ControlType.Hyperlink,"fixture-link",false,Expected);
        edit.Add(Document("document-under-edit",true,Expected));
        link.Add(Document("document-under-link",true,Expected));
        if (Mode=="omnibox-only" || Mode=="link-only") {
            // Focused discovery must retain Edit/Hyperlink boundaries even
            // when ControlView would omit them and promote their true Doc.
            if (Mode=="omnibox-only") edit.Control=false; else link.Control=false;
            Root.Add(Mode=="omnibox-only" ? edit : link); FocusedNode=Root; return;
        }
        Root.Add(edit); Root.Add(link);
        if (Mode=="false-root") Root.Children.Clear();
        bool web = Mode!="false-root" && Mode!="decoys-only";
        MainDocument=Document("fixture-document",web,
            Mode=="wrong-value" ? Changed : Expected);
        MainDocument.Hidden=Mode=="hidden-root";
        if (Mode=="false-root") MainDocument.Control=false;
        MainDocument.Add(Document("fixture-iframe",true,
            Mode=="wrong-value" || Mode=="false-root" ? Expected : Changed));
        if (Mode=="foreign-pane") {
            // Virtual synthetic metadata only: no external process is created.
            // This unknown-owner branch must be encountered before the valid
            // same-PID main document, so an early refusal cannot read its Value.
            var pane=new Node(this,nextId++,ControlType.Pane,"foreign-pane",false,"");
            pane.PidOverride=Pid+1;
            pane.Add(Document("foreign-pane-document",true,Expected));
            Root.Add(pane);
        }
        if (Mode=="foreign-document-only") {
            Root.Children.Clear();
            MainDocument.PidOverride=Pid+1;
        }
        if (Mode=="foreign-wrapper-main") {
            var wrapper=new Node(this,nextId++,ControlType.Pane,"foreign-wrapper",false,"");
            wrapper.PidOverride=Pid+1;
            wrapper.Add(MainDocument);
            Root.Add(wrapper); FocusedNode=Root; return;
        }
        if (Mode=="foreign-root" || Mode=="native-helper-owner-mismatch") Root.PidOverride=Pid+1;
        if (Mode=="wide-root") {
            Root.Children.Clear();
            // Wider than the existing global 192 retained-node limit. Edit
            // boundaries remain in focused discovery; no Value may be read.
            for (int index=0;index<193;index++)
                Root.Add(new Node(this,nextId++,ControlType.Edit,"wide-boundary",false,""));
        }
        Root.Add(MainDocument);
        if (Mode=="multiple-roots") Root.Add(Document("fixture-second-root",true,Expected));
        FocusedNode=Root;
    }
    public string ReadValue(Node node) {
        if (Object.ReferenceEquals(node,MainDocument)) {
            Interlocked.Increment(ref DocumentReads);
            if (Mode=="slow") {
                File.WriteAllText(Entered,"owned-provider-entered");
                Thread.Sleep(6000);
            }
        } else if (node.Type==ControlType.Document) Interlocked.Increment(ref NestedReads);
        else if (node.Type==ControlType.Edit) Interlocked.Increment(ref EditReads);
        else if (node.Type==ControlType.Hyperlink) Interlocked.Increment(ref LinkReads);
        return node.Url;
    }
    public void FocusOwned() {
        FocusedNode=Mode=="password" && Root.Children.Count>0 ? Root.Children[0] : Root;
        SetForegroundWindow(Handle);
        SetFocus(Handle);
    }
    public void RecordForeignChildNavigation(bool unanchored) {
        Interlocked.Increment(ref ForeignChildNavigations);
        if (unanchored) Interlocked.Increment(ref UnanchoredForeignChildNavigations);
    }
    public void RecordRootChildNavigation() {
        Interlocked.Increment(ref RootChildNavigations);
    }
    private void InstallNativeAnchor() {
        string metadata=File.ReadAllText(Entered+".native-anchor");
        if (metadata.Length>128) throw new InvalidOperationException("anchor-metadata-size");
        string[] fields=metadata.Split('\n');
        if (fields.Length!=2) throw new InvalidOperationException("anchor-metadata-shape");
        int pid=Int32.Parse(fields[0],CultureInfo.InvariantCulture);
        ulong handle=UInt64.Parse(fields[1],CultureInfo.InvariantCulture);
        if (pid<=0 || pid==Pid || handle==0) throw new InvalidOperationException("anchor-metadata-owner");
        Root.Children.Clear();
        var pane=new Node(this,nextId++,ControlType.Pane,"owned-native-anchor",false,"");
        pane.PidOverride=pid;
        pane.HwndOverride=new IntPtr(unchecked((long)handle));
        MainDocument=Document("fixture-document",true,Expected);
        if (Mode=="native-embedded-anchor") {
            // The actual WS_CHILD helper is already exposed by the native
            // host provider. A second synthetic Pane alias would duplicate
            // that same helper document and correctly fail uniqueness.
            FocusedNode=Root; return;
        }
        if (Mode=="native-outside-anchor") {
            pane.Add(Document("outside-anchor-decoy",true,Expected));
            Root.Add(pane); Root.Add(MainDocument);
        } else if (Mode=="native-sibling-escape") {
            Root.Add(pane);
            var sibling=new Node(this,nextId++,ControlType.Pane,"unanchored-pid-sibling",false,"");
            sibling.PidOverride=pid;
            MainDocument.PidOverride=pid;
            sibling.Add(MainDocument); Root.Add(sibling);
        } else {
            if (Mode=="native-owner-mismatch") {
                pane.PidOverride=pid+1;
                if (pane.PidOverride==Pid) pane.PidOverride=pid+2;
            }
            MainDocument.PidOverride=pid;
            pane.Add(MainDocument); Root.Add(pane);
        }
        FocusedNode=Root;
    }
    protected override void WndProc(ref Message message) {
        if (message.Msg==0x003D && message.LParam.ToInt64()==AutomationInteropProvider.RootObjectId) {
            message.Result=AutomationInteropProvider.ReturnRawElementProvider(
                Handle,message.WParam,message.LParam,Root);
            return;
        }
        if (message.Msg==CommandMessage) {
            int command=message.WParam.ToInt32();
            if (command==1) MainDocument.Url=Changed;
            if (command==2) {
                var replacement=Document("fixture-document",true,Expected);
                int position=Root.Children.IndexOf(MainDocument);
                replacement.Parent=Root; Root.Children[position]=replacement;
                MainDocument=replacement;
            }
            if (command==3) {
                // Read-only fixed counters. No URL/source text is returned.
                message.Result=new IntPtr(DocumentReads + (NestedReads<<8) +
                    (EditReads<<16) + (LinkReads<<24));
                return;
            }
            if (command==4) FocusOwned();
            if (command==5) {
                message.Result=new IntPtr(ForeignChildNavigations);
                return;
            }
            if (command==6) InstallNativeAnchor();
            if (command==7) {
                message.Result=new IntPtr(UnanchoredForeignChildNavigations);
                return;
            }
            if (command==8) {
                message.Result=new IntPtr(RootChildNavigations);
                return;
            }
            message.Result=new IntPtr(1);
            return;
        }
        base.WndProc(ref message);
    }
}

[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
public class Node : IRawElementProviderSimple, IRawElementProviderFragment, IValueProvider {
    public readonly DocumentFixtureForm Owner;
    public readonly int Id;
    public readonly ControlType Type;
    public readonly string Name;
    public readonly bool Web;
    public string Url;
    public bool Hidden;
    public bool Control=true;
    public int? PidOverride;
    public IntPtr? HwndOverride;
    public Node Parent;
    public readonly List<Node> Children=new List<Node>();
    public Node(DocumentFixtureForm owner,int id,ControlType type,string name,bool web,string url) {
        Owner=owner;Id=id;Type=type;Name=name;Web=web;Url=url;
    }
    public void Add(Node child) { child.Parent=this;Children.Add(child); }
    public ProviderOptions ProviderOptions { get { return ProviderOptions.ServerSideProvider; } }
    public virtual IRawElementProviderSimple HostRawElementProvider { get { return null; } }
    public object GetPatternProvider(int patternId) {
        return patternId==ValuePatternIdentifiers.Pattern.Id && Type!=ControlType.Window ? this : null;
    }
    public object GetPropertyValue(int propertyId) {
        if (propertyId==Owner.WebRootProperty) return Web;
        if (propertyId==ValuePatternIdentifiers.ValueProperty.Id && Type!=ControlType.Window)
            return Owner.ReadValue(this);
        if (propertyId==AutomationElementIdentifiers.ControlTypeProperty.Id) return Type.Id;
        if (propertyId==AutomationElementIdentifiers.ProcessIdProperty.Id)
            return PidOverride.HasValue ? PidOverride.Value : Owner.Pid;
        if (propertyId==AutomationElementIdentifiers.NameProperty.Id) return DocumentFixtureForm.Expected;
        if (propertyId==AutomationElementIdentifiers.AutomationIdProperty.Id) return Name;
        if (propertyId==AutomationElementIdentifiers.FrameworkIdProperty.Id) return "PickleOwnedProvider";
        if (propertyId==AutomationElementIdentifiers.NativeWindowHandleProperty.Id)
            return HwndOverride.HasValue ? unchecked((int)HwndOverride.Value.ToInt64()) :
                Parent==null ? unchecked((int)Owner.Handle.ToInt64()) : 0;
        if (propertyId==AutomationElementIdentifiers.IsControlElementProperty.Id ||
            propertyId==AutomationElementIdentifiers.IsContentElementProperty.Id) return Control;
        if (propertyId==AutomationElementIdentifiers.IsEnabledProperty.Id ||
            propertyId==AutomationElementIdentifiers.IsKeyboardFocusableProperty.Id) return true;
        if (propertyId==AutomationElementIdentifiers.IsOffscreenProperty.Id) return Hidden;
        if (propertyId==AutomationElementIdentifiers.IsPasswordProperty.Id)
            return Owner.Mode=="password" && Object.ReferenceEquals(Owner.FocusedNode,this);
        if (propertyId==AutomationElementIdentifiers.HasKeyboardFocusProperty.Id)
            return Object.ReferenceEquals(Owner.FocusedNode,this);
        return null;
    }
    public IRawElementProviderFragment Navigate(NavigateDirection direction) {
        if (Object.ReferenceEquals(this,Owner.Root) &&
            (direction==NavigateDirection.FirstChild || direction==NavigateDirection.LastChild))
            Owner.RecordRootChildNavigation();
        if (PidOverride.HasValue && PidOverride.Value!=Owner.Pid &&
            (direction==NavigateDirection.FirstChild || direction==NavigateDirection.LastChild))
            Owner.RecordForeignChildNavigation(!HwndOverride.HasValue || HwndOverride.Value==IntPtr.Zero);
        if (direction==NavigateDirection.Parent) return Parent;
        if (direction==NavigateDirection.FirstChild) return Children.Count==0 ? null : Children[0];
        if (direction==NavigateDirection.LastChild) return Children.Count==0 ? null : Children[Children.Count-1];
        if (Parent==null) return null;
        int index=Parent.Children.IndexOf(this);
        if (index<0) return null;
        if (direction==NavigateDirection.NextSibling && index+1<Parent.Children.Count) return Parent.Children[index+1];
        if (direction==NavigateDirection.PreviousSibling && index>0) return Parent.Children[index-1];
        return null;
    }
    public virtual int[] GetRuntimeId() { return new[]{AutomationInteropProvider.AppendRuntimeId,Owner.Pid,Id}; }
    public WpfRect BoundingRectangle { get { return new WpfRect(120,140,450,100); } }
    public IRawElementProviderSimple[] GetEmbeddedFragmentRoots() { return null; }
    public void SetFocus() { Owner.FocusedNode=this; }
    public IRawElementProviderFragmentRoot FragmentRoot { get { return Owner.Root; } }
    public bool IsReadOnly { get { return true; } }
    public string Value { get { return Owner.ReadValue(this); } }
    public void SetValue(string value) { throw new InvalidOperationException("read-only-fixture"); }
}

[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
public sealed class RootNode : Node, IRawElementProviderFragmentRoot {
    public RootNode(DocumentFixtureForm owner) : base(owner,1,ControlType.Window,"fixture-root",false,"") {}
    public override IRawElementProviderSimple HostRawElementProvider {
        get { return AutomationInteropProvider.HostProviderFromHandle(Owner.Handle); }
    }
    public override int[] GetRuntimeId() { return null; }
    public IRawElementProviderFragment ElementProviderFromPoint(double x,double y) { return Owner.FocusedNode; }
    public IRawElementProviderFragment GetFocus() { return Owner.FocusedNode; }
}

public static class BrowserDocumentFixture {
    [STAThread] public static int Main(string[] args) {
        try {
            if (args.Length!=3) return 2;
            string mode=args[0],ready=args[1],entered=args[2];
            if (Array.IndexOf(new[]{"stable","wrong-value","false-root","decoys-only","omnibox-only","link-only","multiple-roots","hidden-root","password","slow","foreign-pane","foreign-document-only","foreign-wrapper-main","foreign-root","wide-root","native-helper","native-helper-empty","native-helper-owner-mismatch","native-embedded-anchor","native-outside-anchor","native-sibling-escape","native-owner-mismatch","native-wrong-executable"},mode)<0) return 2;
            Application.EnableVisualStyles();
            using (var form=new DocumentFixtureForm(mode,entered)) {
                var watchdog=new System.Windows.Forms.Timer();
                watchdog.Interval=30000;
                watchdog.Tick+=(sender,eventArgs)=>form.Close();
                form.Shown+=(sender,eventArgs)=> {
                    form.FocusOwned(); watchdog.Start();
                    int pid=form.Pid;
                    string json="{\"pid\":"+pid.ToString(CultureInfo.InvariantCulture)+
                        ",\"hwnd\":"+unchecked((ulong)form.Handle.ToInt64()).ToString(CultureInfo.InvariantCulture)+"}";
                    File.WriteAllText(ready+".staged",json);
                    File.Move(ready+".staged",ready);
                };
                Application.Run(form); watchdog.Dispose();
            }
            return 0;
        } catch { return 3; }
    }
}
