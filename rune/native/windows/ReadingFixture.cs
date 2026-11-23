// Owned Windows Server CI source/driver only. Never linked into Pickle.
// Framework csc references: System, System.Core, System.Web.Extensions,
// System.Windows.Forms, System.Drawing, WindowsBase, UIAutomationClient,
// UIAutomationTypes. /define:PICKLE_PREDICATES compiles non-UI selftests alone.
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
#if !PICKLE_PREDICATES
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Web.Script.Serialization;
using System.Windows.Automation;
using System.Windows.Forms;
#endif

public static class ReadingFixturePredicates {
    public const string PixelMarker="ORCHID";
    public static bool MarkerAbsent(IEnumerable<string> values) {
        return values!=null && values.All(value=>value!=null &&
            value.IndexOf(PixelMarker,StringComparison.OrdinalIgnoreCase)<0);
    }
    public static string ErrorKind(Exception error) {
        if (error is TypeInitializationException) return "type-initialization";
        if (error is System.DllNotFoundException) return "native-library-unavailable";
        if (error is System.ComponentModel.Win32Exception) return "win32";
        if (error is System.Runtime.InteropServices.COMException) return "com";
        if (error is UnauthorizedAccessException) return "access-denied";
        if (error is System.Security.SecurityException) return "security";
        if (error is System.IO.IOException) return "io";
        if (error is ArgumentException) return "argument";
        if (error is InvalidOperationException) return "invalid-operation";
        return "other";
    }
    public static bool Contract(string before, string selected, string after, string answer) {
        Func<string,int,bool> line = (value, max) => value != null && value.Trim().Length > 0 &&
            value.Length <= max && value.IndexOfAny(new char[] {'\r','\n','\0'}) < 0 &&
            !value.Any(c => Char.IsSurrogate(c) || Char.IsControl(c));
        return line(before,300) && line(selected,200) && line(after,300) && line(answer,300) &&
            MarkerAbsent(new[]{before,selected,after,answer}) &&
            before != after && selected.Contains("Caf\u00e9") &&
            !selected.Contains(before) && !selected.Contains(after) &&
            !before.Contains(selected) && !after.Contains(selected) &&
            !answer.Contains(before) && !answer.Contains(after);
    }
    public static bool Source(string[] texts, string selected, string before, string after) {
        return texts != null && texts.Count(s => s == selected) == 1 &&
            texts.All(s => s == selected || s == "Show full passage" || s == "Hide full passage") &&
            texts.All(s => !s.Contains(before) && !s.Contains(after));
    }
    public static string NormalizeDocument(string value) {
        return value.Replace("\r\n","\n").Replace('\r','\n').TrimEnd('\n');
    }
    public static bool SavedEntry(Dictionary<string,object> entry,string selected,string answer) {
        string[] keys={"id","recorded_at","app_name","app_id","action","question","passage","answer","quality","chart","reference"};
        if (entry==null || entry.Count!=keys.Length || keys.Any(key=>!entry.ContainsKey(key))) return false;
        foreach (string key in new[]{"id","recorded_at","app_name","app_id","action","passage","answer"})
            if (!(entry[key] is string)) return false;
        Guid id; DateTimeOffset recorded;
        return Guid.TryParseExact((string)entry["id"],"D",out id) && id!=Guid.Empty &&
            DateTimeOffset.TryParse((string)entry["recorded_at"],System.Globalization.CultureInfo.InvariantCulture,
                System.Globalization.DateTimeStyles.RoundtripKind,out recorded) &&
            (string)entry["app_name"]=="ReadingFixture" && ((string)entry["app_id"]).StartsWith("windows:",StringComparison.Ordinal) &&
            ((string)entry["app_id"]).Length>8 && ((string)entry["app_id"]).Length<=1000 &&
            (string)entry["action"]=="Simplify" && (string)entry["passage"]==selected && (string)entry["answer"]==answer &&
            new[]{"question","quality","chart","reference"}.All(key=>entry[key]==null);
    }
    public static void SelfTest() {
        const string chosen="Caf\u00e9 selected passage.";
        if (!Contract("BEFORE",chosen,"AFTER","A controlled answer.")) throw new Exception("contract");
        if (Contract("BEFORE","Cafe selection","AFTER","answer")) throw new Exception("unicode");
        if (Contract("BEFORE",chosen+" BEFORE","AFTER","answer")) throw new Exception("sentinel");
        if (Contract("BEFORE "+chosen,chosen,"AFTER","answer")) throw new Exception("ambiguous-selection");
        if (Contract("BEFORE",chosen,"AFTER","answer\nmore")) throw new Exception("multiline");
        if (Contract("BEFORE",chosen,"AFTER","ORCHID answer")) throw new Exception("marker-in-contract");
        if (!Source(new[]{chosen,"Show full passage"},chosen,"BEFORE","AFTER")) throw new Exception("source");
        foreach (string[] bad in new[]{new[]{"BEFORE "+chosen},new[]{chosen,"AFTER"},new[]{chosen,chosen},new[]{"unexpected"}})
            if (Source(bad,chosen,"BEFORE","AFTER")) throw new Exception("source-negative");
        if (NormalizeDocument("BEFORE\r\n"+chosen+"\rAFTER\r") != "BEFORE\n"+chosen+"\nAFTER")
            throw new Exception("document");
        if (ErrorKind(new InvalidOperationException("synthetic text must not become a diagnostic")) != "invalid-operation")
            throw new Exception("error-kind");
        if (!MarkerAbsent(new[]{chosen,"Controlled image region","BEFORE\nAFTER"})) throw new Exception("marker-absence");
        foreach (string exposed in new[]{"ORCHID","before orchid after","OrChId"})
            if (MarkerAbsent(new[]{chosen,exposed})) throw new Exception("marker-exposed");
        var saved=new Dictionary<string,object> {{"id","123e4567-e89b-42d3-a456-426614174000"},
            {"recorded_at","2026-11-23T12:00:00Z"},{"app_name","ReadingFixture"},{"app_id",@"windows:c:\owned\ReadingFixture.exe"},
            {"action","Simplify"},{"question",null},{"passage",chosen},{"answer","A controlled answer."},
            {"quality",null},{"chart",null},{"reference",null}};
        if (!SavedEntry(saved,chosen,"A controlled answer.")) throw new Exception("saved-entry");
        foreach (string timestamp in new[]{"2026-11-23T12:00:00.123456700+00:00","2026-11-23T12:00:00.1234567Z"}) {
            var precise=new Dictionary<string,object>(saved); precise["recorded_at"]=timestamp;
            if (!SavedEntry(precise,chosen,"A controlled answer.")) throw new Exception("saved-entry-fractional-time");
            if ((string)precise["recorded_at"]!=timestamp) throw new Exception("saved-entry-timestamp-mutated");
        }
        foreach (string key in new[]{"id","recorded_at","app_name","app_id","action","question","passage","answer","quality","chart","reference"}) {
            var bad=new Dictionary<string,object>(saved); bad[key]="unexpected";
            if (SavedEntry(bad,chosen,"A controlled answer.")) throw new Exception("saved-entry-negative");
        }
        var missing=new Dictionary<string,object>(saved); missing.Remove("quality");
        if (SavedEntry(missing,chosen,"A controlled answer.")) throw new Exception("saved-entry-missing");
    }
}

#if !PICKLE_PREDICATES
public sealed class ReadingFixtureFailure : Exception {
    public readonly string Code;
    public ReadingFixtureFailure(string code) { Code=code; }
}

public static class ReadingFixtureProgram {
    sealed class Contract {
        public string Before,Selected,After,Answer;
        public string Document { get { return Before+"\n"+Selected+"\n"+After; } }
    }
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength=32768, RecursionLimit=16 };
    static Dictionary<string,object> evidence;
    static string evidencePath;
    static string stage="arguments";
    static DateTime overall;
    const int MaxNodes=512;
    const int MarkerStatusMessage=0x8000+0x51;

    // This word exists only in pixels: never set Text, AccessibleName, a title,
    // RichEdit contents, clipboard, or an application/provider request field.
    sealed class MarkerPanel : Panel {
        public bool Painted;
        protected override void OnPaint(PaintEventArgs e) {
            base.OnPaint(e);
            e.Graphics.Clear(Color.White);
            using (var font=new Font("Segoe UI",48.0f,FontStyle.Bold,GraphicsUnit.Point)) {
                Size size=TextRenderer.MeasureText(e.Graphics,ReadingFixturePredicates.PixelMarker,font);
                if (size.Width>ClientSize.Width || size.Height>ClientSize.Height) Fail("fixture-marker-clipped");
                TextRenderer.DrawText(e.Graphics,ReadingFixturePredicates.PixelMarker,font,ClientRectangle,
                    Color.Black,Color.White,TextFormatFlags.HorizontalCenter|TextFormatFlags.VerticalCenter|TextFormatFlags.SingleLine);
            }
            e.Graphics.Flush(System.Drawing.Drawing2D.FlushIntention.Sync);
            if (!Native.GdiFlush()) Fail("fixture-marker-flush-failed");
            Painted=true;
        }
    }
    sealed class MarkerForm : Form {
        public MarkerPanel Marker;
        protected override void WndProc(ref Message message) {
            if (message.Msg==MarkerStatusMessage) {
                // A bounded boolean-only diagnostic; never disclose the word.
                message.Result=new IntPtr(Marker!=null && Marker.Painted && Marker.Visible &&
                    ClientRectangle.Contains(Marker.Bounds) ? 1 : 0);
                return;
            }
            base.WndProc(ref message);
        }
    }

    [MTAThread]
    public static int Main(string[] args) {
        bool fixtureMode=false;
        try {
            if (args.Length==1 && args[0]=="--self-test") {
                ReadingFixturePredicates.SelfTest(); Console.WriteLine("Reading fixture predicates passed; no UI."); return 0;
            }
            if (args.Length<1) Fail("invalid-arguments");
            bool fixture=args[0]=="fixture", drive=args[0]=="drive", saved=args[0]=="drive-saved";
            fixtureMode=fixture;
            if (!fixture && !drive && !saved) Fail("invalid-mode");
            string[] names=fixture ? new[]{"--contract","--ready"} :
                saved ? new[]{"--contract","--application-pid","--application-path","--application-start-ticks","--saved-entry","--evidence"} :
                new[]{"--contract","--application-pid","--fixture-pid","--fixture-hwnd","--server-state","--evidence"};
            Dictionary<string,string> options=Options(args,names);
            evidencePath=fixture ? options["--ready"]+".failure.json" : options["--evidence"];
            evidence=new Dictionary<string,object> {{"schema",1},{"test",fixture ? "windows-owned-selection-source" : saved ? "windows-server-installed-saved-reading" : "windows-server-installed-selection-reading"},
                {"passed",false},{"pid",Process.GetCurrentProcess().Id}};
            stage="contract-read";
            Contract contract=ReadContract(options["--contract"]);
            if (fixture) {
                int result=Fixture(contract,options["--ready"]); evidence["passed"]=result==0; return result;
            }
            evidence["source_length"]=contract.Selected.Length; evidence["source_sha256"]=Hash(contract.Selected);
            overall=DateTime.UtcNow.AddSeconds(75);
            if (saved) DriveSaved(contract,options); else Drive(contract,options);
            evidence["passed"]=true; evidence["stage"]="complete"; return 0;
        } catch (ReadingFixtureFailure error) {
            RecordError(error,error.Code);
            Console.Error.WriteLine("Controlled reading fixture failed: "+error.Code); return 1;
        } catch (Exception error) {
            string code=fixtureMode ? "unexpected-fixture-error" : "unexpected-driver-error";
            RecordError(error,code);
            Console.Error.WriteLine("Controlled reading fixture failed: "+code); return 1;
        } finally {
            if (evidence!=null && evidencePath!=null && (!fixtureMode || !(bool)evidence["passed"])) {
                try { AtomicJson(evidencePath,evidence); }
                catch { Console.Error.WriteLine("Controlled reading evidence could not be written."); }
            }
        }
    }
    static void RecordError(Exception error,string code) {
        if (evidence==null) return;
        evidence["stage"]=stage; evidence["failure"]=code;
        evidence["error_kind"]=ReadingFixturePredicates.ErrorKind(error);
        evidence["error_hresult"]=error.HResult;
        var native=error as System.ComponentModel.Win32Exception;
        if (native!=null) evidence["native_error_code"]=native.NativeErrorCode;
        // At most one inner exception, and only fixed classification/numeric
        // metadata. Never Message, StackTrace, ToString, paths or source text.
        if (error.InnerException!=null) {
            evidence["inner_error_kind"]=ReadingFixturePredicates.ErrorKind(error.InnerException);
            evidence["inner_error_hresult"]=error.InnerException.HResult;
        }
    }
    static void Fail(string code) { throw new ReadingFixtureFailure(code); }
    static Dictionary<string,string> Options(string[] args,string[] allowed) {
        if (args.Length!=1+allowed.Length*2) Fail("invalid-arguments");
        var values=new Dictionary<string,string>(StringComparer.Ordinal);
        for (int n=1;n<args.Length;n+=2) {
            if (!allowed.Contains(args[n]) || values.ContainsKey(args[n]) || String.IsNullOrWhiteSpace(args[n+1])) Fail("invalid-arguments");
            values.Add(args[n],args[n+1]);
        }
        return values;
    }
    static Dictionary<string,object> ReadJson(string path) {
        using (var file=new FileStream(path,FileMode.Open,FileAccess.Read,FileShare.ReadWrite|FileShare.Delete)) {
            if (file.Length<2 || file.Length>32768) { Fail("invalid-json-size"); }
            using (var reader=new StreamReader(file,new UTF8Encoding(false,true))) {
                var value=Json.DeserializeObject(reader.ReadToEnd()) as Dictionary<string,object>;
                if (value==null) Fail("invalid-json-object"); return value;
            }
        }
    }
    static string StringField(Dictionary<string,object> value,string key) {
        object field; if (!value.TryGetValue(key,out field) || !(field is string)) Fail("invalid-contract-field");
        return (string)field;
    }
    static Contract ReadContract(string path) {
        var value=ReadJson(path);
        if (value.Count!=4) Fail("invalid-contract-fields");
        var contract=new Contract { Before=StringField(value,"before"),Selected=StringField(value,"selected"),
            After=StringField(value,"after"),Answer=StringField(value,"answer") };
        if (!ReadingFixturePredicates.Contract(contract.Before,contract.Selected,contract.After,contract.Answer)) Fail("invalid-contract-text");
        return contract;
    }
    static void AtomicJson(string path,object value) {
        string full=Path.GetFullPath(path), temp=full+"."+Guid.NewGuid().ToString("N")+".tmp";
        string text=Json.Serialize(value);
        if (Encoding.UTF8.GetByteCount(text)>32768) Fail("evidence-too-large");
        try {
            using (var file=new FileStream(temp,FileMode.CreateNew,FileAccess.Write,FileShare.None)) {
                byte[] bytes=new UTF8Encoding(false).GetBytes(text); file.Write(bytes,0,bytes.Length); file.Flush(true);
            }
            if (File.Exists(full)) File.Replace(temp,full,null); else File.Move(temp,full);
        } finally { if (File.Exists(temp)) File.Delete(temp); }
    }
    static string Hash(string value) {
        using (var hash=SHA256.Create()) return BitConverter.ToString(hash.ComputeHash(Encoding.UTF8.GetBytes(value))).Replace("-","").ToLowerInvariant();
    }
    static string Hex(IntPtr handle) { return "0x"+handle.ToInt64().ToString("x",CultureInfo.InvariantCulture); }
    static int Fixture(Contract contract,string ready) {
        Exception failure=null;
        stage="fixture-sta-thread";
        var thread=new Thread(delegate() {
            try {
                stage="fixture-exception-mode";
                Application.SetUnhandledExceptionMode(UnhandledExceptionMode.ThrowException);
                stage="fixture-visual-styles";
                Application.EnableVisualStyles();
                stage="fixture-controls";
                using (var form=new MarkerForm()) using (var edit=new RichTextBox()) using (var marker=new MarkerPanel()) {
                    stage="fixture-content";
                    form.Text="Pickle controlled selection fixture"; form.Width=720; form.Height=420;
                    form.StartPosition=FormStartPosition.CenterScreen;
                    edit.Text=contract.Document; edit.HideSelection=false; edit.ReadOnly=true;
                    edit.AccessibleName="Controlled selected passage"; form.Controls.Add(edit);
                    marker.AccessibleName="Controlled image region"; marker.TabStop=false;
                    form.Marker=marker; form.Controls.Add(marker);
                    Action layout=()=> {
                        const int height=120;
                        edit.SetBounds(0,0,form.ClientSize.Width,Math.Max(1,form.ClientSize.Height-height));
                        marker.SetBounds(0,edit.Bottom,form.ClientSize.Width,height);
                    };
                    form.ClientSizeChanged+=delegate { layout(); }; layout();
                    // Restore only this fixture's edit on its own STA when the
                    // installed app's initial window has temporarily taken focus.
                    form.Activated+=delegate { if (!form.IsDisposed && !edit.IsDisposed) { form.ActiveControl=edit; edit.Focus(); } };
                    form.Shown+=delegate {
                        stage="fixture-shown";
                        form.Activate(); edit.Focus();
                        stage="fixture-selection";
                        int start=edit.Text.IndexOf(contract.Selected,StringComparison.Ordinal);
                        if (start<0) Fail("fixture-selection-unavailable");
                        edit.Select(start,contract.Selected.Length);
                        stage="fixture-marker-paint";
                        form.Update(); marker.Invalidate(); marker.Update();
                        if (!marker.Painted || !marker.Visible || !form.ClientRectangle.Contains(marker.Bounds)) Fail("fixture-marker-not-painted");
                        stage="fixture-ready-write";
                        AtomicJson(ready,new Dictionary<string,object> {
                            {"pid",Process.GetCurrentProcess().Id},{"hwnd",Hex(form.Handle)},{"edit_hwnd",Hex(edit.Handle)},
                            {"window_marker_painted",true}});
                        stage="fixture-message-loop";
                    };
                    Application.Run(form);
                }
            } catch (Exception error) { failure=error; }
        });
        thread.SetApartmentState(ApartmentState.STA); thread.Start(); thread.Join();
        if (failure!=null) throw failure; return 0;
    }
    static int Pid(string value) { int pid; if (!Int32.TryParse(value,out pid) || pid<=0) { Fail("invalid-pid"); } return pid; }
    static IntPtr Handle(string value) {
        long number; if (value.StartsWith("0x",StringComparison.OrdinalIgnoreCase)) value=value.Substring(2);
        if (!Int64.TryParse(value,NumberStyles.AllowHexSpecifier,CultureInfo.InvariantCulture,out number) || number==0) Fail("invalid-hwnd");
        return new IntPtr(number);
    }
    static DateTime Deadline(int seconds) { DateTime end=DateTime.UtcNow.AddSeconds(seconds); return end<overall ? end : overall; }
    static void Wait(Func<bool> predicate,int seconds,string failure) {
        DateTime end=Deadline(seconds);
        do {
            try { if (predicate()) return; }
            catch (ElementNotAvailableException) { /* A view committed; resolve its owned node again. */ }
            Thread.Sleep(50);
        } while (DateTime.UtcNow<end);
        Fail(failure);
    }
    static bool Owned(IntPtr hwnd,int pid) { uint actual; return hwnd!=IntPtr.Zero && Native.IsWindow(hwnd) && Native.GetWindowThreadProcessId(hwnd,out actual)!=0 && actual==(uint)pid; }
    static void Focus(IntPtr hwnd,int pid) {
        if (!Owned(hwnd,pid) || !Native.IsWindowVisible(hwnd)) Fail("focus-target-not-owned");
        Native.SetForegroundWindow(hwnd);
        Wait(()=>Owned(hwnd,pid) && Native.GetForegroundWindow()==hwnd,5,"foreground-not-owned");
    }
    static AutomationElement Root(IntPtr hwnd,int pid) {
        if (!Owned(hwnd,pid)) Fail("uia-window-not-owned");
        var root=AutomationElement.FromHandle(hwnd);
        if (root==null || root.Current.ProcessId!=pid) Fail("uia-root-not-owned"); return root;
    }
    static AutomationElement Find(AutomationElement root,string id,int pid) {
        AutomationElementCollection found=root.FindAll(TreeScope.Descendants,
            new PropertyCondition(AutomationElement.AutomationIdProperty,id));
        if (found.Count>1) Fail("duplicate-uia-id");
        if (found.Count==0) return null;
        if (found[0].Current.ProcessId!=pid) Fail("uia-element-not-owned"); return found[0];
    }
    static string[] Texts(AutomationElement container,int pid) {
        if (container==null) Fail("uia-text-container-unavailable");
        var texts=new List<string>();
        var leaves=container.FindAll(TreeScope.Descendants,
            new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Text));
        if (leaves.Count>MaxNodes) Fail("uia-text-bound-exceeded");
        for (int i=0;i<leaves.Count;i++) {
            if (leaves[i].Current.ProcessId!=pid) Fail("uia-text-not-owned");
            string name=leaves[i].Current.Name;
            if (name.Length>2048) Fail("uia-text-bound-exceeded");
            if (name.Length>0) texts.Add(name);
        }
        return texts.ToArray();
    }
    static void Invoke(AutomationElement element,int pid) {
        if (element==null || element.Current.ProcessId!=pid || !element.Current.IsEnabled) Fail("invoke-not-owned-or-enabled");
        object pattern;
        if (!element.TryGetCurrentPattern(InvokePattern.Pattern,out pattern)) Fail("invoke-pattern-unavailable");
        ((InvokePattern)pattern).Invoke();
    }
    static void Server(string path,bool complete) {
        var value=ReadJson(path); object field;
        string[] counts={"total_requests","show_count","chat_count"};
        int[] expected=complete ? new[]{2,1,1} : new[]{0,0,0};
        for (int i=0;i<counts.Length;i++) {
            if (!value.TryGetValue(counts[i],out field) || !(field is int) || (int)field!=expected[i]) Fail("unexpected-provider-request-count");
        }
        if (!value.TryGetValue("rejected",out field) || !(field is bool) || (bool)field) Fail("provider-rejected-input");
        if (!value.TryGetValue("source_verified",out field) || !(field is bool) || (bool)field!=complete) Fail("provider-source-unverified");
        if (!value.TryGetValue("context_verified",out field) || !(field is bool) || (bool)field!=complete) Fail("provider-context-unverified");
    }
    static void VerifyPixelMarker(IntPtr fixture,int pid) {
        stage="source-pixel-marker";
        if (!Owned(fixture,pid)) Fail("marker-source-not-owned");
        UIntPtr painted;
        if (Native.SendMessageTimeout(fixture,MarkerStatusMessage,UIntPtr.Zero,IntPtr.Zero,2,1000,out painted)==IntPtr.Zero ||
            painted.ToUInt64()!=1) Fail("fixture-marker-not-painted");
        evidence["window_marker_painted"]=true;
        var root=Root(fixture,pid);
        var nodes=root.FindAll(TreeScope.Subtree,Condition.TrueCondition);
        if (nodes.Count>MaxNodes) Fail("marker-uia-bound-exceeded");
        var texts=new List<string>(); int total=0;
        for (int n=0;n<nodes.Count;n++) {
            var node=nodes[n];
            if (node.Current.ProcessId!=pid) Fail("marker-uia-not-owned");
            texts.Add(node.Current.Name);
            object pattern;
            if (node.TryGetCurrentPattern(ValuePattern.Pattern,out pattern)) texts.Add(((ValuePattern)pattern).Current.Value);
            if (node.TryGetCurrentPattern(TextPattern.Pattern,out pattern)) texts.Add(((TextPattern)pattern).DocumentRange.GetText(4097));
        }
        var title=new StringBuilder(4097);
        if (Native.GetWindowText(fixture,title,title.Capacity)>=4096) Fail("marker-title-bound-exceeded");
        texts.Add(title.ToString());
        foreach (string text in texts) {
            if (text==null || text.Length>4096 || (total+=text.Length)>32768) Fail("marker-uia-text-bound-exceeded");
        }
        if (!ReadingFixturePredicates.MarkerAbsent(texts)) Fail("pixel-marker-exposed-through-accessibility");
        evidence["window_marker_absent_from_accessibility"]=true;
    }
    static void SourceOracle(Contract contract,IntPtr fixture,int fixturePid,Process source) {
        int attempts=0;
        Wait(()=> {
            var state=new Dictionary<string,object> {{"attempts",++attempts},{"status","native-foreground"},
                {"uia_checked",false},{"uia_present",false},{"uia_pid",0},{"uia_native_hwnd","0x0"},
                {"password_checked",false},{"password",false},{"text_pattern",false},
                {"selection_exact",false},{"document_exact",false}};
            evidence["source_focus"]=state;
            if (source.HasExited || !Owned(fixture,fixturePid)) Fail("source-exited-during-oracle");
            IntPtr foreground=Native.GetForegroundWindow(); uint foregroundPid;
            Native.GetWindowThreadProcessId(foreground,out foregroundPid);
            state["foreground_hwnd"]=Hex(foreground); state["foreground_pid"]=foregroundPid;
            bool foregroundOwned=foreground==fixture && Owned(foreground,fixturePid);
            state["foreground_owned"]=foregroundOwned;
            IntPtr nativeFocus;
            bool nativeFocusAvailable=Native.FocusedWindow(fixture,out nativeFocus);
            uint nativePid; Native.GetWindowThreadProcessId(nativeFocus,out nativePid);
            bool nativeEditOwned=nativeFocusAvailable && Owned(nativeFocus,fixturePid) && Native.IsChild(fixture,nativeFocus);
            state["gui_thread_available"]=nativeFocusAvailable; state["gui_focus_hwnd"]=Hex(nativeFocus);
            state["gui_focus_pid"]=nativePid; state["gui_focus_owned_child"]=nativeEditOwned;
            state["uia_checked"]=true; state["status"]="uia-query";
            var focus=AutomationElement.FocusedElement;
            state["uia_present"]=focus!=null;
            if (focus==null) { state["status"]="uia-no-element"; return false; }
            int focusedPid=focus.Current.ProcessId;
            IntPtr uiaHandle=new IntPtr(focus.Current.NativeWindowHandle);
            state["uia_pid"]=focusedPid; state["uia_native_hwnd"]=Hex(uiaHandle);
            if (!foregroundOwned) return false;
            if (!nativeEditOwned) { state["status"]="native-edit-focus"; return false; }
            if (focusedPid!=fixturePid) { state["status"]="uia-wrong-pid"; return false; }
            // A virtual UIA element may have no HWND. Its provider PID, native
            // focused edit, full document and selected range must still agree.
            if (uiaHandle!=IntPtr.Zero && (!Owned(uiaHandle,fixturePid) || !Native.IsChild(fixture,uiaHandle))) {
                state["status"]="uia-native-not-owned"; return false;
            }
            state["password_checked"]=true; bool password=focus.Current.IsPassword; state["password"]=password;
            if (password) { state["status"]="uia-protected"; return false; }
            object pattern;
            if (!focus.TryGetCurrentPattern(TextPattern.Pattern,out pattern)) { state["status"]="text-pattern-unavailable"; return false; }
            state["text_pattern"]=true; var text=(TextPattern)pattern;
            var ranges=text.GetSelection(); state["selection_count"]=ranges.Length;
            if (ranges.Length!=1) { state["status"]="selection-count"; return false; }
            bool selection=ranges[0].GetText(2048)==contract.Selected; state["selection_exact"]=selection;
            if (!selection) { state["status"]="selection-mismatch"; return false; }
            bool document=ReadingFixturePredicates.NormalizeDocument(text.DocumentRange.GetText(4096))==contract.Document;
            state["document_exact"]=document; state["status"]=document ? "ready" : "document-mismatch";
            return document;
        },5,"source-focus-readiness-timeout");
    }
    static void RecheckSource(Contract contract,IntPtr app,int appPid,Process application) {
        stage="source-recheck";
        AutomationElement root=Root(app,appPid), copy=null;
        Wait(()=> { root=Root(app,appPid); return Find(root,"reader-scroll",appPid)!=null; },5,"post-answer-reader-scroll-unavailable");
        Wait(()=> {
            root=Root(app,appPid); copy=Find(root,"copy-answer",appPid);
            return copy!=null && copy.Current.IsEnabled && copy.Current.IsKeyboardFocusable && !copy.Current.IsOffscreen;
        },5,"post-answer-focus-control-unavailable");
        // The pinned Windows adapter exposes ScrollItem but not ScrollPattern.
        // Focus (never Invoke) this ordinary reader button, then use Rune's
        // documented Home key route to its nearest scrolling ancestor.
        Focus(app,appPid); copy.SetFocus();
        Wait(()=> {
            if (application.HasExited) Fail("application-exited-before-source-recheck");
            var focused=AutomationElement.FocusedElement;
            return Owned(app,appPid) && Native.GetForegroundWindow()==app && focused!=null &&
                focused.Current.ProcessId==appPid && focused.Current.AutomationId=="copy-answer";
        },5,"post-answer-control-focus-unavailable");
        Native.Home(app,appPid);
        evidence["source_recheck_method"]="owned-button-focus-home";
        evidence["source_recheck_home_events_inserted"]=2;
        AutomationElement preview=null;
        Wait(()=> {
            if (application.HasExited) Fail("application-exited-during-source-recheck");
            root=Root(app,appPid); preview=Find(root,"source-preview",appPid);
            return preview!=null && !preview.Current.IsOffscreen;
        },5,"post-answer-source-unavailable");
        Wait(()=> {
            root=Root(app,appPid); preview=Find(root,"source-preview",appPid);
            return preview!=null && !preview.Current.IsOffscreen &&
                ReadingFixturePredicates.Source(Texts(preview,appPid),contract.Selected,contract.Before,contract.After);
        },5,"post-answer-source-mismatch");
        evidence["source_retained_verified"]=true;
    }
    static void SaveAnswer(IntPtr app,int appPid,Process application) {
        stage="save-answer";
        // Source recheck deliberately scrolled upward with the Copy button
        // focused. Ordinary End returns that same reader viewport to actions.
        Focus(app,appPid); Native.End(app,appPid);
        AutomationElement save=null;
        Wait(()=> {
            if (application.HasExited) Fail("application-exited-before-save");
            save=Find(Root(app,appPid),"save-answer",appPid);
            return save!=null && save.Current.IsEnabled && !save.Current.IsOffscreen;
        },5,"save-answer-unavailable");
        Invoke(save,appPid);
        Wait(()=> {
            if (application.HasExited) Fail("application-exited-during-save");
            return Texts(Root(app,appPid),appPid).Contains("Answer saved on this device.");
        },10,"saved-answer-acknowledgement-unavailable");
        evidence["saved_answer_verified"]=true;
    }
    static void CheckRestartIdentity(Process application,string path,string ticks) {
        long expected;
        if (!Int64.TryParse(ticks,NumberStyles.None,CultureInfo.InvariantCulture,out expected) || expected<=0)
            Fail("invalid-application-start-ticks");
        if (!Path.IsPathRooted(path) || !File.Exists(path)) Fail("invalid-application-path");
        if (application.HasExited || application.StartTime.ToUniversalTime().Ticks!=expected ||
            !String.Equals(Path.GetFullPath(application.MainModule.FileName),Path.GetFullPath(path),StringComparison.OrdinalIgnoreCase))
            Fail("saved-application-identity-mismatch");
    }
    static bool LibraryReady(AutomationElement root,int pid) {
        if (Find(root,"library-error",pid)!=null || Find(root,"library-search-error",pid)!=null)
            Fail("saved-library-error");
        return Find(root,"library-busy",pid)==null && Find(root,"library-search-pending",pid)==null;
    }
    static bool MetadataAbsent(AutomationElement root,int pid,string id) {
        return new[]{"entry-quality-","entry-request-","entry-reference-","entry-chart-kind-"}
            .All(prefix=>Find(root,prefix+id,pid)==null);
    }
    static void DriveSaved(Contract contract,Dictionary<string,string> options) {
        stage="saved-entry-read";
        var entry=ReadJson(options["--saved-entry"]);
        if (!ReadingFixturePredicates.SavedEntry(entry,contract.Selected,contract.Answer)) Fail("saved-entry-contract-mismatch");
        string id=(string)entry["id"];
        evidence["saved_entry_verified"]=true; evidence["saved_entry_id_sha256"]=Hash(id);
        evidence["answer_sha256"]=Hash(contract.Answer);
        int appPid=Pid(options["--application-pid"]);
        if (appPid==Process.GetCurrentProcess().Id) Fail("pid-scope-invalid");
        evidence["application_pid"]=appPid;
        using (var application=Process.GetProcessById(appPid)) {
            IntPtr retained=application.Handle;
            stage="saved-application-identity";
            CheckRestartIdentity(application,options["--application-path"],options["--application-start-ticks"]);
            evidence["application_identity_verified"]=true;
            IntPtr app=IntPtr.Zero; AutomationElement root=null;
            Wait(()=> {
                if (application.HasExited) Fail("application-exited-before-saved-ready");
                app=Native.ApplicationWindow(appPid);
                if (app==IntPtr.Zero) return false;
                root=Root(app,appPid);
                return Find(root,"open-saved",appPid)!=null;
            },15,"saved-reader-window-unavailable");
            stage="open-saved";
            Invoke(Find(root,"open-saved",appPid),appPid);
            AutomationElement read=null;
            Wait(()=> {
                root=Root(app,appPid);
                if (!LibraryReady(root,appPid)) return false;
                read=Find(root,"read-"+id,appPid);
                return read!=null && read.Current.IsEnabled;
            },10,"saved-entry-card-unavailable");
            if (!MetadataAbsent(root,appPid,id)) Fail("unexpected-saved-card-metadata");
            stage="saved-detail";
            Invoke(read,appPid);
            Wait(()=> {
                if (application.HasExited) Fail("application-exited-during-saved-detail");
                root=Root(app,appPid);
                if (!LibraryReady(root,appPid) || Find(root,"entry-back",appPid)==null || Find(root,"copy-saved-detail",appPid)==null) return false;
                var answer=Find(root,"answer",appPid);
                return answer!=null && Texts(answer,appPid).SequenceEqual(new[]{contract.Answer}) &&
                    Texts(root,appPid).Count(text=>text==contract.Selected)==1;
            },10,"saved-detail-content-mismatch");
            if (!MetadataAbsent(root,appPid,id)) Fail("unexpected-saved-detail-metadata");
            evidence["saved_detail_verified"]=true; evidence["saved_metadata_absence_verified"]=true;
            CheckRestartIdentity(application,options["--application-path"],options["--application-start-ticks"]);
            stage="ordinary-quit";
            Focus(app,appPid); Native.Chord(app,appPid,0x51,false);
            Wait(()=>application.HasExited,10,"saved-application-did-not-quit");
            if (application.ExitCode!=0) Fail("saved-application-quit-nonzero");
            evidence["ordinary_quit_verified"]=true; evidence["app_exit_code"]=application.ExitCode;
        }
    }
    static void Drive(Contract contract,Dictionary<string,string> options) {
        int appPid=Pid(options["--application-pid"]), fixturePid=Pid(options["--fixture-pid"]);
        if (appPid==fixturePid || appPid==Process.GetCurrentProcess().Id || fixturePid==Process.GetCurrentProcess().Id) Fail("pid-scope-invalid");
        IntPtr fixture=Handle(options["--fixture-hwnd"]), app=IntPtr.Zero;
        evidence["application_pid"]=appPid; evidence["fixture_pid"]=fixturePid; evidence["fixture_hwnd"]=Hex(fixture);
        using (var application=Process.GetProcessById(appPid)) using (var source=Process.GetProcessById(fixturePid)) {
            // Retain process handles to distinguish exit from a later reused PID.
            IntPtr retainedApp=application.Handle, retainedSource=source.Handle;
            stage="application-ready";
            // Native UIA publication follows synchronous AppSpec.setup (including
            // shortcut configuration). A lease alone precedes that setup.
            AutomationElement root=null;
            Wait(()=> {
                if (application.HasExited) Fail("application-exited-before-ready");
                app=Native.ApplicationWindow(appPid);
                if (app==IntPtr.Zero) return false;
                root=Root(app,appPid);
                return Find(root,"passage-input",appPid)!=null && Find(root,"use-passage",appPid)!=null;
            },15,"initial-compose-unavailable");
            if (Find(root,"source-preview",appPid)!=null) Fail("unexpected-initial-source");
            Server(options["--server-state"],false);
            evidence["initial_compose_ready"]=true;
            stage="source-oracle";
            Focus(fixture,fixturePid);
            SourceOracle(contract,fixture,fixturePid,source);
            VerifyPixelMarker(fixture,fixturePid);
            if (application.HasExited || source.HasExited || !Owned(fixture,fixturePid) || Native.GetForegroundWindow()!=fixture) Fail("source-changed-before-shortcut");
            Server(options["--server-state"],false); evidence["fixture_selection_verified"]=true;
            stage="shortcut";
            Native.Chord(fixture,fixturePid,0x50,true); evidence["shortcut_events_inserted"]=6;
            stage="chooser";
            Wait(()=> { if (application.HasExited) Fail("application-exited-before-chooser"); app=Native.ApplicationWindow(appPid); return app!=IntPtr.Zero; },15,"chooser-window-unavailable");
            evidence["application_hwnd"]=Hex(app);
            root=Root(app,appPid); AutomationElement preview=null;
            Wait(()=> { root=Root(app,appPid); preview=Find(root,"source-preview",appPid); return preview!=null; },15,"source-preview-unavailable");
            if (!ReadingFixturePredicates.Source(Texts(preview,appPid),contract.Selected,contract.Before,contract.After)) Fail("captured-source-mismatch");
            evidence["chooser_source_verified"]=true;
            Server(options["--server-state"],false);
            stage="simplify";
            Invoke(Find(root,"simplify",appPid),appPid);
            AutomationElement consent=null;
            Wait(()=> { root=Root(app,appPid); consent=Find(root,"consent-continue",appPid); return consent!=null; },10,"consent-unavailable");
            Server(options["--server-state"],false); evidence["consent_before_network_verified"]=true;
            stage="reading";
            Invoke(consent,appPid);
            Wait(()=> {
                if (application.HasExited) Fail("application-exited-during-reading");
                root=Root(app,appPid); var answer=Find(root,"answer",appPid);
                return answer!=null && Texts(answer,appPid).SequenceEqual(new[]{contract.Answer}) &&
                    Find(root,"cancel-request",appPid)==null && Find(root,"answer-draft",appPid)==null &&
                    Find(root,"copy-answer",appPid)!=null && Find(root,"save-answer",appPid)!=null;
            },20,"completed-answer-unavailable");
            Server(options["--server-state"],true); evidence["provider_source_verified"]=true; evidence["completed_answer_verified"]=true;
            RecheckSource(contract,app,appPid,application);
            SaveAnswer(app,appPid,application);
            Server(options["--server-state"],true);
            stage="ordinary-quit";
            Focus(app,appPid); Native.Chord(app,appPid,0x51,false);
            Wait(()=>application.HasExited,10,"application-did-not-quit");
            if (application.ExitCode!=0) Fail("application-quit-nonzero");
            evidence["ordinary_quit_verified"]=true; evidence["app_exit_code"]=application.ExitCode;
        }
    }
    static class Native {
        [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left,Top,Right,Bottom; }
        [StructLayout(LayoutKind.Sequential)] struct GuiThreadInfo {
            public uint Size,Flags;
            public IntPtr Active,Focus,Capture,MenuOwner,MoveSize,Caret;
            public Rect CaretRect;
        }
        [StructLayout(LayoutKind.Sequential)] struct Keyboard { public ushort Key,Scan; public uint Flags,Time; public UIntPtr Extra; }
        [StructLayout(LayoutKind.Sequential)] struct Mouse { public int X,Y; public uint Data,Flags,Time; public UIntPtr Extra; }
        [StructLayout(LayoutKind.Explicit)] struct Union { [FieldOffset(0)] public Keyboard Keyboard; [FieldOffset(0)] public Mouse Mouse; }
        [StructLayout(LayoutKind.Sequential)] struct Input { public uint Type; public Union Data; }
        delegate bool WindowCallback(IntPtr hwnd,IntPtr parameter);
        [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hwnd);
        [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
        [DllImport("gdi32.dll")] public static extern bool GdiFlush();
        [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,StringBuilder text,int count);
        [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr hwnd,int message,UIntPtr wparam,IntPtr lparam,uint flags,uint timeout,out UIntPtr result);
        [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
        [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
        [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent,IntPtr child);
        [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint thread,ref GuiThreadInfo info);
        [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd,out Rect rect);
        [DllImport("user32.dll")] static extern bool EnumWindows(WindowCallback callback,IntPtr parameter);
        [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd,uint attribute,out int value,int size);
        [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
        [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
        public static bool FocusedWindow(IntPtr owner,out IntPtr focus) {
            uint pid; uint thread=GetWindowThreadProcessId(owner,out pid);
            var info=new GuiThreadInfo { Size=(uint)Marshal.SizeOf(typeof(GuiThreadInfo)) };
            bool available=thread!=0 && GetGUIThreadInfo(thread,ref info);
            focus=available ? info.Focus : IntPtr.Zero; return available;
        }
        static Input Key(ushort key,bool up) { return new Input { Type=1,Data=new Union { Keyboard=new Keyboard { Key=key,Flags=up ? 2U : 0U } } }; }
        public static void Home(IntPtr hwnd,int pid) {
            if (!Owned(hwnd,pid) || GetForegroundWindow()!=hwnd) Fail("input-target-not-owned");
            foreach (int key in new[]{0x11,0x12,0x10,0x5b,0x5c,0x24})
                if ((GetAsyncKeyState(key)&0x8000)!=0) Fail("input-key-already-held");
            Input down=Key(0x24,false), up=Key(0x24,true);
            down.Data.Keyboard.Flags=1; up.Data.Keyboard.Flags=3; // EXTENDEDKEY, plus KEYUP.
            Input[] inputs={down,up};
            if (SendInput(2,inputs,Marshal.SizeOf(typeof(Input)))!=2) {
                SendInput(1,new[]{up},Marshal.SizeOf(typeof(Input))); Fail("input-insertion-incomplete");
            }
        }
        public static void End(IntPtr hwnd,int pid) {
            if (!Owned(hwnd,pid) || GetForegroundWindow()!=hwnd) Fail("input-target-not-owned");
            foreach (int key in new[]{0x11,0x12,0x10,0x5b,0x5c,0x23})
                if ((GetAsyncKeyState(key)&0x8000)!=0) Fail("input-key-already-held");
            Input down=Key(0x23,false), up=Key(0x23,true);
            down.Data.Keyboard.Flags=1; up.Data.Keyboard.Flags=3;
            if (SendInput(2,new[]{down,up},Marshal.SizeOf(typeof(Input)))!=2) {
                SendInput(1,new[]{up},Marshal.SizeOf(typeof(Input))); Fail("input-insertion-incomplete");
            }
        }
        public static void Chord(IntPtr hwnd,int pid,ushort key,bool alt) {
            if (!Owned(hwnd,pid) || GetForegroundWindow()!=hwnd) Fail("input-target-not-owned");
            if ((GetAsyncKeyState(0x11)&0x8000)!=0 || (GetAsyncKeyState(0x12)&0x8000)!=0 || (GetAsyncKeyState(key)&0x8000)!=0) Fail("input-key-already-held");
            Input[] inputs=alt ? new[]{Key(0x11,false),Key(0x12,false),Key(key,false),Key(key,true),Key(0x12,true),Key(0x11,true)} :
                new[]{Key(0x11,false),Key(key,false),Key(key,true),Key(0x11,true)};
            if (SendInput((uint)inputs.Length,inputs,Marshal.SizeOf(typeof(Input)))!=(uint)inputs.Length) {
                Input[] release=alt ? new[]{Key(key,true),Key(0x12,true),Key(0x11,true)} : new[]{Key(key,true),Key(0x11,true)};
                SendInput((uint)release.Length,release,Marshal.SizeOf(typeof(Input))); Fail("input-insertion-incomplete");
            }
        }
        public static IntPtr ApplicationWindow(int pid) {
            IntPtr found=IntPtr.Zero; bool multiple=false;
            if (!EnumWindows(delegate(IntPtr hwnd,IntPtr _) {
                Rect rect; int cloaked;
                if (Owned(hwnd,pid) && IsWindowVisible(hwnd) && GetWindowRect(hwnd,out rect) &&
                    DwmGetWindowAttribute(hwnd,14,out cloaked,4)==0 && cloaked==0 && rect.Right-rect.Left>=480 && rect.Bottom-rect.Top>=180) {
                    if (found!=IntPtr.Zero) multiple=true; found=hwnd;
                }
                return true;
            },IntPtr.Zero)) Fail("owned-window-enumeration-failed");
            if (multiple) Fail("multiple-owned-reader-windows");
            return found;
        }
    }
}
#endif
