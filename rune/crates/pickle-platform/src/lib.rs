//! Native services only. Rune owns application/window lifetime and the UI event loop.
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct PlatformError(pub String);

pub const READER_FAILURE_TITLE: &str = "Pickle could not open the reader";
pub const READER_FAILURE_MESSAGE: &str = "Restart Pickle. If the problem continues, restart your computer and check for operating system and graphics-driver updates.";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelectionBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Usable monitor rectangle in Rune/winit WindowPlacement physical coordinates:
/// top-left origin, downward-positive y. See the platform README for macOS scale.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
mod browser_page;
mod browser_target;
pub use browser_page::{capture_browser_page, document_urls_match};
mod geometry;
pub use browser_target::{capture_browser_target, validate_browser_target_metadata};
mod library_search;
pub use library_search::{
    LibrarySearchIndex, LibrarySearchLocale, LibrarySearchStats, library_search_locale,
};
mod invocation;
#[cfg(any(target_os = "windows", test))]
mod windows_capabilities;
pub use invocation::InvocationConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapturedSelection {
    pub text: String,
    pub app_name: String,
    pub app_id: String,
    pub bounds: Option<SelectionBounds>,
    pub source_url: Option<String>,
    pub source_pid: u32,
    pub source_window: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionStatus {
    pub accessibility: bool,
    pub screen_capture: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub selection: bool,
    pub menu_bar: bool,
    pub modifier_gesture: bool,
    pub window_capture_ocr: bool,
    pub source_audio: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformEvent {
    Invoke,
    AutomaticInvoke,
    HideChooser,
    Reopen,
    Paste,
    Settings,
    History,
    TogglePause,
    Quit,
    Dismiss,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapturedPage {
    pub jpeg: Vec<u8>,
    pub text: String,
    pub truncated: bool,
}
mod transcription;
#[cfg(target_os = "windows")]
mod windows_child_job;
pub use transcription::{
    LocalTranscriberConfig, PreparedLocalTranscriber, configure_local_transcriber,
    install_local_transcriber, local_transcriber_config, prepare_local_transcriber, transcribe_wav,
};

#[cfg(target_os = "windows")]
mod windows_audio;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "windows")]
mod windows_capture_probe;
#[cfg(target_os = "windows")]
mod windows_context;
#[cfg(target_os = "windows")]
mod windows_impl;
#[cfg(target_os = "windows")]
mod windows_ocr;
#[cfg(target_os = "windows")]
pub use windows_impl::*;
#[cfg(target_os = "windows")]
mod windows_shell;
#[cfg(target_os = "windows")]
pub use windows_shell::NativeShell;

/// Verify Windows package identity and actual local recognition against a fixed
/// in-memory fixture. Reads no screen, clipboard, source application, or network.
pub fn verify_ocr_runtime() -> Result<(), PlatformError> {
    #[cfg(target_os = "windows")]
    {
        windows_ocr::verify_ocr_runtime()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(PlatformError(
            "Windows OCR verification is available only on Windows.".into(),
        ))
    }
}

/// Capture only the fixed diagnostic window created by an owned same-executable
/// helper. No caller-supplied window, application, URL or source is accepted.
pub fn verify_window_capture_runtime() -> Result<serde_json::Value, PlatformError> {
    #[cfg(target_os = "windows")]
    {
        windows_capture_probe::verify_window_capture_runtime()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(PlatformError(
            "Windows owned-window capture verification is available only on Windows.".into(),
        ))
    }
}

/// Private fixed painter protocol, dispatched before the reader app starts.
/// The Windows implementation requires package identity before making a window.
pub fn run_window_capture_fixture() -> Result<(), PlatformError> {
    #[cfg(target_os = "windows")]
    {
        windows_capture_probe::run_window_capture_fixture()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(PlatformError(
            "The owned-window capture fixture is available only on Windows.".into(),
        ))
    }
}

/// Match the same exclusion policy before capture, before a request, and when
/// accepting browser context. Windows exclusions accept a qualified identity,
/// an absolute executable path, or an executable basename. A browser-host alias
/// has no trustworthy installation path, so a matching browser executable/path
/// exclusion conservatively blocks that family's extension messages too.
pub fn source_is_excluded(app_id: &str, exclusions: &[String]) -> bool {
    let source = app_id.trim();
    exclusions.iter().any(|excluded| {
        let excluded = excluded.trim();
        if excluded.is_empty() {
            return false;
        }
        if source.eq_ignore_ascii_case(excluded) {
            return true;
        }
        let Some(source) = windows_identity(source) else {
            return false;
        };
        let Some(excluded) = windows_identity(excluded) else {
            return false;
        };
        if source.alias || excluded.alias || !excluded.path {
            source.basename == excluded.basename
        } else {
            source.name == excluded.name
        }
    })
}

struct WindowsIdentity {
    name: String,
    basename: String,
    path: bool,
    alias: bool,
}
fn windows_identity(identity: &str) -> Option<WindowsIdentity> {
    let identity = identity.to_lowercase().replace('/', "\\");
    if let Some(browser) = identity.strip_prefix("windows-browser:") {
        let executable = match browser {
            "chrome" => "chrome.exe",
            "edge" => "msedge.exe",
            "brave" => "brave.exe",
            "arc" => "arc.exe",
            _ => return None,
        };
        return Some(WindowsIdentity {
            name: executable.into(),
            basename: executable.into(),
            path: false,
            alias: true,
        });
    }
    let name = identity.strip_prefix("windows:").unwrap_or(&identity);
    // Bare Mac bundle IDs and other source namespaces are never Windows filenames.
    if !name.ends_with(".exe") {
        return None;
    }
    let name = if let Some(unc) = name.strip_prefix("\\\\?\\unc\\") {
        format!("\\\\{unc}")
    } else {
        name.strip_prefix("\\\\?\\").unwrap_or(name).to_owned()
    };
    let basename = name.rsplit('\\').next()?.to_owned();
    if basename.is_empty() {
        return None;
    }
    Some(WindowsIdentity {
        path: name.contains('\\'),
        name,
        basename,
        alias: false,
    })
}

pub fn validate_selection(
    value: CapturedSelection,
    exclusions: &[String],
) -> Result<CapturedSelection, PlatformError> {
    if source_is_excluded(&value.app_id, exclusions) {
        return Err(PlatformError(
            "This source application is excluded in Settings.".into(),
        ));
    }
    if value.text.len() > 12_000 {
        return Err(PlatformError(
            "The selection is too long. Select a shorter passage.".into(),
        ));
    }
    if value.text.trim().is_empty() && value.source_url.is_none() {
        return Err(PlatformError(
            "No usable selection was exposed. Try selecting again or paste the passage.".into(),
        ));
    }
    if value.bounds.as_ref().is_some_and(|b| {
        ![b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite())
            || b.width <= 0.0
            || b.height <= 0.0
    }) {
        return Err(PlatformError(
            "The source returned invalid selection coordinates.".into(),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> CapturedSelection {
        CapturedSelection {
            text: "A source passage".into(),
            app_name: "Editor".into(),
            app_id: "editor".into(),
            bounds: None,
            source_url: None,
            source_pid: 1,
            source_window: None,
        }
    }
    #[test]
    fn source_policy_rejects_exclusions_and_oversized_unicode() {
        assert!(validate_selection(selection(), &["EDITOR".into()]).is_err());
        let mut value = selection();
        value.text = "界".repeat(4001);
        assert!(validate_selection(value, &[]).is_err());
    }
    #[test]
    fn invalid_source_geometry_does_not_reach_window_placement() {
        let mut value = selection();
        value.bounds = Some(SelectionBounds {
            x: f64::NAN,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        });
        assert!(validate_selection(value, &[]).is_err());
    }
    #[test]
    fn cancelled_context_operations_never_request_native_access() {
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let source = selection();
        let page = capture_page_with_cancel(&source, cancel.clone()).unwrap_err();
        assert!(page.0.contains("cancelled"));
        let audio = record_audio(&source, cancel).unwrap_err();
        assert!(audio.0.contains("cancelled"));
    }
    #[test]
    fn windows_exclusions_match_basename_full_path_and_qualified_identity() {
        let source = r"windows:C:\Users\Reader\Apps\1Password.exe";
        for excluded in [
            "1PASSWORD.EXE",
            r"C:\Users\Reader\Apps\1Password.exe",
            r"windows:C:\Users\Reader\Apps\1Password.exe",
            r"  C:/Users/Reader/Apps/1Password.exe  ",
            r"\\?\C:\Users\Reader\Apps\1Password.exe",
        ] {
            assert!(
                source_is_excluded(source, &[excluded.into()]),
                "Exclusion must match: {excluded}"
            );
        }
        assert!(!source_is_excluded(source, &["Password.exe".into()]));
        assert!(!source_is_excluded(source, &["my1Password.exe".into()]));
        assert!(!source_is_excluded(
            source,
            &[r"C:\Other\1Password.exe".into()]
        ));
    }
    #[test]
    fn browser_aliases_inherit_executable_exclusions_in_both_directions() {
        for (alias, executable) in [
            ("chrome", "chrome.exe"),
            ("edge", "msedge.exe"),
            ("brave", "brave.exe"),
            ("arc", "arc.exe"),
        ] {
            let identity = format!("windows-browser:{alias}");
            let full_path = format!(r"windows:C:\Program Files\Browser\{executable}");
            assert!(source_is_excluded(&identity, &[executable.into()]));
            assert!(source_is_excluded(
                &identity,
                std::slice::from_ref(&full_path)
            ));
            assert!(source_is_excluded(&full_path, &[identity]));
            assert!(!source_is_excluded(
                &full_path,
                &["otherbrowser.exe".into()]
            ));
        }
        assert!(!source_is_excluded(
            "windows-browser:chrome",
            &["msedge.exe".into()]
        ));
        assert!(!source_is_excluded(
            "windows-browser:unknown",
            &["chrome.exe".into()]
        ));
        assert!(!source_is_excluded(
            r"windows:C:\Apps\mychrome.exe",
            &["windows-browser:chrome".into()]
        ));
    }
    #[test]
    fn macos_exclusions_remain_exact_bundle_id_matches() {
        assert!(source_is_excluded(
            "com.google.Chrome",
            &[" COM.GOOGLE.CHROME ".into()]
        ));
        assert!(!source_is_excluded(
            "com.google.Chrome",
            &["chrome.exe".into()]
        ));
        assert!(!source_is_excluded(
            "com.google.ChromeBeta",
            &["com.google.Chrome".into()]
        ));
    }
}
