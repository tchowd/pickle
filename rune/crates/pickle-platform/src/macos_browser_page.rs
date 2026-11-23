use super::*;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn pickle_browser_page_start(
        input: *const c_char,
        callback: extern "C" fn(*mut c_void, *mut c_char),
        context: *mut c_void,
    ) -> *mut c_void;
    fn pickle_browser_page_release(handle: *mut c_void);
    fn pickle_string_free(value: *mut c_char);
}

pub(super) fn capture(
    source: &CapturedSelection,
    expected: &str,
    cancel: CancellationToken,
) -> Result<CapturedPage, PlatformError> {
    let input =
        CString::new(serde_json::json!({"source":source,"documentURL":expected}).to_string())
            .map_err(|_| unavailable())?;
    let (sender, receiver) =
        std::sync::mpsc::sync_channel::<Result<CapturedPage, PlatformError>>(1);
    extern "C" fn complete(context: *mut c_void, value: *mut c_char) {
        let sender = unsafe {
            Box::from_raw(
                context.cast::<std::sync::mpsc::SyncSender<Result<CapturedPage, PlatformError>>>(),
            )
        };
        let result = (|| {
            if value.is_null() {
                return Err(unavailable());
            }
            let bytes = unsafe { CStr::from_ptr(value).to_bytes().to_vec() };
            unsafe { pickle_string_free(value) };
            if bytes.len() > 4_000_000 {
                return Err(unavailable());
            }
            let envelope: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
            if envelope["ok"] != true {
                return Err(unavailable());
            }
            serde_json::from_value(envelope["value"].clone()).map_err(|_| unavailable())
        })();
        let _ = sender.send(result);
    }
    let handle = unsafe {
        pickle_browser_page_start(
            input.as_ptr(),
            complete,
            Box::into_raw(Box::new(sender)).cast(),
        )
    };
    struct Operation(*mut c_void);
    impl Drop for Operation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { pickle_browser_page_release(self.0) }
            }
        }
    }
    let _operation = Operation(handle);
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
