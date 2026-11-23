use super::*;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    marker::PhantomData,
    rc::Rc,
};

unsafe extern "C" {
    fn pickle_string_free(value: *mut c_char);
    fn pickle_capture_selection(exclusions: *const c_char) -> *mut c_char;
    fn pickle_permission_status() -> *mut c_char;
    fn pickle_screen_work_areas() -> *mut c_char;
    fn pickle_window_work_area(window_number: u32, fallback_x: i32, fallback_y: i32)
    -> *mut c_char;
    fn pickle_configure_panel_drag(window_number: u32) -> *mut c_char;
    fn pickle_request_accessibility();
    fn pickle_request_screen_capture();
    fn pickle_credential_get(account: *const c_char, interactive: i32) -> *mut c_char;
    fn pickle_credential_set(account: *const c_char, token: *const c_char) -> *mut c_char;
    fn pickle_shell_install(
        callback: extern "C" fn(*mut c_void, i32),
        context: *mut c_void,
    ) -> *mut c_void;
    fn pickle_shell_pause(handle: *mut c_void, paused: i32) -> *mut c_char;
    fn pickle_shell_configure(handle: *mut c_void, config: *const c_char) -> *mut c_char;
    fn pickle_shell_remove(handle: *mut c_void);
    fn pickle_capture_page_start(
        input: *const c_char,
        callback: extern "C" fn(*mut c_void, *mut c_char),
        context: *mut c_void,
    ) -> *mut c_void;
    fn pickle_capture_page_release(handle: *mut c_void);
    fn pickle_audio_start(
        input: *const c_char,
        callback: extern "C" fn(*mut c_void, *mut c_char),
        context: *mut c_void,
    ) -> *mut c_void;
    fn pickle_audio_release(handle: *mut c_void);
}

fn cstring(value: &str) -> Result<CString, PlatformError> {
    CString::new(value)
        .map_err(|_| PlatformError("A native setting contains an invalid null character.".into()))
}
fn decode<T: serde::de::DeserializeOwned>(raw: *mut c_char) -> Result<T, PlatformError> {
    if raw.is_null() {
        return Err(PlatformError(
            "The native service did not return a result.".into(),
        ));
    }
    // SAFETY: bridge returns an owned null-terminated UTF-8 allocation, freed exactly once here.
    let data = unsafe { CStr::from_ptr(raw).to_bytes().to_vec() };
    unsafe { pickle_string_free(raw) };
    parse_response(&data)
}
fn parse_response<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, PlatformError> {
    if data.len() > 4_000_000 {
        return Err(PlatformError(
            "Native response exceeded its size limit.".into(),
        ));
    }
    #[derive(Deserialize)]
    struct Envelope {
        ok: bool,
        #[serde(default)]
        value: serde_json::Value,
        error: Option<String>,
    }
    let envelope: Envelope = serde_json::from_slice(data)
        .map_err(|_| PlatformError("The native service returned an invalid result.".into()))?;
    if envelope.ok {
        serde_json::from_value(envelope.value).map_err(|_| {
            PlatformError("The native service returned an invalid result type.".into())
        })
    } else {
        Err(PlatformError(
            envelope
                .error
                .unwrap_or_else(|| "The native operation failed.".into()),
        ))
    }
}
pub fn capabilities() -> Capabilities {
    Capabilities {
        selection: true,
        menu_bar: true,
        modifier_gesture: true,
        window_capture_ocr: true,
        source_audio: true,
    }
}

/// Install the missing pinned-Rune source-window drag forward for the actual
/// adopted panel. Call on_ready on the UI thread; retains no raw view pointer.
pub fn configure_panel_drag(window_number: Option<u32>) -> Result<(), PlatformError> {
    decode::<bool>(unsafe { pickle_configure_panel_drag(window_number.unwrap_or(0)) }).map(|_| ())
}

/// Main-thread query using the actual panel's WindowHandle::window_number.
/// Ambiguous mixed-DPI point fallbacks return None instead of guessing a scale.
pub fn window_work_area(
    window_number: Option<u32>,
    fallback_x: i32,
    fallback_y: i32,
) -> Option<WorkArea> {
    #[derive(Deserialize)]
    struct Area {
        visible_x: f64,
        visible_y: f64,
        width: f64,
        height: f64,
        primary_height: f64,
        scale: f64,
    }
    let native: Option<Area> = decode(unsafe {
        pickle_window_work_area(window_number.unwrap_or(0), fallback_x, fallback_y)
    })
    .ok()?;
    let area = native?;
    super::geometry::appkit_work_area(
        area.visible_x,
        area.visible_y,
        area.width,
        area.height,
        area.primary_height,
        area.scale,
    )
}
/// UI-thread restoration query. On macOS the caller must rescale the saved
/// global origin by `current_scale / saved_scale` before clamping it to this area.
pub fn restore_work_area(x: i32, y: i32, saved_scale: f64) -> Option<(WorkArea, f64)> {
    #[derive(Deserialize)]
    struct Layout {
        primary_height: f64,
        screens: Vec<super::geometry::AppKitScreen>,
    }
    let layout: Option<Layout> = decode(unsafe { pickle_screen_work_areas() }).ok()?;
    let layout = layout?;
    super::geometry::restore_appkit_work_area(
        x,
        y,
        saved_scale,
        layout.primary_height,
        &layout.screens,
    )
}
/// Must run on Rune's main/UI thread before the chooser receives focus.
pub fn capture_selection(exclusions: &[String]) -> Result<CapturedSelection, PlatformError> {
    let encoded = cstring(
        &serde_json::to_string(exclusions)
            .map_err(|_| PlatformError("Invalid exclusions.".into()))?,
    )?;
    let value = decode(unsafe { pickle_capture_selection(encoded.as_ptr()) })?;
    validate_selection(value, exclusions)
}
pub fn permission_status() -> PermissionStatus {
    decode(unsafe { pickle_permission_status() }).unwrap_or(PermissionStatus {
        accessibility: false,
        screen_capture: false,
    })
}
pub fn request_accessibility() {
    unsafe { pickle_request_accessibility() }
}
pub fn request_screen_capture() {
    unsafe { pickle_request_screen_capture() }
}
pub fn credential_get(account: &str, interactive: bool) -> Result<Option<String>, PlatformError> {
    let account = cstring(account)?;
    // A JSON null is a successful absent credential, unlike a missing native result.
    let raw = unsafe { pickle_credential_get(account.as_ptr(), i32::from(interactive)) };
    let value: serde_json::Value = decode(raw)?;
    if value.is_null() {
        Ok(None)
    } else {
        value
            .as_str()
            .map(|s| Some(s.to_owned()))
            .ok_or_else(|| PlatformError("Invalid credential response.".into()))
    }
}
pub fn credential_set(account: &str, token: &str) -> Result<(), PlatformError> {
    if token.len() > 20_000 {
        return Err(PlatformError("The credential is too long.".into()));
    }
    let account = cstring(account)?;
    let token = cstring(token)?;
    decode::<bool>(unsafe { pickle_credential_set(account.as_ptr(), token.as_ptr()) }).map(|_| ())
}
pub fn credential_remove(account: &str) -> Result<(), PlatformError> {
    credential_set(account, "")
}

struct Callback {
    invoke: Box<dyn Fn(PlatformEvent) + Send + Sync>,
    chord: std::sync::Mutex<super::invocation::ModifierChord>,
}
extern "C" fn shell_callback(context: *mut c_void, event: i32) {
    if context.is_null() {
        return;
    }
    // Raw flags/interruption events are handled by the same tested Rust recognizer.
    if (100..108).contains(&event) || (200..208).contains(&event) {
        let callback = unsafe { &*(context as *const Callback) };
        let flags = event % 100;
        let invoke = callback
            .chord
            .lock()
            .map(|mut chord| {
                if event >= 200 {
                    chord.interrupt(flags & 1 != 0, flags & 2 != 0);
                    false
                } else {
                    chord.flags(flags & 1 != 0, flags & 2 != 0, flags & 4 != 0)
                }
            })
            .unwrap_or(false);
        if invoke {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (callback.invoke)(PlatformEvent::Invoke)
            }));
        }
        return;
    }
    let event = match event {
        0 => PlatformEvent::Invoke,
        1 => PlatformEvent::Reopen,
        2 => PlatformEvent::Paste,
        3 => PlatformEvent::Settings,
        4 => PlatformEvent::History,
        5 => PlatformEvent::TogglePause,
        6 => PlatformEvent::Quit,
        7 => PlatformEvent::Dismiss,
        8 => PlatformEvent::AutomaticInvoke,
        9 => PlatformEvent::HideChooser,
        _ => return,
    };
    // No unwind may cross the C boundary, even if app code panics.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        ((&*(context as *const Callback)).invoke)(event)
    }));
}
/// UI-thread-owned native observers/status item. It cannot be sent to another thread.
pub struct NativeShell {
    handle: *mut c_void,
    callback: *mut Callback,
    _main: PhantomData<Rc<()>>,
}
impl NativeShell {
    pub fn install(
        callback: impl Fn(PlatformEvent) + Send + Sync + 'static,
    ) -> Result<Self, PlatformError> {
        let callback = Box::into_raw(Box::new(Callback {
            invoke: Box::new(callback),
            chord: Default::default(),
        }));
        let handle = unsafe { pickle_shell_install(shell_callback, callback.cast()) };
        if handle.is_null() {
            unsafe { drop(Box::from_raw(callback)) };
            return Err(PlatformError("Could not create the menu-bar item.".into()));
        }
        Ok(Self {
            handle,
            callback,
            _main: PhantomData,
        })
    }
    pub fn set_paused(&self, paused: bool) -> Result<(), PlatformError> {
        if let Ok(mut chord) = unsafe { &*self.callback }.chord.lock() {
            chord.reset()
        }
        decode::<bool>(unsafe { pickle_shell_pause(self.handle, i32::from(paused)) }).map(|_| ())
    }
    pub fn configure_invocation(&self, config: InvocationConfig) -> Result<(), PlatformError> {
        let config = config.normalized()?;
        let encoded = cstring(
            &serde_json::to_string(&config)
                .map_err(|_| PlatformError("Invocation settings could not be encoded.".into()))?,
        )?;
        decode::<bool>(unsafe { pickle_shell_configure(self.handle, encoded.as_ptr()) })?;
        if let Ok(mut chord) = unsafe { &*self.callback }.chord.lock() {
            chord.reset()
        }
        Ok(())
    }
}
impl Drop for NativeShell {
    fn drop(&mut self) {
        unsafe {
            pickle_shell_remove(self.handle);
            drop(Box::from_raw(self.callback))
        }
    }
}

/// Blocking caller facade: call from a worker, with Rune's native event loop running.
/// The caller's timeout does not free callback memory until Swift completes.
pub fn capture_page(source: &CapturedSelection) -> Result<CapturedPage, PlatformError> {
    capture_page_with_cancel(source, tokio_util::sync::CancellationToken::new())
}
pub fn capture_page_with_cancel(
    source: &CapturedSelection,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<CapturedPage, PlatformError> {
    if cancel.is_cancelled() {
        return Err(PlatformError("Page capture was cancelled.".into()));
    }
    let encoded = cstring(
        &serde_json::to_string(source)
            .map_err(|_| PlatformError("Invalid source target.".into()))?,
    )?;
    let (sender, receiver) =
        std::sync::mpsc::sync_channel::<Result<CapturedPage, PlatformError>>(1);
    extern "C" fn complete(context: *mut c_void, value: *mut c_char) {
        // SAFETY: exactly one callback consumes the sender allocated by capture_page.
        let sender = unsafe {
            Box::from_raw(
                context as *mut std::sync::mpsc::SyncSender<Result<CapturedPage, PlatformError>>,
            )
        };
        let result = decode(value);
        let _ = sender.send(result);
    }
    let native = unsafe {
        pickle_capture_page_start(
            encoded.as_ptr(),
            complete,
            Box::into_raw(Box::new(sender)).cast(),
        )
    };
    struct Operation(*mut c_void);
    impl Drop for Operation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { pickle_capture_page_release(self.0) }
            }
        }
    }
    let _operation = Operation(native);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if cancel.is_cancelled() {
            return Err(PlatformError("Page capture was cancelled.".into()));
        }
        if std::time::Instant::now() >= deadline {
            return Err(PlatformError(
                "Page capture timed out. Continue with the selected passage.".into(),
            ));
        }
        match receiver.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(PlatformError("Page capture ended without a result.".into())),
        }
    }
}

/// Records source-application audio for up to 30 seconds and transcribes on-device.
/// Call on a worker while Rune's UI loop runs; cancellation stops the native stream.
pub fn record_audio(
    source: &CapturedSelection,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<String, PlatformError> {
    if cancel.is_cancelled() {
        return Err(PlatformError("Listening was cancelled.".into()));
    }
    let encoded = cstring(
        &serde_json::to_string(source)
            .map_err(|_| PlatformError("Invalid source target.".into()))?,
    )?;
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Result<String, PlatformError>>(1);
    extern "C" fn complete(context: *mut c_void, value: *mut c_char) {
        let sender = unsafe {
            Box::from_raw(
                context as *mut std::sync::mpsc::SyncSender<Result<String, PlatformError>>,
            )
        };
        let _ = sender.send(decode(value));
    }
    let native = unsafe {
        pickle_audio_start(
            encoded.as_ptr(),
            complete,
            Box::into_raw(Box::new(sender)).cast(),
        )
    };
    struct Operation(*mut c_void);
    impl Drop for Operation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { pickle_audio_release(self.0) }
            }
        }
    }
    let _operation = Operation(native);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    loop {
        if cancel.is_cancelled() {
            return Err(PlatformError("Listening was cancelled.".into()));
        }
        if std::time::Instant::now() >= deadline {
            return Err(PlatformError(
                "Listening timed out. Use captions or paste a transcript.".into(),
            ));
        }
        match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                return Err(PlatformError(
                    "Audio capture ended without a transcript.".into(),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_permissions_round_trip_without_requesting_access() {
        let status: PermissionStatus = decode(unsafe { pickle_permission_status() })
            .expect("Swift permission envelope must decode successfully");
        let encoded = serde_json::to_value(status).unwrap();
        assert!(encoded["accessibility"].is_boolean());
        assert!(encoded["screen_capture"].is_boolean());
    }
    #[test]
    fn absent_credentials_and_native_failures_are_distinct() {
        let empty: Option<String> = parse_response(br#"{"ok":true,"value":null}"#).unwrap();
        assert_eq!(empty, None);
        let denied = parse_response::<String>(br#"{"ok":false,"error":"Authorization required"}"#)
            .unwrap_err();
        assert_eq!(denied.0, "Authorization required");
    }
    #[test]
    fn native_modifier_callback_routes_only_clean_release_to_invocation() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let invoked = Arc::new(AtomicUsize::new(0));
        let automatic = Arc::new(AtomicUsize::new(0));
        let n = invoked.clone();
        let a = automatic.clone();
        let callback = Callback {
            invoke: Box::new(move |event| {
                if event == PlatformEvent::Invoke {
                    n.fetch_add(1, Ordering::Relaxed);
                }
                if event == PlatformEvent::AutomaticInvoke {
                    a.fetch_add(1, Ordering::Relaxed);
                }
            }),
            chord: Default::default(),
        };
        let context = (&callback as *const Callback).cast_mut().cast();
        shell_callback(context, 103);
        shell_callback(context, 100);
        shell_callback(context, 100);
        assert_eq!(invoked.load(Ordering::Relaxed), 1);
        shell_callback(context, 103);
        shell_callback(context, 203);
        shell_callback(context, 100);
        assert_eq!(invoked.load(Ordering::Relaxed), 1);
        shell_callback(context, 8);
        assert_eq!(automatic.load(Ordering::Relaxed), 1);
    }
}
