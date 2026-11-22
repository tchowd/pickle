use super::{
    invocation::{AutoDebounce, RequestGate, Shortcut},
    *,
};
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
        UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

const TRAY_EVENT: u32 = WM_APP + 1;
const CONFIGURE: u32 = WM_APP + 2;
const MOUSE_DOWN: u32 = WM_APP + 3;
const MOUSE_UP: u32 = WM_APP + 4;
const AUTO_TIMER: usize = 73;
thread_local! {static MOUSE_WINDOW:Cell<isize>=const{Cell::new(0)};static MOUSE_REVISION:Cell<usize>=const{Cell::new(0)};}
fn error(message: &str) -> PlatformError {
    PlatformError(message.into())
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
enum Command {
    Configure(InvocationConfig),
    Pause(bool),
}
struct Request {
    command: Command,
    gate: Arc<RequestGate>,
    reply: SyncSender<Result<(), PlatformError>>,
}
struct InvocationState {
    config: InvocationConfig,
    hotkey: i32,
    last_hotkey_id: u32,
    registered: bool,
    hook: Option<HHOOK>,
    automatic: AutoDebounce,
    clock: Instant,
    revision: usize,
}
impl Default for InvocationState {
    fn default() -> Self {
        Self {
            config: InvocationConfig::default(),
            hotkey: 0,
            last_hotkey_id: 0,
            registered: false,
            hook: None,
            automatic: Default::default(),
            clock: Instant::now(),
            revision: 0,
        }
    }
}
impl InvocationState {
    fn now(&self) -> u64 {
        self.clock.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    fn cancel_automatic(&mut self, window: HWND) {
        self.automatic.down();
        let _ = unsafe { KillTimer(Some(window), AUTO_TIMER) };
    }
    fn stop(&mut self, window: HWND) {
        self.revision = self.revision.wrapping_add(1);
        MOUSE_REVISION.set(self.revision);
        self.cancel_automatic(window);
        if self.registered {
            let _ = unsafe { UnregisterHotKey(Some(window), self.hotkey) };
            self.registered = false
        }
        MOUSE_WINDOW.set(0);
        if let Some(hook) = self.hook.take() {
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
    }
    fn configure(
        &mut self,
        window: HWND,
        config: InvocationConfig,
        active: bool,
        commit: impl FnOnce() -> bool,
    ) -> Result<(), PlatformError> {
        let config = config.normalized()?;
        if config.modifier_gesture {
            return Err(error(
                "Modifier-only invocation is not supported on Windows. Choose a conventional shortcut.",
            ));
        }
        let shortcut = config.parsed()?;
        let (modifiers, key) = windows_shortcut(&shortcut)?;
        let keep = self.registered && active && config.shortcut == self.config.shortcut;
        let next = if keep {
            self.hotkey
        } else if active {
            // Windows can leave WM_HOTKEY messages queued after unregistering.
            // Never recycle an ID during this shell's lifetime, including a
            // candidate rolled back after mouse-hook installation failed.
            self.last_hotkey_id =
                super::invocation::next_registration_id(self.last_hotkey_id, 0xBFFF)?;
            self.last_hotkey_id as i32
        } else {
            self.hotkey
        };
        let mut registered = false;
        if active && !keep {
            unsafe{RegisterHotKey(Some(window),next,modifiers|MOD_NOREPEAT,key)}.map_err(|_|error("This shortcut is already in use or unavailable. Choose a different combination."))?;
            registered = true;
        }
        let mut next_hook = None;
        if active && config.automatic_menu && self.hook.is_none() {
            let install = unsafe {
                GetModuleHandleW(None).and_then(|module| {
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), Some(module.into()), 0)
                })
            };
            match install {
                Ok(hook) => next_hook = Some(hook),
                Err(_) => {
                    if registered {
                        let _ = unsafe { UnregisterHotKey(Some(window), next) };
                    }
                    return Err(error(
                        "Automatic selection monitoring could not start. Use the shortcut or retry.",
                    ));
                }
            }
        }
        if !commit() {
            if let Some(hook) = next_hook {
                let _ = unsafe { UnhookWindowsHookEx(hook) };
            }
            if registered {
                let _ = unsafe { UnregisterHotKey(Some(window), next) };
            }
            return Err(error(
                "The invocation settings request was cancelled before being applied.",
            ));
        }
        if !keep && self.registered {
            let _ = unsafe { UnregisterHotKey(Some(window), self.hotkey) };
        }
        self.registered = active;
        self.hotkey = next;
        if let Some(hook) = next_hook {
            self.hook = Some(hook);
            MOUSE_WINDOW.set(window.0 as isize);
        }
        if !active || !config.automatic_menu {
            MOUSE_WINDOW.set(0);
            if let Some(hook) = self.hook.take() {
                let _ = unsafe { UnhookWindowsHookEx(hook) };
            }
        }
        self.cancel_automatic(window);
        self.config = config;
        self.revision = self.revision.wrapping_add(1);
        MOUSE_REVISION.set(self.revision);
        Ok(())
    }
}

fn windows_shortcut(shortcut: &Shortcut) -> Result<(HOT_KEY_MODIFIERS, u32), PlatformError> {
    let mut mods = HOT_KEY_MODIFIERS(0);
    for (mask, native) in [(1, MOD_CONTROL), (2, MOD_ALT), (4, MOD_SHIFT), (8, MOD_WIN)] {
        if shortcut.mods & mask != 0 {
            mods |= native
        }
    }
    let key = if shortcut.key.len() == 1 {
        shortcut.key.as_bytes()[0].to_ascii_uppercase() as u32
    } else if let Some(number) = shortcut
        .key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u32>().ok())
    {
        0x70 + number - 1
    } else {
        match shortcut.key.as_str() {
            "space" => 0x20,
            "return" => 0x0D,
            "tab" => 0x09,
            "escape" => 0x1B,
            "semicolon" => 0xBA,
            "equal" => 0xBB,
            "comma" => 0xBC,
            "minus" => 0xBD,
            "period" => 0xBE,
            "slash" => 0xBF,
            "backquote" => 0xC0,
            "leftbracket" => 0xDB,
            "backslash" => 0xDC,
            "rightbracket" => 0xDD,
            "quote" => 0xDE,
            _ => return Err(error("This shortcut key is not supported on Windows.")),
        }
    };
    Ok((mods, key))
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let message = wparam.0 as u32;
        if code >= 0
            && [WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN].contains(&message)
            && lparam.0 != 0
        {
            let mouse = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let mut pid = 0;
            GetWindowThreadProcessId(WindowFromPoint(mouse.pt), Some(&mut pid));
            // The global monitor excludes Pickle's own controls, matching AppKit's
            // global-only mouse monitor. Never swallow or replay input.
            if pid != 0 && pid != GetCurrentProcessId() {
                let target = MOUSE_WINDOW.get();
                if target != 0 {
                    let _ = PostMessageW(
                        Some(HWND(target as *mut _)),
                        if message == WM_LBUTTONUP {
                            MOUSE_UP
                        } else {
                            MOUSE_DOWN
                        },
                        WPARAM(MOUSE_REVISION.get()),
                        LPARAM(0),
                    );
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }
}

struct ShellState {
    callback: Arc<dyn Fn(PlatformEvent) + Send + Sync>,
    callback_active: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    taskbar_created: u32,
    invocation: RefCell<InvocationState>,
    requests: Receiver<Request>,
}
fn emit(state: &ShellState, event: PlatformEvent) {
    if matches!(
        event,
        PlatformEvent::Invoke | PlatformEvent::AutomaticInvoke
    ) {
        if state.paused.load(Ordering::Acquire)
            || state.callback_active.swap(true, Ordering::AcqRel)
        {
            return;
        }
        let active = state.callback_active.clone();
        let reset = active.clone();
        let callback = state.callback.clone();
        let worker = std::thread::Builder::new()
            .name("pickle-invocation".into())
            .spawn(move || {
                struct Guard(Arc<AtomicBool>);
                impl Drop for Guard {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release)
                    }
                }
                let _active = Guard(active);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(event)));
            });
        if worker.is_err() {
            reset.store(false, Ordering::Release)
        }
    } else {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.callback)(event)));
    }
}
unsafe fn tray_data(window: HWND) -> NOTIFYICONDATAW {
    unsafe {
        let icon = GetModuleHandleW(None)
            .and_then(|module| {
                LoadIconW(Some(module.into()), PCWSTR(std::ptr::without_provenance(1)))
            })
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default();
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_EVENT,
            hIcon: icon,
            ..Default::default()
        };
        let tip: Vec<u16> = "Pickle — reading assistant".encode_utf16().collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        data
    }
}
unsafe fn configure_requests(state: &ShellState, window: HWND) {
    while let Ok(request) = state.requests.try_recv() {
        if !request.gate.pending() {
            continue;
        }
        let result = match request.command {
            Command::Configure(config) => state.invocation.borrow_mut().configure(
                window,
                config,
                !state.paused.load(Ordering::Acquire),
                || request.gate.commit(),
            ),
            Command::Pause(paused) => {
                if paused {
                    if !request.gate.commit() {
                        continue;
                    }
                    state.paused.store(true, Ordering::Release);
                    state.invocation.borrow_mut().stop(window);
                    Ok(())
                } else {
                    let config = state.invocation.borrow().config.clone();
                    let result =
                        state
                            .invocation
                            .borrow_mut()
                            .configure(window, config, true, || request.gate.commit());
                    if result.is_ok() {
                        state.paused.store(false, Ordering::Release)
                    }
                    result
                }
            }
        };
        let _ = request.reply.send(result);
    }
}
unsafe fn tray_menu(state: &ShellState, window: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        for (id, title) in [
            (1, "Open Pickle"),
            (2, "Paste a passage"),
            (3, "Settings…"),
            (4, "Saved answers…"),
            (5, "Pause or resume"),
            (6, "Quit Pickle"),
        ] {
            let title = wide(title);
            let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(title.as_ptr()));
        }
        let mut point = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(window);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            window,
            None,
        )
        .0;
        let _ = DestroyMenu(menu);
        let event = match selected {
            1 => Some(PlatformEvent::Reopen),
            2 => Some(PlatformEvent::Paste),
            3 => Some(PlatformEvent::Settings),
            4 => Some(PlatformEvent::History),
            5 => Some(PlatformEvent::TogglePause),
            6 => Some(PlatformEvent::Quit),
            _ => None,
        };
        if let Some(event) = event {
            emit(state, event)
        }
    }
}
unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *const ShellState;
        if !pointer.is_null() {
            let state = &*pointer;
            if message == state.taskbar_created {
                let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(window));
                return LRESULT(0);
            }
            if message == CONFIGURE {
                configure_requests(state, window);
                return LRESULT(0);
            }
            if message == WM_HOTKEY && !state.paused.load(Ordering::Acquire) {
                let mut invocation = state.invocation.borrow_mut();
                let current = invocation.registered && wparam.0 == invocation.hotkey as usize;
                invocation.cancel_automatic(window);
                drop(invocation);
                if current {
                    emit(state, PlatformEvent::Invoke)
                }
                return LRESULT(0);
            }
            if [MOUSE_DOWN, MOUSE_UP].contains(&message) {
                let mut invocation = state.invocation.borrow_mut();
                if invocation.config.automatic_menu
                    && !state.paused.load(Ordering::Acquire)
                    && wparam.0 == invocation.revision
                {
                    if message == MOUSE_DOWN {
                        invocation.cancel_automatic(window);
                        drop(invocation);
                        emit(state, PlatformEvent::HideChooser)
                    } else {
                        let now = invocation.now();
                        invocation.automatic.up(now);
                        SetTimer(
                            Some(window),
                            AUTO_TIMER,
                            AutoDebounce::DELAY_MS as u32,
                            None,
                        );
                    }
                }
                return LRESULT(0);
            }
            if message == WM_TIMER && wparam.0 == AUTO_TIMER {
                let mut invocation = state.invocation.borrow_mut();
                if invocation.config.automatic_menu && !state.paused.load(Ordering::Acquire) {
                    let now = invocation.now();
                    if invocation.automatic.poll(now) {
                        let _ = KillTimer(Some(window), AUTO_TIMER);
                        drop(invocation);
                        emit(state, PlatformEvent::AutomaticInvoke)
                    } else if let Some(remaining) = invocation.automatic.remaining(now) {
                        SetTimer(
                            Some(window),
                            AUTO_TIMER,
                            remaining.clamp(1, u32::MAX as u64) as u32,
                            None,
                        );
                    }
                } else {
                    invocation.cancel_automatic(window)
                }
                return LRESULT(0);
            }
            if message == TRAY_EVENT {
                if lparam.0 as u32 == WM_LBUTTONUP {
                    emit(state, PlatformEvent::Reopen)
                }
                if [WM_RBUTTONUP, WM_CONTEXTMENU].contains(&(lparam.0 as u32)) {
                    tray_menu(state, window)
                }
                return LRESULT(0);
            }
            if message == WM_CLOSE || message == WM_DESTROY {
                state.invocation.borrow_mut().stop(window);
            }
        }
        if message == WM_CLOSE {
            let _ = DestroyWindow(window);
            return LRESULT(0);
        }
        if message == WM_DESTROY {
            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(window));
            PostQuitMessage(0);
            return LRESULT(0);
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

pub struct NativeShell {
    window: usize,
    commands: Sender<Request>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl NativeShell {
    pub fn install(
        callback: impl Fn(PlatformEvent) + Send + Sync + 'static,
    ) -> Result<Self, PlatformError> {
        let callback = Arc::new(callback);
        let (commands, requests) = mpsc::channel();
        let (ready, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("pickle-desktop-shell".into())
            .spawn(move || unsafe {
                let run = || -> Result<(), PlatformError> {
                    let module = GetModuleHandleW(None)
                        .map_err(|_| error("Could not start the Windows desktop shell."))?;
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(window_proc),
                        hInstance: module.into(),
                        lpszClassName: w!("PickleRuneTray"),
                        ..Default::default()
                    };
                    let _ = RegisterClassW(&class);
                    let window = CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        w!("PickleRuneTray"),
                        w!("Pickle"),
                        WINDOW_STYLE::default(),
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        Some(module.into()),
                        None,
                    )
                    .map_err(|_| error("Could not start the Windows system tray."))?;
                    let state = Box::new(ShellState {
                        callback,
                        callback_active: Arc::new(AtomicBool::new(false)),
                        paused: Arc::new(AtomicBool::new(false)),
                        taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
                        invocation: RefCell::new(Default::default()),
                        requests,
                    });
                    SetWindowLongPtrW(
                        window,
                        GWLP_USERDATA,
                        (&*state as *const ShellState) as isize,
                    );
                    // A conflicting default must not prevent the tray/settings from
                    // opening. The app explicitly applies saved configuration and
                    // surfaces that registration result after install.
                    let _ = state.invocation.borrow_mut().configure(
                        window,
                        InvocationConfig::default(),
                        true,
                        || true,
                    );
                    if !Shell_NotifyIconW(NIM_ADD, &tray_data(window)).as_bool() {
                        state.invocation.borrow_mut().stop(window);
                        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                        let _ = DestroyWindow(window);
                        return Err(error("Could not add Pickle to the system tray."));
                    }
                    if ready.send(Ok(window.0 as usize)).is_err() {
                        state.invocation.borrow_mut().stop(window);
                        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                        let _ = DestroyWindow(window);
                        return Ok(());
                    }
                    let mut message = MSG::default();
                    while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    state.invocation.borrow_mut().stop(window);
                    SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                    let _ = DestroyWindow(window);
                    drop(state);
                    Ok(())
                };
                if let Err(error) = run() {
                    let _ = ready.send(Err(error));
                }
            })
            .map_err(|_| error("Could not start the Windows desktop service."))?;
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(window)) => Ok(Self {
                window,
                commands,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(error("The Windows desktop shell did not start in time.")),
        }
    }
    fn request(&self, command: Command) -> Result<(), PlatformError> {
        let (gate, reply) = (Arc::new(RequestGate::default()), mpsc::sync_channel(1));
        let (sender, receiver) = reply;
        self.commands
            .send(Request {
                command,
                gate: gate.clone(),
                reply: sender,
            })
            .map_err(|_| error("The Windows desktop service has stopped."))?;
        if unsafe {
            PostMessageW(
                Some(HWND(self.window as *mut _)),
                CONFIGURE,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .is_err()
        {
            if gate.cancel() {
                return Err(error("The Windows desktop service is unavailable."));
            }
            return receiver
                .recv()
                .map_err(|_| error("The Windows desktop service has stopped."))?;
        }
        match receiver.recv_timeout(Duration::from_secs(3)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(error("The Windows desktop service has stopped."))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if gate.cancel() {
                    Err(error(
                        "The Windows desktop service did not apply the invocation settings in time.",
                    ))
                } else {
                    // Native commit won the race. Its result is authoritative;
                    // returning a timeout here would let persisted policy diverge.
                    receiver
                        .recv()
                        .map_err(|_| error("The Windows desktop service has stopped."))?
                }
            }
        }
    }
    pub fn configure_invocation(&self, config: InvocationConfig) -> Result<(), PlatformError> {
        let config = config.normalized()?;
        if config.modifier_gesture {
            return Err(error("Choose a conventional shortcut on Windows."));
        }
        self.request(Command::Configure(config))
    }
    pub fn set_paused(&self, paused: bool) -> Result<(), PlatformError> {
        self.request(Command::Pause(paused))
    }
}
impl Drop for NativeShell {
    fn drop(&mut self) {
        let _ = unsafe {
            PostMessageW(
                Some(HWND(self.window as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        };
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conventional_bindings_map_to_windows_virtual_keys() {
        let config = InvocationConfig {
            shortcut: "ctrl+alt+p".into(),
            modifier_gesture: false,
            automatic_menu: false,
        };
        let (mods, key) = windows_shortcut(&config.parsed().unwrap()).unwrap();
        assert_eq!(key, b'P' as u32);
        assert!(mods.contains(MOD_CONTROL | MOD_ALT));
    }
}
