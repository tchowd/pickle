// Reversible layout experiment on one disposable hosted-fixture Edge window.
// No menu guessing, policy/display changes, navigation, or application code.
using System;
#if !PICKLE_PREDICATES
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Automation;
#endif

public static class BrowserExtensionLayoutPredicates {
    public static bool MayResize(string kind,bool liveOwned,bool foreground,bool singleDocument,
        bool documentBound,bool committedUri,int width,int height) {
        return kind=="edge" && liveOwned && foreground && singleDocument && documentBound && committedUri &&
            width>0 && height>0 && width<=32768 && height<=32768;
    }
    public static void SelfTest() {
        if(!MayResize("edge",true,true,true,true,true,988,692)) throw new Exception("layout-positive");
        for(int missing=0;missing<5;missing++) {
            var facts=new[]{true,true,true,true,true};facts[missing]=false;
            if(MayResize("edge",facts[0],facts[1],facts[2],facts[3],facts[4],988,692))
                throw new Exception("layout-no-mutation-before-proof");
        }
        if(MayResize("chrome",true,true,true,true,true,988,692) ||
            MayResize("edge",true,true,true,true,true,0,692) ||
            MayResize("edge",true,true,true,true,true,988,-1) ||
            MayResize("edge",true,true,true,true,true,32769,692)) throw new Exception("layout-boundary");
    }
}

#if !PICKLE_PREDICATES
public static partial class BrowserExtensionFixtureProgram {
    static bool LayoutWindowBound(IntPtr window,IntPtr owner,uint pid) {
        Check();uint current;
        return browser!=null && window!=IntPtr.Zero && owner!=IntPtr.Zero &&
            browser.OwnsLiveProcess(owner) && Native.GetProcessId(owner)==pid &&
            Native.IsWindow(window) && Native.IsWindowVisible(window) &&
            Native.GetWindowThreadProcessId(window,out current)!=0 && current==pid &&
            Native.GetAncestor(window,2)==window && Native.GetForegroundWindow()==window;
    }
    static bool LayoutElementBound(AutomationElement node,AutomationElement root,IntPtr window) {
        for(int depth=0;node!=null && depth<32;depth++) {
            Check();if(!browser.Owns(node.Current.ProcessId) || node.Current.IsPassword) return false;
            if(Automation.Compare(node,root)) return browser.OwnsWindow(window);
            node=TreeWalker.RawViewWalker.GetParent(node);
        }
        return false;
    }
    static bool LayoutPage(IntPtr window,IntPtr owner,uint pid,AutomationElement expectedRoot,
        AutomationElement expectedDocument,string expectedUri,out AutomationElement root,
        out AutomationElement document,out string uri) {
        root=null;document=null;uri=null;
        if(!LayoutWindowBound(window,owner,pid)) return false;
        root=AutomationElement.FromHandle(window);
        if(root==null || !LayoutElementBound(root,root,window) ||
            (expectedRoot!=null && !Automation.Compare(root,expectedRoot))) return false;
        var documents=new List<AutomationElement>();
        foreach(var node in Tree(window,false)) {
            if(!LayoutElementBound(node,root,window)) return false;
            if(node.Current.ControlType!=ControlType.Document) continue;
            if(!LayoutElementBound(node,root,window)) return false;
            string name=node.Current.Name;
            if(!LayoutElementBound(node,root,window)) return false;
            if(name=="Extensions" && !node.Current.IsOffscreen) documents.Add(node);
        }
        if(documents.Count!=1) return false;
        document=documents[0];
        if(expectedDocument!=null && !Automation.Compare(document,expectedDocument)) return false;
        object pattern;
        if(!LayoutElementBound(document,root,window) ||
            !document.TryGetCurrentPattern(ValuePattern.Pattern,out pattern)) return false;
        if(!LayoutElementBound(document,root,window)) return false;
        uri=((ValuePattern)pattern).Current.Value;
        if(!LayoutElementBound(document,root,window)) return false;
        if(BrowserExtensionPredicates.InternalUriForm(uri,"edge")==0 ||
            (expectedUri!=null && !String.Equals(uri,expectedUri,StringComparison.Ordinal))) return false;
        return LayoutElementBound(document,root,window) && LayoutWindowBound(window,owner,pid);
    }
    static bool LayoutDimensions(IntPtr window,out int clientWidth,out int clientHeight,
        out int outerWidth,out int outerHeight) {
        clientWidth=clientHeight=outerWidth=outerHeight=0;
        Native.Rect client,outer;
        if(!Native.GetClientRect(window,out client) || !Native.GetWindowRect(window,out outer)) return false;
        long cw=(long)client.Right-client.Left,ch=(long)client.Bottom-client.Top;
        long ow=(long)outer.Right-outer.Left,oh=(long)outer.Bottom-outer.Top;
        if(cw<=0 || ch<=0 || ow<=0 || oh<=0 || cw>32768 || ch>32768 || ow>32768 || oh>32768) return false;
        clientWidth=(int)cw;clientHeight=(int)ch;outerWidth=(int)ow;outerHeight=(int)oh;return true;
    }
    // Return=true means the mutation was safely reverified. Once the native
    // resize is attempted, actionPerformed remains true even if SetWindowPos
    // fails; callers must stop on actionPerformed && !returnValue. There is no
    // retry, new deadline, or Developer-mode action inside this helper.
    static bool TryEdgeDeveloperLayout(IntPtr window,string kind,out bool actionPerformed) {
        actionPerformed=false;
        var observation=new Dictionary<string,object> {
            {"action_performed",false},{"safely_verified",false},{"stage","before-ownership"},
            {"requested_outer_width",1280},{"requested_outer_height",800},
            {"before_client_width",null},{"before_client_height",null},
            {"after_client_width",null},{"after_client_height",null},
            {"after_outer_width",null},{"after_outer_height",null},
            {"before_verified",false},{"set_window_pos_succeeded",false},{"after_verified",false},
            {"unavailable",false}
        };
        Report["edge_layout_observation"]=observation;
        IntPtr owner=IntPtr.Zero;
        try {
            Check();uint pid;
            if(kind!="edge" || browser==null || !browser.OwnsWindow(window) ||
                Native.GetWindowThreadProcessId(window,out pid)==0 || pid==0) return false;
            // Hold this exact browser member across every pre/post check. Its
            // identity cannot be replaced by a later process reusing the PID.
            owner=Native.OpenProcess(0x00101000,false,pid); // SYNCHRONIZE | QUERY_LIMITED_INFORMATION.
            if(!LayoutWindowBound(window,owner,pid)) return false;
            observation["stage"]="before-document";
            AutomationElement beforeRoot,beforeDocument;string beforeUri;
            if(!LayoutPage(window,owner,pid,null,null,null,out beforeRoot,out beforeDocument,out beforeUri)) return false;
            int width,height,outerWidth,outerHeight;
            observation["stage"]="before-dimensions";
            if(!LayoutDimensions(window,out width,out height,out outerWidth,out outerHeight)) return false;
            observation["before_client_width"]=width;observation["before_client_height"]=height;
            if(!BrowserExtensionLayoutPredicates.MayResize(kind,LayoutWindowBound(window,owner,pid),
                Native.GetForegroundWindow()==window,true,LayoutElementBound(beforeDocument,beforeRoot,window),
                BrowserExtensionPredicates.InternalUriForm(beforeUri,"edge")!=0,width,height)) return false;
            observation["before_verified"]=true;
            // Reprove the same page after geometry inspection, before any
            // mutation. A changed route is refused before the resize request.
            observation["stage"]="before-action-document";
            AutomationElement currentRoot,currentDocument;string currentUri;
            if(!LayoutPage(window,owner,pid,beforeRoot,beforeDocument,beforeUri,out currentRoot,out currentDocument,out currentUri)) return false;
            // Last ownership/focus proof immediately precedes the sole native
            // request. ASYNCWINDOWPOS avoids waiting for the other UI thread;
            // NOMOVE | NOZORDER | NOACTIVATE changes only this HWND's size.
            observation["stage"]="resize";Check();
            if(!LayoutWindowBound(window,owner,pid)) return false;
            actionPerformed=true;observation["action_performed"]=true;
            bool resized=Native.SetWindowPos(window,IntPtr.Zero,0,0,1280,800,0x4016);
            observation["set_window_pos_succeeded"]=resized;
            if(!resized) return false;
            observation["stage"]="after-dimensions";
            // Observe the one posted request, never resubmit it. A local2s
            // ceiling is clipped to the original scenario deadline.
            DateTime until=DateTime.UtcNow.AddSeconds(2);if(until>deadline) until=deadline;
            bool dimensions=false;
            do {
                Check();if(!LayoutWindowBound(window,owner,pid) ||
                    !LayoutDimensions(window,out width,out height,out outerWidth,out outerHeight)) return false;
                observation["after_client_width"]=width;observation["after_client_height"]=height;
                observation["after_outer_width"]=outerWidth;observation["after_outer_height"]=outerHeight;
                if(outerWidth==1280 && outerHeight==800) { dimensions=true;break; }
                int remaining=(int)Math.Min(25,Math.Max(0,(until-DateTime.UtcNow).TotalMilliseconds));
                if(remaining>0) Thread.Sleep(remaining);
            } while(DateTime.UtcNow<until);
            if(!dimensions) return false;
            observation["stage"]="after-document";
            AutomationElement afterRoot,afterDocument;string afterUri;
            if(!LayoutPage(window,owner,pid,beforeRoot,beforeDocument,beforeUri,out afterRoot,out afterDocument,out afterUri)) return false;
            observation["after_verified"]=true;observation["safely_verified"]=true;observation["stage"]="complete";
            return true;
        } catch(ExtensionStageFailure) { observation["unavailable"]=true;return false; }
        catch(Exception) { observation["unavailable"]=true;return false; }
        finally { if(owner!=IntPtr.Zero) Native.CloseHandle(owner); }
        // The only affected window belongs to the disposable fixture profile;
        // existing exact-job drain/profile deletion provides final teardown.
    }
    static partial class Native {
        [DllImport("user32.dll",SetLastError=true)] public static extern bool SetWindowPos(
            IntPtr window,IntPtr insertAfter,int x,int y,int width,int height,uint flags);
    }
}
#endif
