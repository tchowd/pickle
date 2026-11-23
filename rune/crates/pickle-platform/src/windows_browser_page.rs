//! URL reads are confined to Chromium's top-level UIA document, never the
//! omnibox, links, iframe documents, names, or page text. All COM stays on MTA.
use super::*;
use ::windows::{
    Win32::{
        Foundation::{HWND, VARIANT_TRUE},
        System::{
            Com::*,
            Variant::{VT_BOOL, VariantClear},
        },
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
    core::{GUID, HRESULT, Interface, w},
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    time::{Duration, Instant},
};

// Chromium documents this Boolean as true only on the top-level tab Document,
// explicitly false on child-frame documents. No window-class/name heuristic.
// https://chromium.googlesource.com/chromium/src.git/+/refs/heads/main/docs/accessibility/browser/custom_uia_properties.md
const WEB_ROOT: GUID = GUID::from_u128(0xc5fdc049_4eec_4299_87bb_ca9fc718a681);
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
    property: UIA_PROPERTY_ID,
    first_document: RefCell<Option<IUIAutomationElement>>,
    deadline: Instant,
    cancel: CancellationToken,
}
impl DocumentReader {
    fn new(source: &CapturedSelection, cancel: CancellationToken) -> Result<Self, PlatformError> {
        let deadline = Instant::now() + Duration::from_millis(1800);
        crate::windows_impl::validate_source_window(source)?;
        let ui: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| unavailable())?;
        let settings: IUIAutomation2 = ui.cast().map_err(|_| unavailable())?;
        unsafe { settings.SetConnectionTimeout(80) }.map_err(|_| unavailable())?;
        unsafe { settings.SetTransactionTimeout(80) }.map_err(|_| unavailable())?;
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
        Ok(Self {
            source: source.clone(),
            ui,
            settings,
            root,
            walker,
            property: UIA_PROPERTY_ID(property),
            first_document: RefCell::new(None),
            deadline,
            cancel,
        })
    }
    fn check(&self) -> Result<(), PlatformError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if self.cancel.is_cancelled() || remaining.is_zero() {
            return Err(unavailable());
        }
        let millis = remaining.as_millis().clamp(1, 80) as u32;
        unsafe { self.settings.SetConnectionTimeout(millis) }.map_err(|_| unavailable())?;
        unsafe { self.settings.SetTransactionTimeout(millis) }.map_err(|_| unavailable())?;
        Ok(())
    }
    fn same(
        &self,
        left: &IUIAutomationElement,
        right: &IUIAutomationElement,
    ) -> Result<bool, PlatformError> {
        self.check()?;
        unsafe { self.ui.CompareElements(left, right) }
            .map(|same| same.as_bool())
            .map_err(|_| unavailable())
    }
    fn next(
        &self,
        value: ::windows::core::Result<IUIAutomationElement>,
    ) -> Result<Option<IUIAutomationElement>, PlatformError> {
        match value {
            Ok(element) => Ok(Some(element)),
            Err(error) if error.code() == HRESULT(0x80004003u32 as i32) => Ok(None),
            Err(_) => Err(unavailable()),
        }
    }
    fn web_root(&self, element: &IUIAutomationElement) -> Result<bool, PlatformError> {
        self.check()?;
        let mut value = unsafe { element.GetCurrentPropertyValueEx(self.property, true) }
            .map_err(|_| unavailable())?;
        let supported = unsafe {
            value.Anonymous.Anonymous.vt == VT_BOOL
                && value.Anonymous.Anonymous.Anonymous.boolVal == VARIANT_TRUE
        };
        unsafe { VariantClear(&mut value) }.map_err(|_| unavailable())?;
        Ok(supported)
    }
    fn observe(&self) -> Result<DocumentObservation, PlatformError> {
        self.check()?;
        crate::windows_impl::validate_source_window(&self.source)?;
        let hwnd = HWND(self.source.source_window.ok_or_else(unavailable)? as *mut _);
        if unsafe { GetForegroundWindow() } != hwnd || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return Err(unavailable());
        }
        let fresh = unsafe { self.ui.ElementFromHandle(hwnd) }.map_err(|_| unavailable())?;
        if !self.same(&self.root, &fresh)? {
            return Err(unavailable());
        }
        self.check()?;
        if unsafe { fresh.CurrentProcessId() }.map_err(|_| unavailable())? as u32
            != self.source.source_pid
        {
            return Err(unavailable());
        }
        // A secure focused field is refused without reading any field value.
        self.check()?;
        let mut focused = unsafe { self.ui.GetFocusedElement() }.map_err(|_| unavailable())?;
        let mut reached = false;
        for _ in 0..16 {
            self.check()?;
            if unsafe { focused.CurrentIsPassword() }
                .map_err(|_| unavailable())?
                .as_bool()
            {
                return Err(unavailable());
            }
            if self.same(&focused, &fresh)? {
                reached = true;
                break;
            }
            self.check()?;
            focused =
                unsafe { self.walker.GetParentElement(&focused) }.map_err(|_| unavailable())?;
        }
        if !reached {
            return Err(unavailable());
        }
        let mut queue = VecDeque::from([(fresh, 0usize)]);
        let mut count = 1usize;
        let mut documents = Vec::new();
        while let Some((node, depth)) = queue.pop_front() {
            if depth > 12 || count > 192 {
                return Err(unavailable());
            }
            self.check()?;
            if unsafe { node.CurrentProcessId() }.map_err(|_| unavailable())? as u32
                != self.source.source_pid
            {
                return Err(unavailable());
            }
            self.check()?;
            let control = unsafe { node.CurrentControlType() }.map_err(|_| unavailable())?;
            if control == UIA_DocumentControlTypeId {
                if self.web_root(&node)? {
                    self.check()?;
                    if !unsafe { node.CurrentIsOffscreen() }
                        .map_err(|_| unavailable())?
                        .as_bool()
                    {
                        documents.push(node);
                    }
                }
                continue; // Never traverse any document's page/iframe/link content.
            }
            if control == UIA_EditControlTypeId || control == UIA_HyperlinkControlTypeId {
                continue;
            }
            self.check()?;
            let mut child = self.next(unsafe { self.walker.GetFirstChildElement(&node) })?;
            while let Some(element) = child {
                count += 1;
                if count > 192 {
                    return Err(unavailable());
                }
                self.check()?;
                child = self.next(unsafe { self.walker.GetNextSiblingElement(&element) })?;
                queue.push_back((element, depth + 1));
            }
        }
        if documents.len() != 1 {
            return Err(unavailable());
        }
        let document = documents.pop().ok_or_else(unavailable)?;
        {
            let baseline = self.first_document.borrow();
            if let Some(first) = baseline.as_ref()
                && !self.same(first, &document)?
            {
                return Err(unavailable());
            }
        }
        self.check()?;
        // Chromium exposes document URL through ValuePattern; generic Value on
        // an Edit/Hyperlink instead denotes user text or a link target.
        // ui/accessibility/platform/ax_platform_node_delegate_utils_win.cc
        let pattern: IUIAutomationValuePattern =
            unsafe { document.GetCurrentPatternAs(UIA_ValuePatternId) }
                .map_err(|_| unavailable())?;
        self.check()?;
        let value = unsafe { pattern.CurrentValue() }.map_err(|_| unavailable())?;
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
            *self.first_document.borrow_mut() = Some(document);
        }
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
