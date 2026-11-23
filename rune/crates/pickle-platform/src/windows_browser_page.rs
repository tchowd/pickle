//! URL reads are confined to Chromium's top-level UIA document, never the
//! omnibox, links, iframe documents, names, or page text. All COM stays on MTA.
use super::*;
use ::windows::{
    Win32::{
        Foundation::{HANDLE, HWND, VARIANT_TRUE, WAIT_TIMEOUT},
        System::{
            Com::*,
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
            },
            Variant::{VARIANT, VT_BOOL, VariantClear},
        },
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
    core::{GUID, HRESULT, Interface, PWSTR, w},
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

// Chromium documents this Boolean as true only on the top-level tab Document,
// explicitly false on child-frame documents. No window-class/name heuristic.
// https://chromium.googlesource.com/chromium/src.git/+/refs/heads/main/docs/accessibility/browser/custom_uia_properties.md
const WEB_ROOT: GUID = GUID::from_u128(0xc5fdc049_4eec_4299_87bb_ca9fc718a681);
// Match selection's established per-request cap. UIA's connection and
// transaction settings bound individual provider requests, not this complete
// operation; every request is still clipped to the remaining total budget.
// https://learn.microsoft.com/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_connectiontimeout
const UIA_REQUEST_TIMEOUT_MS: u32 = 500;

#[derive(Clone, Copy)]
struct NativeBranch {
    window: u64,
    valid: bool,
    owner: u32,
    root: u64,
}
fn foreign_branch_is_outside(source_window: u64, node_pid: i32, branch: NativeBranch) -> bool {
    source_window != 0
        && node_pid > 0
        && branch.window != 0
        && branch.valid
        && branch.owner == node_pid as u32
        && branch.root != 0
        && branch.root != source_window
}
fn foreign_anchor_is_bound(
    source_window: u64,
    node_pid: i32,
    branch: NativeBranch,
    executable_matches: bool,
) -> bool {
    source_window != 0
        && node_pid > 0
        && branch.window != 0
        && branch.valid
        && branch.owner == node_pid as u32
        && branch.root == source_window
        && executable_matches
}
fn pid_is_path_bound(
    source_pid: u32,
    node_pid: i32,
    anchors: impl IntoIterator<Item = u32>,
) -> bool {
    node_pid > 0
        && (source_pid == node_pid as u32 || anchors.into_iter().any(|pid| pid == node_pid as u32))
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct AnchorKey {
    window: u64,
    pid: u32,
}
struct NativeAnchor {
    key: AnchorKey,
    process: OwnedHandle,
}
type AnchorPath = Vec<Rc<NativeAnchor>>;
struct DocumentBinding {
    element: IUIAutomationElement,
    anchors: AnchorPath,
    raw_chain: Vec<IUIAutomationElement>,
}
struct RawProof {
    anchors: AnchorPath,
    chain: Vec<IUIAutomationElement>,
    document_pid: u32,
}

fn focused_discovery_walker(
    ui: &IUIAutomation,
    source_pid: u32,
) -> Result<IUIAutomationTreeWalker, PlatformError> {
    // Preserve every pruning boundary, native anchor and unknown/foreign PID.
    // Only same-process layout/command nodes may be promoted past. Raw ancestry
    // is independently proved before and after any document Value is read.
    let property = |id, value: i32| {
        unsafe { ui.CreatePropertyCondition(id, &VARIANT::from(value)) }.map_err(|_| unavailable())
    };
    let document = property(UIA_ControlTypePropertyId, UIA_DocumentControlTypeId.0)?;
    let edit = property(UIA_ControlTypePropertyId, UIA_EditControlTypeId.0)?;
    let link = property(UIA_ControlTypePropertyId, UIA_HyperlinkControlTypeId.0)?;
    let no_hwnd = property(UIA_NativeWindowHandlePropertyId, 0)?;
    let native = unsafe { ui.CreateNotCondition(&no_hwnd) }.map_err(|_| unavailable())?;
    let owned_pid = property(UIA_ProcessIdPropertyId, source_pid as i32)?;
    let foreign_pid = unsafe { ui.CreateNotCondition(&owned_pid) }.map_err(|_| unavailable())?;
    let boundary = unsafe { ui.CreateOrCondition(&document, &edit) }.map_err(|_| unavailable())?;
    let boundary = unsafe { ui.CreateOrCondition(&boundary, &link) }.map_err(|_| unavailable())?;
    let condition =
        unsafe { ui.CreateOrCondition(&boundary, &native) }.map_err(|_| unavailable())?;
    let condition =
        unsafe { ui.CreateOrCondition(&condition, &foreign_pid) }.map_err(|_| unavailable())?;
    unsafe { ui.CreateTreeWalker(&condition) }.map_err(|_| unavailable())
}

// Both hosted fixture types own foreground windows. Serialize their native UI
// work inside this test binary without changing ordinary parallel unit checks.
#[cfg(test)]
pub(super) fn native_browser_fixture_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// TreeWalker navigation explicitly returns S_OK + NULL at the end. The
// windows 0.62 non-null interface projection converts that successful NULL to
// Error::empty(), losing its distinction from a real HRESULT failure. Preserve
// the ABI result here; never reinterpret E_POINTER or other failed HRESULTs as
// an ordinary end of traversal.
// https://learn.microsoft.com/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtreewalker-getnextsiblingelement
// SAFETY: query must follow the COM out-interface contract: write NULL or a
// valid owned IUIAutomationElement reference to the initialized output pointer.
unsafe fn nullable_automation_element(
    query: impl FnOnce(*mut *mut std::ffi::c_void) -> HRESULT,
) -> ::windows::core::Result<Option<IUIAutomationElement>> {
    let mut raw = std::ptr::null_mut();
    let status = query(&mut raw);
    let element = if raw.is_null() {
        None
    } else {
        // Adopt the one returned reference, including releasing it if a
        // provider returned an owned reference together with a failed status.
        Some(unsafe { IUIAutomationElement::from_raw(raw) })
    };
    status.ok()?;
    Ok(element)
}
struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

pub(super) fn capture(
    source: &CapturedSelection,
    expected: &str,
    cancel: CancellationToken,
) -> Result<CapturedPage, PlatformError> {
    // No permission/package changes or automatic registration are attempted.
    if !crate::windows_context::has_package_identity() {
        return Err(unavailable());
    }
    let source = source.clone();
    let expected = expected.to_owned();
    on_document_worker(cancel.clone(), move || {
        let reader = DocumentReader::new(&source, cancel.clone())?;
        verified_capture(
            &source,
            &expected,
            &cancel,
            || reader.observe(),
            || crate::capture_page_with_cancel(&source, cancel.clone()),
        )
    })
}

// UIA connection/transaction timeouts are cooperative RPC settings, not a hard
// deadline on COM activation or a faulty provider. Isolate all COM in one MTA
// worker and retain admission there until it actually returns. The caller can
// fall back to browser text even when that one provider never returns.
fn on_document_worker<T: Send + 'static>(
    cancel: CancellationToken,
    work: impl FnOnce() -> Result<T, PlatformError> + Send + 'static,
) -> Result<T, PlatformError> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    if cancel.is_cancelled()
        || ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return Err(unavailable());
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("pickle-browser-document".into())
        .spawn(move || {
            struct Flight;
            impl Drop for Flight {
                fn drop(&mut self) {
                    ACTIVE.store(false, Ordering::Release);
                }
            }
            let _flight = Flight;
            let result = (|| {
                unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                    .ok()
                    .map_err(|_| unavailable())?;
                let _com = ComGuard;
                work()
            })();
            let _ = sender.send(result);
        });
    if worker.is_err() {
        ACTIVE.store(false, Ordering::Release);
        return Err(unavailable());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            return Err(unavailable());
        }
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(unavailable()),
        }
    }
}

struct DocumentReader {
    source: CapturedSelection,
    ui: IUIAutomation,
    settings: IUIAutomation2,
    root: IUIAutomationElement,
    walker: IUIAutomationTreeWalker,
    discovery_walker: IUIAutomationTreeWalker,
    property: UIA_PROPERTY_ID,
    first_document: RefCell<Option<DocumentBinding>>,
    source_executable: PathBuf,
    deadline: Instant,
    cancel: CancellationToken,
    #[cfg(test)]
    diagnostic: std::cell::Cell<ReaderDiagnostic>,
}
#[cfg(test)]
#[derive(Clone, Copy)]
struct ReaderDiagnostic {
    stage: &'static str,
    hresult: i32,
    nodes: usize,
    documents: usize,
    control_type: i32,
    variant_type: u16,
    variant_bool: i16,
    nullable_navigation_ends: usize,
    root_pid: i32,
    node_pid: i32,
    queue_len: usize,
    node_depth: usize,
    node_native_hwnd_present: Option<bool>,
    node_hwnd_root_matches_source: Option<bool>,
    node_executable_matches_source: Option<bool>,
    post_rejection_metadata: bool,
    node_hwnd_owner_pid: u32,
    pruned_foreign_native_branches: usize,
    anchored_foreign_native_branches: usize,
    virtual_nodes_using_path_anchor: usize,
    candidate_native_anchors: usize,
    anchor_revalidations: usize,
    document_pid: u32,
    raw_ancestry_checks: usize,
    raw_ancestry_nodes: usize,
}
impl DocumentReader {
    fn new(source: &CapturedSelection, cancel: CancellationToken) -> Result<Self, PlatformError> {
        let deadline = Instant::now() + Duration::from_millis(1800);
        crate::windows_impl::validate_source_window(source)?;
        let source_executable = std::fs::canonicalize(
            source
                .app_id
                .strip_prefix("windows:")
                .ok_or_else(unavailable)?,
        )
        .map_err(|_| unavailable())?;
        let ui: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| unavailable())?;
        let settings: IUIAutomation2 = ui.cast().map_err(|_| unavailable())?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || cancel.is_cancelled() {
            return Err(unavailable());
        }
        let timeout = remaining
            .as_millis()
            .clamp(1, u128::from(UIA_REQUEST_TIMEOUT_MS)) as u32;
        unsafe { settings.SetConnectionTimeout(timeout) }.map_err(|_| unavailable())?;
        unsafe { settings.SetTransactionTimeout(timeout) }.map_err(|_| unavailable())?;
        let registrar: IUIAutomationRegistrar =
            unsafe { CoCreateInstance(&CUIAutomationRegistrar, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| unavailable())?;
        let property = unsafe {
            registrar.RegisterProperty(&UIAutomationPropertyInfo {
                guid: WEB_ROOT,
                pProgrammaticName: w!("IsWebContentRoot"),
                r#type: UIAutomationType_Bool,
            })
        }
        .map_err(|_| unavailable())?;
        if property <= 0 {
            return Err(unavailable());
        }
        let root = unsafe {
            ui.ElementFromHandle(HWND(source.source_window.ok_or_else(unavailable)? as *mut _))
        }
        .map_err(|_| unavailable())?;
        let walker = unsafe { ui.RawViewWalker() }.map_err(|_| unavailable())?;
        let discovery_walker = focused_discovery_walker(&ui, source.source_pid)?;
        Ok(Self {
            source: source.clone(),
            ui,
            settings,
            root,
            walker,
            discovery_walker,
            property: UIA_PROPERTY_ID(property),
            first_document: RefCell::new(None),
            source_executable,
            deadline,
            cancel,
            #[cfg(test)]
            diagnostic: std::cell::Cell::new(ReaderDiagnostic {
                stage: "constructed",
                hresult: 0,
                nodes: 0,
                documents: 0,
                control_type: 0,
                variant_type: 0,
                variant_bool: 0,
                nullable_navigation_ends: 0,
                root_pid: 0,
                node_pid: 0,
                queue_len: 0,
                node_depth: 0,
                node_native_hwnd_present: None,
                node_hwnd_root_matches_source: None,
                node_executable_matches_source: None,
                post_rejection_metadata: false,
                node_hwnd_owner_pid: 0,
                pruned_foreign_native_branches: 0,
                anchored_foreign_native_branches: 0,
                virtual_nodes_using_path_anchor: 0,
                candidate_native_anchors: 0,
                anchor_revalidations: 0,
                document_pid: 0,
                raw_ancestry_checks: 0,
                raw_ancestry_nodes: 0,
            }),
        })
    }
    fn stage(&self, stage: &'static str) {
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.stage = stage;
            value
        });
        #[cfg(not(test))]
        let _ = stage;
    }
    fn rpc<T>(&self, result: ::windows::core::Result<T>) -> Result<T, PlatformError> {
        result.map_err(|error| {
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.hresult = error.code().0;
                value
            });
            #[cfg(not(test))]
            let _ = error;
            unavailable()
        })
    }
    #[cfg(test)]
    fn failure_stage(&self) -> &'static str {
        self.diagnostic.get().stage
    }
    #[cfg(test)]
    fn failure_diagnostics(&self) -> serde_json::Value {
        let value = self.diagnostic.get();
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        let elapsed = (self.deadline - Duration::from_millis(1800)).elapsed();
        serde_json::json!({"stage":value.stage,"elapsed_ms":elapsed.as_millis(),"remaining_ms":remaining.as_millis(),"cancelled":self.cancel.is_cancelled(),"hresult":value.hresult,"nodes":value.nodes,"documents":value.documents,"control_type":value.control_type,"custom_variant_type":value.variant_type,"custom_variant_bool":value.variant_bool,"nullable_navigation_ends":value.nullable_navigation_ends,
            "source_pid":self.source.source_pid,"root_pid":value.root_pid,"node_pid":value.node_pid,"queue_len":value.queue_len,"node_depth":value.node_depth,
            "node_native_hwnd_present":value.node_native_hwnd_present,"node_hwnd_root_matches_source":value.node_hwnd_root_matches_source,"node_hwnd_owner_pid":value.node_hwnd_owner_pid,"node_executable_matches_source":value.node_executable_matches_source,"post_rejection_metadata":value.post_rejection_metadata,"pruned_foreign_native_branches":value.pruned_foreign_native_branches,
            "anchored_foreign_native_branches":value.anchored_foreign_native_branches,"virtual_nodes_using_path_anchor":value.virtual_nodes_using_path_anchor,"candidate_native_anchors":value.candidate_native_anchors,"anchor_revalidations":value.anchor_revalidations,"document_pid":value.document_pid,
            "filtered_discovery":true,"raw_ancestry_checks":value.raw_ancestry_checks,"raw_ancestry_nodes":value.raw_ancestry_nodes})
    }
    #[cfg(test)]
    fn record_rejected_node(&self, node: &IUIAutomationElement, pid: i32, branch: NativeBranch) {
        use ::windows::{
            Win32::{
                Foundation::CloseHandle,
                System::Threading::{
                    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                    QueryFullProcessImageNameW,
                },
            },
            core::PWSTR,
        };
        // These extra queries occur only after the production PID guard has
        // rejected this observation. They never influence capture admission.
        self.diagnostic.update(|mut value| {
            value.post_rejection_metadata = true;
            value
        });
        if self.cancel.is_cancelled() || Instant::now() >= self.deadline {
            return;
        }
        let control = unsafe { node.CurrentControlType() }.ok();
        let executable_matches = (|| {
            if pid <= 0 {
                return None;
            }
            let source = self.source.app_id.strip_prefix("windows:")?;
            let process =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32) }
                    .ok()?;
            let mut path = vec![0u16; 32768];
            let mut size = path.len() as u32;
            let read = unsafe {
                QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    PWSTR(path.as_mut_ptr()),
                    &mut size,
                )
            };
            let _ = unsafe { CloseHandle(process) };
            read.ok()?;
            let path = String::from_utf16(&path[..size as usize]).ok()?;
            let actual = std::fs::canonicalize(path).ok()?;
            let expected = std::fs::canonicalize(source).ok()?;
            Some(actual.to_str()?.eq_ignore_ascii_case(expected.to_str()?))
        })();
        self.diagnostic.update(|mut value| {
            if let Some(control) = control {
                value.control_type = control.0;
            }
            value.node_native_hwnd_present = Some(branch.window != 0);
            value.node_hwnd_owner_pid = branch.owner;
            value.node_hwnd_root_matches_source =
                (branch.root != 0).then_some(branch.root == self.source.source_window.unwrap_or(0));
            value.node_executable_matches_source = executable_matches;
            value
        });
    }
    fn native_branch(&self, node: &IUIAutomationElement) -> Result<NativeBranch, PlatformError> {
        self.stage("foreign-native-hwnd");
        self.check()?;
        let window = self.rpc(unsafe { node.CurrentNativeWindowHandle() })?;
        self.branch_for_window(window)
    }
    fn branch_for_window(&self, window: HWND) -> Result<NativeBranch, PlatformError> {
        self.check()?;
        let valid = unsafe { IsWindow(Some(window)) }.as_bool();
        self.check()?;
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut owner)) };
        self.check()?;
        // GA_ROOT follows only parents, whereas GA_ROOTOWNER would also pull
        // separate owned popup windows into the captured window's scope.
        let root = unsafe { GetAncestor(window, GA_ROOT) };
        Ok(NativeBranch {
            window: window.0 as u64,
            valid,
            owner,
            root: root.0 as u64,
        })
    }
    fn anchor_executable_matches(&self, process: &OwnedHandle) -> Result<bool, PlatformError> {
        self.stage("native-anchor-executable");
        self.check()?;
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        self.rpc(unsafe {
            QueryFullProcessImageNameW(
                HANDLE(process.as_raw_handle()),
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut size,
            )
        })?;
        self.check()?;
        let path = String::from_utf16(&path[..size as usize]).map_err(|_| unavailable())?;
        let path = std::fs::canonicalize(path).map_err(|_| unavailable())?;
        self.check()?;
        Ok(path
            .to_str()
            .zip(self.source_executable.to_str())
            .is_some_and(|(actual, expected)| actual.eq_ignore_ascii_case(expected)))
    }
    fn validate_anchor(&self, anchor: &NativeAnchor) -> Result<(), PlatformError> {
        self.stage("native-anchor-alive");
        self.check()?;
        if unsafe { WaitForSingleObject(HANDLE(anchor.process.as_raw_handle()), 0) } != WAIT_TIMEOUT
        {
            return Err(unavailable());
        }
        self.stage("native-anchor-window");
        let branch = self.branch_for_window(HWND(anchor.key.window as *mut _))?;
        let matches = self.anchor_executable_matches(&anchor.process)?;
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.node_native_hwnd_present = Some(branch.window != 0);
            value.node_hwnd_owner_pid = branch.owner;
            value.node_hwnd_root_matches_source =
                (branch.root != 0).then_some(Some(branch.root) == self.source.source_window);
            value.node_executable_matches_source = Some(matches);
            value
        });
        if !foreign_anchor_is_bound(
            self.source.source_window.unwrap_or(0),
            anchor.key.pid as i32,
            branch,
            matches,
        ) {
            self.stage("native-anchor-binding");
            return Err(unavailable());
        }
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.anchor_revalidations += 1;
            value
        });
        Ok(())
    }
    fn bind_anchor(
        &self,
        node_pid: i32,
        branch: NativeBranch,
        path: &mut AnchorPath,
    ) -> Result<(), PlatformError> {
        self.stage("native-anchor-binding");
        if !foreign_anchor_is_bound(
            self.source.source_window.unwrap_or(0),
            node_pid,
            branch,
            true,
        ) {
            return Err(unavailable());
        }
        let key = AnchorKey {
            window: branch.window,
            pid: node_pid as u32,
        };
        if let Some(anchor) = path.iter().find(|anchor| anchor.key == key) {
            return self.validate_anchor(anchor);
        }
        self.stage("native-anchor-process");
        self.check()?;
        let process = self.rpc(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                key.pid,
            )
        })?;
        let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
        let anchor = Rc::new(NativeAnchor { key, process });
        self.validate_anchor(&anchor)?;
        if path.len() >= 12 {
            return Err(unavailable());
        }
        path.push(anchor);
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.anchored_foreign_native_branches += 1;
            value
        });
        Ok(())
    }
    fn validate_path(&self, path: &AnchorPath) -> Result<(), PlatformError> {
        self.stage("document-source-binding");
        self.check()?;
        crate::windows_impl::validate_source_window(&self.source)?;
        self.check()?;
        let window = HWND(self.source.source_window.unwrap_or(0) as *mut _);
        if unsafe { GetForegroundWindow() } != window
            || !unsafe { IsWindowVisible(window) }.as_bool()
        {
            return Err(unavailable());
        }
        for anchor in path {
            self.validate_anchor(anchor)?;
        }
        self.check()
    }
    fn qualify_node(
        &self,
        node: &IUIAutomationElement,
        node_pid: i32,
        anchors: &mut AnchorPath,
        pid_stage: &'static str,
    ) -> Result<bool, PlatformError> {
        if node_pid as u32 == self.source.source_pid {
            return Ok(true);
        }
        let branch = self.native_branch(node)?;
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.node_native_hwnd_present = Some(branch.window != 0);
            value.node_hwnd_owner_pid = branch.owner;
            value.node_hwnd_root_matches_source =
                (branch.root != 0).then_some(Some(branch.root) == self.source.source_window);
            value
        });
        if foreign_branch_is_outside(self.source.source_window.unwrap_or(0), node_pid, branch) {
            return Ok(false);
        }
        if branch.window != 0 {
            self.bind_anchor(node_pid, branch, anchors)?;
        } else if pid_is_path_bound(
            self.source.source_pid,
            node_pid,
            anchors.iter().map(|anchor| anchor.key.pid),
        ) {
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.virtual_nodes_using_path_anchor += 1;
                value
            });
        } else {
            self.stage(pid_stage);
            #[cfg(test)]
            self.record_rejected_node(node, node_pid, branch);
            return Err(unavailable());
        }
        Ok(true)
    }
    fn raw_proof(&self, document: &IUIAutomationElement) -> Result<RawProof, PlatformError> {
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.raw_ancestry_checks += 1;
            value.raw_ancestry_nodes = 0;
            value
        });
        let mut chain = Vec::new();
        let mut current = document.clone();
        let mut reached = false;
        for _ in 0..16 {
            self.stage("raw-ancestry-root");
            self.check()?;
            if self.same(&current, &self.root)? {
                reached = true;
                break;
            }
            chain.push(current.clone());
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.raw_ancestry_nodes = chain.len();
                value
            });
            self.stage("raw-ancestry-parent");
            self.check()?;
            current = self.rpc(unsafe { self.walker.GetParentElement(&current) })?;
        }
        if !reached {
            self.stage("raw-ancestry-limit");
            return Err(unavailable());
        }
        self.stage("raw-ancestry-root");
        self.check()?;
        if self.rpc(unsafe { current.CurrentProcessId() })? as u32 != self.source.source_pid {
            return Err(unavailable());
        }
        let mut anchors = AnchorPath::new();
        let mut document_pid = 0;
        // Rebuild ownership top-down. Filtered discovery may have promoted a
        // descendant past omitted wrappers; those wrappers remain mandatory
        // parts of this raw proof, never a global PID allowlist.
        for (index, node) in chain.iter().enumerate().rev() {
            self.stage("raw-ancestry-pid");
            self.check()?;
            let pid = self.rpc(unsafe { node.CurrentProcessId() })?;
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.node_pid = pid;
                value
            });
            if !self.qualify_node(node, pid, &mut anchors, "raw-ancestry-pid")? {
                self.stage("raw-ancestry-pid");
                return Err(unavailable());
            }
            self.stage("raw-ancestry-boundary");
            self.check()?;
            let control = self.rpc(unsafe { node.CurrentControlType() })?;
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.control_type = control.0;
                value
            });
            if index != 0
                && [
                    UIA_DocumentControlTypeId,
                    UIA_EditControlTypeId,
                    UIA_HyperlinkControlTypeId,
                ]
                .contains(&control)
            {
                return Err(unavailable());
            }
            if index == 0 {
                if control != UIA_DocumentControlTypeId || !self.web_root(node)? {
                    return Err(unavailable());
                }
                self.check()?;
                if self.rpc(unsafe { node.CurrentIsOffscreen() })?.as_bool() {
                    return Err(unavailable());
                }
                document_pid = pid as u32;
            }
        }
        if chain.is_empty() {
            // A native root can itself be a Document. Its process was checked
            // above and it still needs the same document role/marker proof.
            self.stage("raw-ancestry-boundary");
            self.check()?;
            if self.rpc(unsafe { document.CurrentControlType() })? != UIA_DocumentControlTypeId
                || !self.web_root(document)?
            {
                return Err(unavailable());
            }
            self.check()?;
            if self
                .rpc(unsafe { document.CurrentIsOffscreen() })?
                .as_bool()
            {
                return Err(unavailable());
            }
            document_pid = self.source.source_pid;
        }
        self.validate_path(&anchors)?;
        Ok(RawProof {
            anchors,
            chain,
            document_pid,
        })
    }
    fn same_chain(
        &self,
        before: &[IUIAutomationElement],
        now: &[IUIAutomationElement],
    ) -> Result<bool, PlatformError> {
        if before.len() != now.len() {
            return Ok(false);
        }
        for (before, now) in before.iter().zip(now) {
            if !self.same(before, now)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn check(&self) -> Result<(), PlatformError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if self.cancel.is_cancelled() || remaining.is_zero() {
            self.stage("budget-or-cancelled");
            return Err(unavailable());
        }
        let millis = remaining
            .as_millis()
            .clamp(1, u128::from(UIA_REQUEST_TIMEOUT_MS)) as u32;
        self.rpc(unsafe { self.settings.SetConnectionTimeout(millis) })?;
        self.rpc(unsafe { self.settings.SetTransactionTimeout(millis) })?;
        Ok(())
    }
    fn same(
        &self,
        left: &IUIAutomationElement,
        right: &IUIAutomationElement,
    ) -> Result<bool, PlatformError> {
        self.check()?;
        self.rpc(unsafe { self.ui.CompareElements(left, right) })
            .map(|same| same.as_bool())
    }
    fn navigate(
        &self,
        element: &IUIAutomationElement,
        first_child: bool,
    ) -> Result<Option<IUIAutomationElement>, PlatformError> {
        let walker = Interface::vtable(&self.discovery_walker);
        let getter = if first_child {
            walker.GetFirstChildElement
        } else {
            walker.GetNextSiblingElement
        };
        let result = unsafe {
            nullable_automation_element(|output| {
                getter(
                    Interface::as_raw(&self.discovery_walker),
                    Interface::as_raw(element),
                    output,
                )
            })
        };
        let result = self.rpc(result)?;
        #[cfg(test)]
        if result.is_none() {
            self.diagnostic.update(|mut value| {
                value.nullable_navigation_ends += 1;
                value
            });
        }
        Ok(result)
    }
    fn web_root(&self, element: &IUIAutomationElement) -> Result<bool, PlatformError> {
        self.check()?;
        let mut value =
            self.rpc(unsafe { element.GetCurrentPropertyValueEx(self.property, true) })?;
        #[cfg(test)]
        self.diagnostic.update(|mut diagnostic| {
            diagnostic.variant_type = unsafe { value.Anonymous.Anonymous.vt.0 };
            diagnostic.variant_bool = if unsafe { value.Anonymous.Anonymous.vt } == VT_BOOL {
                unsafe { value.Anonymous.Anonymous.Anonymous.boolVal.0 }
            } else {
                0
            };
            diagnostic
        });
        let supported = unsafe {
            value.Anonymous.Anonymous.vt == VT_BOOL
                && value.Anonymous.Anonymous.Anonymous.boolVal == VARIANT_TRUE
        };
        self.rpc(unsafe { VariantClear(&mut value) })?;
        Ok(supported)
    }
    fn observe(&self) -> Result<DocumentObservation, PlatformError> {
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.document_pid = 0;
            value.candidate_native_anchors = 0;
            value.root_pid = 0;
            value
        });
        self.stage("source-owner");
        self.check()?;
        crate::windows_impl::validate_source_window(&self.source)?;
        self.stage("source-foreground-visible");
        let hwnd = HWND(self.source.source_window.ok_or_else(unavailable)? as *mut _);
        if unsafe { GetForegroundWindow() } != hwnd || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return Err(unavailable());
        }
        self.stage("root-from-handle");
        let fresh = self.rpc(unsafe { self.ui.ElementFromHandle(hwnd) })?;
        self.stage("root-identity");
        if !self.same(&self.root, &fresh)? {
            return Err(unavailable());
        }
        self.check()?;
        self.stage("root-pid");
        let root_pid = self.rpc(unsafe { fresh.CurrentProcessId() })?;
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.root_pid = root_pid;
            value
        });
        if root_pid as u32 != self.source.source_pid {
            return Err(unavailable());
        }
        // A secure focused field is refused without reading any field value.
        self.check()?;
        self.stage("focused-element");
        let mut focused = self.rpc(unsafe { self.ui.GetFocusedElement() })?;
        let mut reached = false;
        for _ in 0..16 {
            self.check()?;
            self.stage("focus-password");
            if self.rpc(unsafe { focused.CurrentIsPassword() })?.as_bool() {
                return Err(unavailable());
            }
            self.stage("focus-root-identity");
            if self.same(&focused, &fresh)? {
                reached = true;
                break;
            }
            self.check()?;
            self.stage("focus-parent");
            focused = self.rpc(unsafe { self.walker.GetParentElement(&focused) })?;
        }
        if !reached {
            self.stage("focus-ancestry-limit");
            return Err(unavailable());
        }
        let mut queue = VecDeque::from([(fresh, 0usize, AnchorPath::new())]);
        let mut count = 1usize;
        let mut documents = Vec::new();
        while let Some((node, depth, mut anchors)) = queue.pop_front() {
            self.stage("traversal-limit");
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.nodes = count;
                value.documents = documents.len();
                value.queue_len = queue.len();
                value.node_depth = depth;
                value.node_pid = 0;
                value.control_type = 0;
                value.node_native_hwnd_present = None;
                value.node_hwnd_root_matches_source = None;
                value.node_executable_matches_source = None;
                value.post_rejection_metadata = false;
                value.node_hwnd_owner_pid = 0;
                value
            });
            if depth > 12 || count > 192 {
                return Err(unavailable());
            }
            self.check()?;
            self.stage("traversal-pid");
            let node_pid = self.rpc(unsafe { node.CurrentProcessId() })?;
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.node_pid = node_pid;
                value
            });
            if !self.qualify_node(&node, node_pid, &mut anchors, "traversal-pid")? {
                #[cfg(test)]
                self.diagnostic.update(|mut value| {
                    value.pruned_foreign_native_branches += 1;
                    value
                });
                // Separate OS top-level windows are outside this exact HWND
                // capture (WGC IncludeSecondaryWindows remains default false).
                continue;
            }
            self.check()?;
            self.stage("traversal-control-type");
            let control = self.rpc(unsafe { node.CurrentControlType() })?;
            #[cfg(test)]
            self.diagnostic.update(|mut value| {
                value.control_type = control.0;
                value
            });
            if control == UIA_DocumentControlTypeId {
                self.stage("document-custom-web-root");
                if self.web_root(&node)? {
                    self.check()?;
                    self.stage("document-visible");
                    if !self.rpc(unsafe { node.CurrentIsOffscreen() })?.as_bool() {
                        documents.push((node, anchors.clone(), node_pid as u32));
                    }
                }
                continue; // Never traverse any document's page/iframe/link content.
            }
            if control == UIA_EditControlTypeId || control == UIA_HyperlinkControlTypeId {
                continue;
            }
            self.check()?;
            self.stage("traversal-first-child");
            let mut child = self.navigate(&node, true)?;
            while let Some(element) = child {
                count += 1;
                if count > 192 {
                    self.stage("traversal-node-limit");
                    #[cfg(test)]
                    self.diagnostic.update(|mut value| {
                        value.nodes = count;
                        value.queue_len = queue.len();
                        value
                    });
                    return Err(unavailable());
                }
                self.check()?;
                self.stage("traversal-next-sibling");
                child = self.navigate(&element, false)?;
                queue.push_back((element, depth + 1, anchors.clone()));
            }
        }
        self.stage("document-count");
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.documents = documents.len();
            value
        });
        if documents.len() != 1 {
            return Err(unavailable());
        }
        let (document, discovered_anchors, _) = documents.pop().ok_or_else(unavailable)?;
        let proof = self.raw_proof(&document)?;
        // Discovery's anchors must also occur on the actual raw path, in order.
        // Additional raw anchors are independently validated by raw_proof.
        let mut raw_anchors = proof.anchors.iter();
        for discovered in &discovered_anchors {
            if !raw_anchors.any(|anchor| anchor.key == discovered.key) {
                self.stage("raw-ancestry-anchor-path");
                return Err(unavailable());
            }
        }
        self.validate_path(&discovered_anchors)?;
        let RawProof {
            anchors,
            chain: raw_chain,
            document_pid,
        } = proof;
        #[cfg(test)]
        self.diagnostic.update(|mut value| {
            value.candidate_native_anchors = anchors.len();
            value.document_pid = document_pid;
            value
        });
        #[cfg(not(test))]
        let _ = document_pid;
        {
            self.stage("document-identity");
            let baseline = self.first_document.borrow();
            if let Some(first) = baseline.as_ref()
                && (!self.same(&first.element, &document)?
                    || first.anchors.len() != anchors.len()
                    || !first
                        .anchors
                        .iter()
                        .zip(&anchors)
                        .all(|(before, now)| before.key == now.key)
                    || !self.same_chain(&first.raw_chain, &raw_chain)?)
            {
                return Err(unavailable());
            }
            if let Some(first) = baseline.as_ref() {
                // Keep validating the original retained process handles too:
                // matching numeric HWND/PID keys cannot authorize PID reuse.
                self.validate_path(&first.anchors)?;
            }
        }
        self.check()?;
        self.stage("document-value-pattern");
        // Chromium exposes document URL through ValuePattern; generic Value on
        // an Edit/Hyperlink instead denotes user text or a link target.
        // ui/accessibility/platform/ax_platform_node_delegate_utils_win.cc
        let pattern: IUIAutomationValuePattern =
            self.rpc(unsafe { document.GetCurrentPatternAs(UIA_ValuePatternId) })?;
        self.validate_path(&anchors)?;
        self.check()?;
        self.stage("document-value-read");
        let value = self.rpc(unsafe { pattern.CurrentValue() })?;
        self.validate_path(&anchors)?;
        let after = self.raw_proof(&document)?;
        self.stage("raw-ancestry-identity");
        if after.document_pid != document_pid
            || after.anchors.len() != anchors.len()
            || !after
                .anchors
                .iter()
                .zip(&anchors)
                .all(|(now, before)| now.key == before.key)
            || !self.same_chain(&raw_chain, &after.chain)?
        {
            return Err(unavailable());
        }
        self.stage("document-url-bound");
        if value.len() > 4096 {
            return Err(unavailable());
        }
        let url = value.to_string();
        if url.len() > 4096 {
            return Err(unavailable());
        }
        self.check()?;
        let first_observation = self.first_document.borrow().is_none();
        if first_observation {
            *self.first_document.borrow_mut() = Some(DocumentBinding {
                element: document,
                anchors,
                raw_chain,
            });
        }
        self.stage("observation-complete");
        // CompareElements above owns the real opaque identity comparison. This
        // token is merely local to this retained reader, never a tab identifier.
        Ok(DocumentObservation {
            pid: self.source.source_pid,
            window: self.source.source_window.ok_or_else(unavailable)?,
            identity: vec![1],
            url,
        })
    }
}

#[cfg(test)]
#[path = "windows_browser_page_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "windows_browser_real_tests.rs"]
mod real_tests;

#[cfg(test)]
mod nullable_navigation_tests {
    use super::*;

    #[test]
    fn cross_process_anchor_requires_exact_root_owner_and_executable() {
        let inside = NativeBranch {
            window: 20,
            valid: true,
            owner: 42,
            root: 10,
        };
        assert!(foreign_anchor_is_bound(10, 42, inside, true));
        assert!(!foreign_anchor_is_bound(10, 42, inside, false));
        for (source, pid, branch) in [
            (0, 42, inside),
            (10, 0, inside),
            (10, -1, inside),
            (
                10,
                42,
                NativeBranch {
                    window: 0,
                    ..inside
                },
            ),
            (
                10,
                42,
                NativeBranch {
                    valid: false,
                    ..inside
                },
            ),
            (
                10,
                42,
                NativeBranch {
                    owner: 43,
                    ..inside
                },
            ),
            (10, 42, NativeBranch { root: 0, ..inside }),
            (10, 42, NativeBranch { root: 11, ..inside }),
        ] {
            assert!(!foreign_anchor_is_bound(source, pid, branch, true));
        }
    }

    #[test]
    fn virtual_process_reuse_is_path_scoped_and_does_not_leak_to_siblings() {
        let parent: Vec<u32> = vec![];
        let mut anchored = parent.clone();
        anchored.push(42);
        let sibling = parent.clone();
        assert!(pid_is_path_bound(10, 10, parent.iter().copied()));
        assert!(pid_is_path_bound(10, 42, anchored.iter().copied()));
        assert!(!pid_is_path_bound(10, 42, sibling.iter().copied()));
        assert!(!pid_is_path_bound(10, 43, anchored.iter().copied()));
        assert!(!pid_is_path_bound(10, 0, anchored.iter().copied()));
    }

    #[test]
    fn foreign_native_branch_pruning_requires_matching_owner_and_a_different_os_root() {
        let outside = NativeBranch {
            window: 20,
            valid: true,
            owner: 42,
            root: 21,
        };
        assert!(foreign_branch_is_outside(10, 42, outside));
        for (source, pid, branch) in [
            (0, 42, outside),
            (10, 0, outside),
            (10, -1, outside),
            (
                10,
                42,
                NativeBranch {
                    window: 0,
                    ..outside
                },
            ),
            (
                10,
                42,
                NativeBranch {
                    valid: false,
                    ..outside
                },
            ),
            (
                10,
                42,
                NativeBranch {
                    owner: 43,
                    ..outside
                },
            ),
            (
                10,
                42,
                NativeBranch {
                    owner: 0,
                    ..outside
                },
            ),
            (10, 42, NativeBranch { root: 0, ..outside }),
            (
                10,
                42,
                NativeBranch {
                    root: 10,
                    ..outside
                },
            ),
        ] {
            assert!(!foreign_branch_is_outside(source, pid, branch));
        }
    }

    #[test]
    fn successful_null_navigation_is_end_but_failed_hresult_is_preserved() {
        let calls = std::cell::Cell::new(0);
        for status in [
            HRESULT(0),
            HRESULT(0x80004003u32 as i32),
            HRESULT(0x80131505u32 as i32),
        ] {
            let result = unsafe {
                nullable_automation_element(|output| {
                    calls.set(calls.get() + 1);
                    assert!((*output).is_null());
                    *output = std::ptr::null_mut();
                    status
                })
            };
            if status.is_ok() {
                assert!(result.unwrap().is_none());
            } else {
                assert_eq!(result.unwrap_err().code(), status);
            }
        }
        assert_eq!(calls.get(), 3);
    }
}
