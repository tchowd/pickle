use super::PlatformError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvocationConfig {
    pub shortcut: String,
    pub modifier_gesture: bool,
    pub automatic_menu: bool,
}
impl Default for InvocationConfig {
    fn default() -> Self {
        Self {
            shortcut: "ctrl+alt+p".into(),
            modifier_gesture: cfg!(target_os = "macos"),
            automatic_menu: false,
        }
    }
}
impl InvocationConfig {
    pub fn validate(&self) -> Result<(), PlatformError> {
        self.parsed().map(|_| ())
    }
    pub(crate) fn normalized(&self) -> Result<Self, PlatformError> {
        let parsed = self.parsed()?;
        Ok(Self {
            shortcut: parsed.canonical(),
            ..self.clone()
        })
    }
    pub(crate) fn parsed(&self) -> Result<Shortcut, PlatformError> {
        if self.shortcut.len() > 120 {
            return Err(invalid());
        }
        let mut mods = 0;
        let mut key = None;
        for item in self.shortcut.split('+') {
            let part = item.trim().to_ascii_lowercase();
            let modifier = match part.as_str() {
                "ctrl" | "control" => 1,
                "alt" | "option" | "opt" => 2,
                "shift" => 4,
                "cmd" | "command" | "super" | "win" => 8,
                "mod" => {
                    if cfg!(target_os = "macos") {
                        8
                    } else {
                        1
                    }
                }
                _ => 0,
            };
            if modifier != 0 {
                if key.is_some() || mods & modifier != 0 {
                    return Err(invalid());
                }
                mods |= modifier;
                continue;
            }
            if key.is_some() {
                return Err(invalid());
            }
            let name = match part.as_str() {
                "spacebar" => "space",
                "enter" => "return",
                "esc" => "escape",
                name => name,
            };
            let character = name.len() == 1 && name.as_bytes()[0].is_ascii_alphanumeric();
            let function = name
                .strip_prefix('f')
                .and_then(|number| number.parse::<u8>().ok())
                .is_some_and(|n| (1..=24).contains(&n));
            let named = [
                "space",
                "return",
                "tab",
                "escape",
                "comma",
                "period",
                "minus",
                "equal",
                "slash",
                "backslash",
                "semicolon",
                "quote",
                "backquote",
                "leftbracket",
                "rightbracket",
            ]
            .contains(&name);
            if !character && !function && !named {
                return Err(invalid());
            }
            key = Some(if function {
                format!("f{}", name[1..].parse::<u8>().map_err(|_| invalid())?)
            } else {
                name.to_owned()
            });
        }
        let key = key.ok_or_else(invalid)?;
        let function = key
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|n| (1..=24).contains(&n));
        if mods == 0 && !function {
            return Err(PlatformError(
                "Include Control, Alt/Option, or Command/Windows in a global shortcut.".into(),
            ));
        }
        if mods == 4 && !function {
            return Err(PlatformError(
                "Shift alone cannot make an ordinary key a global shortcut.".into(),
            ));
        }
        Ok(Shortcut { mods, key })
    }
}
fn invalid() -> PlatformError {
    PlatformError(
        "Use a key combination such as ctrl+alt+p, cmd+shift+space, or ctrl+shift+k.".into(),
    )
}
#[cfg(any(target_os = "windows", test))]
pub(crate) fn next_registration_id(current: u32, limit: u32) -> Result<u32, PlatformError> {
    current
        .checked_add(1)
        .filter(|next| *next <= limit)
        .ok_or_else(|| PlatformError("Restart Pickle before changing its shortcut again.".into()))
}
/// Arbitrates a native configuration commit against a caller timeout. A timeout
/// that wins leaves the previous configuration active; once native commit wins,
/// the caller waits for its definitive result instead of reporting a false error.
#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
pub(crate) struct RequestGate(std::sync::atomic::AtomicU8);
#[cfg(any(target_os = "windows", test))]
impl RequestGate {
    pub fn pending(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire) == 0
    }
    pub fn cancel(&self) -> bool {
        self.change(1)
    }
    pub fn commit(&self) -> bool {
        self.change(2)
    }
    fn change(&self, value: u8) -> bool {
        self.0
            .compare_exchange(
                0,
                value,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
    }
}
pub(crate) struct Shortcut {
    pub mods: u8,
    pub key: String,
}
impl Shortcut {
    fn canonical(&self) -> String {
        let mut parts = Vec::new();
        for (mask, name) in [(1, "ctrl"), (2, "alt"), (4, "shift"), (8, "super")] {
            if self.mods & mask != 0 {
                parts.push(name)
            }
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
pub(crate) struct ModifierChord {
    armed: bool,
    interrupted: bool,
}
#[cfg(any(target_os = "macos", test))]
impl ModifierChord {
    pub fn reset(&mut self) {
        self.armed = false;
        self.interrupted = false
    }
    pub fn interrupt(&mut self, control: bool, option: bool) {
        if !control && !option {
            self.reset()
        } else {
            self.armed = false;
            self.interrupted = true
        }
    }
    pub fn flags(&mut self, control: bool, option: bool, other: bool) -> bool {
        if other {
            self.armed = false;
            self.interrupted = true
        }
        if !control && !option && !other {
            let invoke = self.armed && !self.interrupted;
            self.reset();
            return invoke;
        }
        if control && option && !self.interrupted {
            self.armed = true
        }
        false
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
pub(crate) struct AutoDebounce {
    due: Option<u64>,
}
#[cfg(any(target_os = "windows", test))]
impl AutoDebounce {
    pub const DELAY_MS: u64 = 180;
    pub fn down(&mut self) {
        self.due = None
    }
    pub fn up(&mut self, now: u64) {
        self.due = Some(now.saturating_add(Self::DELAY_MS))
    }
    pub fn remaining(&self, now: u64) -> Option<u64> {
        self.due.map(|due| due.saturating_sub(now))
    }
    pub fn poll(&mut self, now: u64) -> bool {
        if self.due.is_some_and(|due| now >= due) {
            self.due = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_hotkey_registration_ids_are_never_recycled() {
        assert_eq!(next_registration_id(0, 0xBFFF).unwrap(), 1);
        assert_eq!(next_registration_id(1, 0xBFFF).unwrap(), 2);
        assert_eq!(next_registration_id(2, 0xBFFF).unwrap(), 3);
        assert!(next_registration_id(0xBFFF, 0xBFFF).is_err());
        assert!(next_registration_id(u32::MAX, u32::MAX).is_err());
    }
    #[test]
    fn timed_out_configuration_and_native_commit_are_mutually_exclusive() {
        let cancelled = RequestGate::default();
        assert!(cancelled.pending());
        assert!(cancelled.cancel());
        assert!(!cancelled.commit());
        assert!(!cancelled.pending());
        let committed = RequestGate::default();
        assert!(committed.commit());
        assert!(!committed.cancel());
        for _ in 0..32 {
            let gate = std::sync::Arc::new(RequestGate::default());
            let cancel = gate.clone();
            let worker = std::thread::spawn(move || cancel.cancel());
            let committed = gate.commit();
            assert_ne!(committed, worker.join().unwrap());
        }
    }
    #[test]
    fn conventional_shortcuts_are_bounded_canonical_and_do_not_capture_typing() {
        let config = InvocationConfig {
            shortcut: "Control + Option + P".into(),
            modifier_gesture: false,
            automatic_menu: true,
        };
        assert_eq!(config.normalized().unwrap().shortcut, "ctrl+alt+p");
        for bad in [
            "p",
            "f",
            "shift+p",
            "ctrl+alt",
            "ctrl+p+k",
            "ctrl+ctrl+p",
            "ctrl+unknown",
            "ctrl+\0",
        ] {
            assert!(
                InvocationConfig {
                    shortcut: bad.into(),
                    ..config.clone()
                }
                .validate()
                .is_err(),
                "{bad}"
            );
        }
    }
    #[test]
    fn modifier_gesture_requires_both_modifiers_and_fires_once_on_release() {
        let mut state = ModifierChord::default();
        assert!(!state.flags(true, false, false));
        assert!(!state.flags(true, true, false));
        assert!(!state.flags(false, true, false));
        assert!(state.flags(false, false, false));
        assert!(!state.flags(false, false, false));
        assert!(!state.flags(false, true, false));
        // The original second chord starts with Option, then adds Control.
        assert!(!state.flags(true, true, false));
        assert!(state.flags(false, false, false));
        assert!(!state.flags(false, false, false));
    }
    #[test]
    fn standalone_control_and_option_release_cycles_never_invoke() {
        let mut state = ModifierChord::default();
        assert!(!state.flags(true, false, false));
        assert!(!state.flags(false, false, false));
        assert!(!state.flags(false, true, false));
        assert!(!state.flags(false, false, false));
    }
    #[test]
    fn ordinary_input_extra_modifiers_and_policy_reset_cancel_gesture() {
        let mut state = ModifierChord::default();
        state.flags(true, true, false);
        state.interrupt(true, true);
        assert!(!state.flags(true, true, false));
        assert!(!state.flags(false, false, false));
        assert!(!state.flags(true, true, true));
        assert!(!state.flags(true, true, false));
        assert!(!state.flags(false, false, false));
        state.flags(true, true, false);
        state.reset();
        assert!(!state.flags(false, false, false));
        // Every interrupted sequence must still allow a later clean chord.
        assert!(!state.flags(true, true, false));
        assert!(state.flags(false, false, false));
    }
    #[test]
    fn mouse_chooser_debounces_drag_and_stale_timer_events() {
        let mut state = AutoDebounce::default();
        state.up(0);
        assert!(!state.poll(179));
        state.down();
        assert!(!state.poll(200));
        state.up(200);
        state.up(300);
        assert!(!state.poll(380));
        assert_eq!(state.remaining(380), Some(100));
        assert!(state.poll(480));
        assert!(!state.poll(481));
    }
}
