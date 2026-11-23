//! Conditional, observational document-URL/window matching. This does not
//! establish an extension tab ID or make an OS screenshot atomic with navigation.
use super::{CapturedPage, CapturedSelection, PlatformError};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "macos")]
#[path = "macos_browser_page.rs"]
mod macos;
#[cfg(target_os = "windows")]
#[path = "windows_browser_page.rs"]
mod windows;

// Every hosted native fixture that owns foreground windows shares this lock.
#[cfg(all(test, target_os = "windows"))]
pub(crate) fn native_desktop_fixture_lock() -> std::sync::MutexGuard<'static, ()> {
    windows::native_browser_fixture_lock()
}

/// Intentionally strict: complete serialized URLs must be byte-identical.
/// No fragment removal, query sorting, Unicode folding, percent decoding,
/// address-bar elision, redirect following or network request is performed.
pub fn document_urls_match(expected: &str, observed: &str) -> bool {
    fn valid(raw: &str) -> bool {
        !raw.is_empty()
            && raw.len() <= 4096
            && !raw
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
            && url::Url::parse(raw).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.has_host()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.as_str() == raw
            })
    }
    expected == observed && valid(expected) && valid(observed)
}

pub fn capture_browser_page(
    source: &CapturedSelection,
    expected_document_url: &str,
    cancel: CancellationToken,
) -> Result<CapturedPage, PlatformError> {
    if cancel.is_cancelled() || !document_urls_match(expected_document_url, expected_document_url) {
        return Err(unavailable());
    }
    let supported = [
        "com.google.Chrome",
        "com.microsoft.edgemac",
        "com.brave.Browser",
        "company.thebrowser.Browser",
        "windows-browser:chrome",
        "windows-browser:edge",
        "windows-browser:brave",
        "windows-browser:arc",
    ]
    .iter()
    .any(|identity| super::validate_browser_target_metadata(identity, source).is_ok());
    if !supported {
        return Err(unavailable());
    }
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    if ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(unavailable());
    }
    struct Flight;
    impl Drop for Flight {
        fn drop(&mut self) {
            ACTIVE.store(false, Ordering::Release);
        }
    }
    let _flight = Flight;
    #[cfg(target_os = "macos")]
    {
        macos::capture(source, expected_document_url, cancel)
    }
    #[cfg(target_os = "windows")]
    {
        windows::capture(source, expected_document_url, cancel)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(unavailable())
    }
}

fn unavailable() -> PlatformError {
    PlatformError("Screen context could not be verified for this browser capture. Continue with the browser text, or select text on the page and invoke Pickle's shortcut.".into())
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq)]
struct DocumentObservation {
    pid: u32,
    window: u64,
    identity: Vec<i32>,
    url: String,
}

#[cfg(any(target_os = "windows", test))]
fn verified_capture<T>(
    source: &CapturedSelection,
    expected: &str,
    cancel: &CancellationToken,
    mut observe: impl FnMut() -> Result<DocumentObservation, PlatformError>,
    capture: impl FnOnce() -> Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    if cancel.is_cancelled() || !document_urls_match(expected, expected) {
        return Err(unavailable());
    }
    let first = observe()?;
    if first.pid != source.source_pid
        || Some(first.window) != source.source_window
        || first.identity.is_empty()
        || !document_urls_match(expected, &first.url)
    {
        return Err(unavailable());
    }
    let before = observe()?;
    if cancel.is_cancelled() || first != before {
        return Err(unavailable());
    }
    let image = capture()?;
    if cancel.is_cancelled() {
        return Err(unavailable());
    }
    let after = observe()?;
    if cancel.is_cancelled() || after != first {
        return Err(unavailable());
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_url_equality_retains_fragments_queries_encoded_separators_and_case() {
        let original = "https://example.com/Path%2Fpart?q=Caf%C3%A9&b=2#route-one";
        assert!(document_urls_match(original, original));
        for different in [
            "https://example.com/Path/part?q=Caf%C3%A9&b=2#route-one",
            "https://example.com/Path%2fpart?q=Caf%C3%A9&b=2#route-one",
            "https://example.com/path%2Fpart?q=Caf%C3%A9&b=2#route-one",
            "https://example.com/Path%2Fpart?b=2&q=Caf%C3%A9#route-one",
            "https://example.com/Path%2Fpart?q=Caf%C3%A9&b=2#route-two",
        ] {
            assert!(!document_urls_match(original, different));
        }
        for invalid in [
            "https://user:pass@example.com/",
            "https://example.com/\n",
            "https://example.com\\path",
            "https://example.com",
            "chrome://settings/",
        ] {
            assert!(!document_urls_match(invalid, invalid));
        }
    }
    #[test]
    fn capture_is_not_invoked_for_mismatch_changed_identity_or_cancelled_observation() {
        let source = CapturedSelection {
            text: String::new(),
            app_name: "Fixture".into(),
            app_id: "com.google.Chrome".into(),
            bounds: None,
            source_url: None,
            source_pid: 42,
            source_window: Some(43),
        };
        for phase in 0..4 {
            let cancel = CancellationToken::new();
            let mut reads = 0;
            let captured = std::cell::Cell::new(0);
            let result = verified_capture(
                &source,
                "https://example.com/page#one",
                &cancel,
                || {
                    reads += 1;
                    if phase == 3 {
                        cancel.cancel();
                    }
                    Ok(DocumentObservation {
                        pid: 42,
                        window: 43,
                        identity: vec![if phase == 1 && reads > 1 { 2 } else { 1 }],
                        url: if phase == 0 {
                            "https://example.com/page#two"
                        } else {
                            "https://example.com/page#one"
                        }
                        .into(),
                    })
                },
                || {
                    captured.set(captured.get() + 1);
                    Ok(())
                },
            );
            if phase == 2 {
                assert!(result.is_ok());
                assert_eq!(captured.get(), 1);
            } else {
                assert!(result.is_err());
                assert_eq!(captured.get(), 0);
            }
        }
    }
    #[test]
    fn navigation_after_capture_discards_the_result() {
        let source = CapturedSelection {
            text: String::new(),
            app_name: "Fixture".into(),
            app_id: "com.google.Chrome".into(),
            bounds: None,
            source_url: None,
            source_pid: 42,
            source_window: Some(43),
        };
        let mut reads = 0;
        assert!(
            verified_capture(
                &source,
                "https://example.com/page",
                &CancellationToken::new(),
                || {
                    reads += 1;
                    Ok(DocumentObservation {
                        pid: 42,
                        window: 43,
                        identity: vec![1],
                        url: if reads == 3 {
                            "https://example.com/changed"
                        } else {
                            "https://example.com/page"
                        }
                        .into(),
                    })
                },
                || Ok("owned synthetic frame")
            )
            .is_err()
        );
    }
}
