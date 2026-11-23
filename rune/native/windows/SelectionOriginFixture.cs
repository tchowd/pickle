// Owned synthetic provider for source-window selection-origin guard tests.
// C#5/.NET Framework references: System, System.Core, System.Windows.Forms,
// WindowsBase, UIAutomationClient, UIAutomationTypes, UIAutomationProvider.
using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Provider;
using System.Windows.Automation.Text;
using System.Windows.Forms;

public sealed class OriginForm : Form {
    public const int Command=0x8000+0x142;
    public readonly int Number, Pid=Process.GetCurrentProcess().Id;
    public readonly RootNode Root;
    public readonly OriginNode VirtualLayout, Selected;
    public volatile bool Protected;
    public int Patterns, Selections, TextReads, DocumentRanges;
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr SetFocus(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    public OriginForm(int number) {
        Number=number;Text="Pickle owned selection window "+number.ToString(CultureInfo.InvariantCulture);
        Width=420;Height=200;Left=80+number*60;Top=80+number*60;
        Root=new RootNode(this);VirtualLayout=new OriginNode(this,2,ControlType.Pane);
        Selected=new OriginNode(this,3,ControlType.Edit);
        Root.Child=VirtualLayout;VirtualLayout.Parent=Root;VirtualLayout.Child=Selected;Selected.Parent=VirtualLayout;
    }
    public void FocusOwned() { SetForegroundWindow(Handle);SetFocus(Handle); }
    public void RecordPattern() { Interlocked.Increment(ref Patterns); }
    public void RecordSelection() { Interlocked.Increment(ref Selections); }
    public void RecordText() { Interlocked.Increment(ref TextReads); }
    public void RecordDocument() { Interlocked.Increment(ref DocumentRanges); }
    protected override void WndProc(ref Message message) {
        if (message.Msg==0x003D && message.LParam.ToInt64()==AutomationInteropProvider.RootObjectId) {
            message.Result=AutomationInteropProvider.ReturnRawElementProvider(Handle,message.WParam,message.LParam,Root);
            return;
        }
        if (message.Msg==Command) {
            switch(message.WParam.ToInt32()) {
                case 1:FocusOwned();message.Result=new IntPtr(1);return;
                case 2:
                    int patterns=Interlocked.CompareExchange(ref Patterns,0,0),selections=Interlocked.CompareExchange(ref Selections,0,0),
                        texts=Interlocked.CompareExchange(ref TextReads,0,0),documents=Interlocked.CompareExchange(ref DocumentRanges,0,0);
                    if (patterns<0 || patterns>255 || selections<0 || selections>255 || texts<0 || texts>255 || documents<0 || documents>255)
                        message.Result=new IntPtr(-1);
                    else message.Result=new IntPtr(unchecked(patterns|(selections<<8)|(texts<<16)|(documents<<24)));
                    return;
                case 3:Protected=true;message.Result=new IntPtr(1);return;
            }
        }
        base.WndProc(ref message);
    }
}

[ComVisible(true),ClassInterface(ClassInterfaceType.None)]
public class OriginNode : IRawElementProviderSimple,IRawElementProviderFragment,ITextProvider {
    public readonly OriginForm Owner;
    public readonly int Id;
    public readonly ControlType Type;
    public OriginNode Parent,Child;
    public OriginNode(OriginForm owner,int id,ControlType type) { Owner=owner;Id=id;Type=type; }
    bool IsText { get { return Object.ReferenceEquals(this,Owner.Selected); } }
    public ProviderOptions ProviderOptions { get { return ProviderOptions.ServerSideProvider; } }
    public virtual IRawElementProviderSimple HostRawElementProvider { get { return null; } }
    public object GetPatternProvider(int id) {
        if (id==TextPatternIdentifiers.Pattern.Id && IsText) { Owner.RecordPattern();return this; }
        return null;
    }
    public object GetPropertyValue(int id) {
        if (id==AutomationElementIdentifiers.ProcessIdProperty.Id) return Owner.Pid;
        if (id==AutomationElementIdentifiers.ControlTypeProperty.Id) return Type.Id;
        if (id==AutomationElementIdentifiers.AutomationIdProperty.Id) return "owned-origin-"+Id.ToString(CultureInfo.InvariantCulture);
        if (id==AutomationElementIdentifiers.NameProperty.Id) return "Owned selection control";
        if (id==AutomationElementIdentifiers.NativeWindowHandleProperty.Id) return Parent==null ? unchecked((int)Owner.Handle.ToInt64()) : 0;
        if (id==AutomationElementIdentifiers.IsTextPatternAvailableProperty.Id) return IsText;
        if (id==AutomationElementIdentifiers.IsPasswordProperty.Id) return Owner.Protected && Object.ReferenceEquals(this,Owner.VirtualLayout);
        if (id==AutomationElementIdentifiers.IsControlElementProperty.Id || id==AutomationElementIdentifiers.IsContentElementProperty.Id ||
            id==AutomationElementIdentifiers.IsEnabledProperty.Id || id==AutomationElementIdentifiers.IsKeyboardFocusableProperty.Id) return true;
        if (id==AutomationElementIdentifiers.IsOffscreenProperty.Id) return false;
        if (id==AutomationElementIdentifiers.HasKeyboardFocusProperty.Id) return IsText && OriginForm.GetForegroundWindow()==Owner.Handle;
        return null;
    }
    public IRawElementProviderFragment Navigate(NavigateDirection direction) {
        if (direction==NavigateDirection.Parent) return Parent;
        if (direction==NavigateDirection.FirstChild || direction==NavigateDirection.LastChild) return Child;
        return null;
    }
    public virtual int[] GetRuntimeId() { return new[]{AutomationInteropProvider.AppendRuntimeId,Owner.Pid,Owner.Number,Id}; }
    public Rect BoundingRectangle { get { return new Rect(120,120,300,80); } }
    public IRawElementProviderSimple[] GetEmbeddedFragmentRoots() { return null; }
    public void SetFocus() { Owner.FocusOwned(); }
    public IRawElementProviderFragmentRoot FragmentRoot { get { return Owner.Root; } }
    public ITextRangeProvider[] GetSelection() { Owner.RecordSelection();return new ITextRangeProvider[]{new OriginRange(this)}; }
    public ITextRangeProvider[] GetVisibleRanges() { return new ITextRangeProvider[0]; }
    public ITextRangeProvider RangeFromChild(IRawElementProviderSimple child) { throw new NotSupportedException(); }
    public ITextRangeProvider RangeFromPoint(Point point) { throw new NotSupportedException(); }
    public ITextRangeProvider DocumentRange { get { Owner.RecordDocument();return new OriginRange(this); } }
    public SupportedTextSelection SupportedTextSelection { get { return SupportedTextSelection.Single; } }
}

[ComVisible(true),ClassInterface(ClassInterfaceType.None)]
public sealed class RootNode : OriginNode,IRawElementProviderFragmentRoot {
    public RootNode(OriginForm owner) : base(owner,1,ControlType.Window) {}
    public override IRawElementProviderSimple HostRawElementProvider { get { return AutomationInteropProvider.HostProviderFromHandle(Owner.Handle); } }
    public override int[] GetRuntimeId() { return null; }
    public IRawElementProviderFragment ElementProviderFromPoint(double x,double y) { return Owner.Selected; }
    public IRawElementProviderFragment GetFocus() { return Owner.Selected; }
}

[ComVisible(true),ClassInterface(ClassInterfaceType.None)]
public sealed class OriginRange : ITextRangeProvider {
    readonly OriginNode node;
    public OriginRange(OriginNode value) { node=value; }
    public ITextRangeProvider Clone() { return new OriginRange(node); }
    public bool Compare(ITextRangeProvider range) { var other=range as OriginRange;return other!=null && Object.ReferenceEquals(node,other.node); }
    public int CompareEndpoints(TextPatternRangeEndpoint endpoint,ITextRangeProvider range,TextPatternRangeEndpoint other) { return 0; }
    public void ExpandToEnclosingUnit(TextUnit unit) { throw new NotSupportedException(); }
    public ITextRangeProvider FindAttribute(int id,object value,bool backward) { throw new NotSupportedException(); }
    public ITextRangeProvider FindText(string text,bool backward,bool ignoreCase) { throw new NotSupportedException(); }
    public object GetAttributeValue(int id) { return AutomationElement.NotSupported; }
    public double[] GetBoundingRectangles() { return new double[0]; }
    public IRawElementProviderSimple GetEnclosingElement() { return node; }
    public string GetText(int maximum) {
        node.Owner.RecordText();
        string text=node.Owner.Number==1 ? "Caf\u00e9 selected only in window A." : "Window B must not be read.";
        return maximum<0 ? text : text.Substring(0,Math.Min(maximum,text.Length));
    }
    public int Move(TextUnit unit,int count) { throw new NotSupportedException(); }
    public void MoveEndpointByRange(TextPatternRangeEndpoint endpoint,ITextRangeProvider range,TextPatternRangeEndpoint other) { throw new NotSupportedException(); }
    public int MoveEndpointByUnit(TextPatternRangeEndpoint endpoint,TextUnit unit,int count) { throw new NotSupportedException(); }
    public void Select() { throw new NotSupportedException(); }
    public void AddToSelection() { throw new NotSupportedException(); }
    public void RemoveFromSelection() { throw new NotSupportedException(); }
    public void ScrollIntoView(bool top) { throw new NotSupportedException(); }
    public IRawElementProviderSimple[] GetChildren() { return new IRawElementProviderSimple[0]; }
}

public static class SelectionOriginFixture {
    [STAThread] public static int Main(string[] args) {
        try {
            if (args.Length!=1) return 2;
            Application.EnableVisualStyles();
            using(var a=new OriginForm(1)) using(var b=new OriginForm(2)) using(var timer=new System.Windows.Forms.Timer()) {
                timer.Interval=30000;timer.Tick+=(sender,eventArgs)=>a.Close();
                a.FormClosed+=(sender,eventArgs)=>b.Close();
                a.Shown+=(sender,eventArgs)=> {
                    a.FocusOwned();timer.Start();
                    int pid=a.Pid;
                    string json="{\"pid\":"+pid.ToString(CultureInfo.InvariantCulture)+
                        ",\"window_a\":"+unchecked((ulong)a.Handle.ToInt64()).ToString(CultureInfo.InvariantCulture)+
                        ",\"window_b\":"+unchecked((ulong)b.Handle.ToInt64()).ToString(CultureInfo.InvariantCulture)+"}";
                    File.WriteAllText(args[0]+".staged",json);File.Move(args[0]+".staged",args[0]);
                };
                b.Show();Application.Run(a);
            }
            return 0;
        } catch { return 3; }
    }
}
