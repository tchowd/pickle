//! Browser app/window metadata only. This never reads selection, URL, pixels,
//! audio, or Accessibility content and cannot establish an extension-tab match.
use super::{CapturedSelection, PlatformError};
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) fn browser_target_matches(expected: &str, actual: &str) -> bool {
    match expected {
        "com.google.Chrome"
        | "com.microsoft.edgemac"
        | "com.brave.Browser"
        | "company.thebrowser.Browser" => actual == expected,
        "windows-browser:chrome"
        | "windows-browser:edge"
        | "windows-browser:brave"
        | "windows-browser:arc" => {
            let Some(expected) = super::windows_identity(expected) else {
                return false;
            };
            let Some(actual) = super::windows_identity(actual) else {
                return false;
            };
            !actual.alias && actual.path && actual.basename == expected.basename
        }
        _ => false,
    }
}

/// Structural check of a resolver result; does not requery the OS or establish
/// tab/URL pairing. Packet fields must never be used to construct this metadata.
pub fn validate_browser_target_metadata(
    expected: &str,
    source: &CapturedSelection,
) -> Result<(), PlatformError> {
    if !browser_target_matches(expected, &source.app_id)
        || source.source_pid == 0
        || source.source_pid == std::process::id()
        || source.source_window.is_none_or(|window| window == 0)
        || !source.text.is_empty()
        || source.source_url.is_some()
        || source.bounds.is_some()
        || source.app_name.len() > 1_000
        || source.app_id.len() > 32_800
    {
        return Err(PlatformError("The foreground browser could not be bound to a native source window. Invoke Pickle from the browser before listening.".into()));
    }
    Ok(())
}

/// Call from a tracked worker and impose an application-side wait budget.
/// A stuck OS metadata query holds this gate, preventing more blocked queries.
/// The returned target authorizes only explicit app-scoped Listen; its absent
/// URL is deliberate and must not be filled from extension packet fields.
pub fn capture_browser_target(expected: &str) -> Result<CapturedSelection, PlatformError> {
    #[cfg(target_os = "macos")]
    let supported = matches!(
        expected,
        "com.google.Chrome"
            | "com.microsoft.edgemac"
            | "com.brave.Browser"
            | "company.thebrowser.Browser"
    );
    #[cfg(target_os = "windows")]
    let supported = matches!(
        expected,
        "windows-browser:chrome"
            | "windows-browser:edge"
            | "windows-browser:brave"
            | "windows-browser:arc"
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let supported = false;
    if !supported {
        return Err(PlatformError(
            "This browser has no supported native source identity.".into(),
        ));
    }
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    if ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(PlatformError("Native browser source resolution is still busy. Invoke Pickle from the browser before listening.".into()));
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            ACTIVE.store(false, Ordering::Release);
        }
    }
    let _guard = Guard;
    #[cfg(target_os = "macos")]
    let source = super::macos::resolve_browser_target(expected)?;
    #[cfg(target_os = "windows")]
    let source = super::windows_impl::resolve_browser_target(expected)?;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        validate_browser_target_metadata(expected, &source)?;
        Ok(source)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err(PlatformError(
        "Native browser sources are unavailable on this platform.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_identity_requires_exact_bundle_or_real_executable_path() {
        assert!(browser_target_matches(
            "com.google.Chrome",
            "com.google.Chrome"
        ));
        assert!(!browser_target_matches(
            "com.google.Chrome",
            "com.google.Chrome.canary"
        ));
        assert!(browser_target_matches(
            "windows-browser:edge",
            r"windows:C:\Program Files\Edge\MSEDGE.EXE"
        ));
        for wrong in [
            r"windows:C:\Chrome\chrome.exe",
            "windows-browser:edge",
            "msedge.exe",
            r"windows:C:\Edge\other-msedge.exe",
        ] {
            assert!(!browser_target_matches("windows-browser:edge", wrong));
        }
        assert!(!browser_target_matches(
            "page-supplied-browser",
            "com.google.Chrome"
        ));
    }
    #[test]
    fn target_metadata_cannot_smuggle_reference_content_or_invalid_ownership() {
        let mut source = CapturedSelection {
            text: String::new(),
            app_name: "Owned browser fixture".into(),
            app_id: "com.google.Chrome".into(),
            bounds: None,
            source_url: None,
            source_pid: std::process::id().wrapping_add(1),
            source_window: Some(42),
        };
        validate_browser_target_metadata("com.google.Chrome", &source).unwrap();
        source.source_url = Some("https://example.com/packet".into());
        assert!(validate_browser_target_metadata("com.google.Chrome", &source).is_err());
        source.source_url = None;
        source.text = "packet text".into();
        assert!(validate_browser_target_metadata("com.google.Chrome", &source).is_err());
        source.text.clear();
        source.source_pid = std::process::id();
        assert!(validate_browser_target_metadata("com.google.Chrome", &source).is_err());
        source.source_pid = 0;
        assert!(validate_browser_target_metadata("com.google.Chrome", &source).is_err());
    }
}
