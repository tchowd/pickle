//! Explicit disconnection removes only the current user's exact origin-bound installation.
use super::{BrowserKind, HostRegistration, error};
use pickle_core::PickleError;
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, PickleError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(error("Could not inspect browser host registration.")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16_384 {
        return Err(error(
            "Browser host registration is unsafe or unreadable; nothing was removed.",
        ));
    }
    let file =
        fs::File::open(path).map_err(|_| error("Could not read browser host registration."))?;
    let mut bytes = Vec::new();
    file.take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| error("Could not read browser host registration."))?;
    if bytes.len() > 16_384 {
        return Err(error(
            "Browser host registration is too large; nothing was removed.",
        ));
    }
    Ok(Some(bytes))
}

fn host_directory(browser: BrowserKind, support: &Path) -> Result<PathBuf, PickleError> {
    if !support.is_absolute()
        || support.parent().is_none()
        || support
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(error(
            "Use Pickle's absolute application-data directory for browser setup.",
        ));
    }
    let root = support.join("NativeHost");
    let directory = root.join(browser.label().to_ascii_lowercase());
    for path in [support, &root, &directory] {
        if fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            return Err(error(
                "Browser host directories must not be symbolic links or other file types.",
            ));
        }
    }
    Ok(directory)
}

fn manifest_owned(bytes: &[u8], destination: &Path, origin: &str) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    value["name"] == "com.pickle.reader"
        && value["type"] == "stdio"
        && value["path"]
            .as_str()
            .is_some_and(|path| Path::new(path) == destination)
        && value["allowed_origins"] == serde_json::json!([origin])
}

struct Removal {
    directory: PathBuf,
    registration: PathBuf,
    registration_bytes: Vec<u8>,
    binary: PathBuf,
    manifest: PathBuf,
    manifest_bytes: Option<Vec<u8>>,
}

fn plan(
    browser: BrowserKind,
    extension: &str,
    support: &Path,
    manifest: PathBuf,
) -> Result<Option<Removal>, PickleError> {
    if extension.len() != 32 || !extension.bytes().all(|byte| (b'a'..=b'p').contains(&byte)) {
        return Err(error(
            "Paste the extension ID connected to this browser before disconnecting it.",
        ));
    }
    let directory = host_directory(browser, support)?;
    let registration = directory.join("registration.json");
    let Some(registration_bytes) = read_regular(&registration)? else {
        return Ok(None);
    };
    let value: HostRegistration = serde_json::from_slice(&registration_bytes)
        .map_err(|_| error("This browser registration is unreadable; nothing was removed."))?;
    let origin = format!("chrome-extension://{extension}/");
    if value.browser != browser
        || value.allowed_origin != origin
        || value.bridge_directory != support.join("BrowserBridge")
    {
        return Err(error(
            "This browser is connected to another extension or installation; nothing was removed.",
        ));
    }
    let binary = directory.join(if cfg!(windows) {
        "pickle-browser-host.exe"
    } else {
        "pickle-browser-host"
    });
    if fs::symlink_metadata(&binary)
        .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(error(
            "The browser host copy is not a regular file; nothing was removed.",
        ));
    }
    let manifest_bytes = read_regular(&manifest)?;
    if manifest_bytes
        .as_ref()
        .is_some_and(|bytes| !manifest_owned(bytes, &binary, &origin))
    {
        return Err(error(
            "The browser now uses another registration; it was left unchanged.",
        ));
    }
    Ok(Some(Removal {
        directory,
        registration,
        registration_bytes,
        binary,
        manifest,
        manifest_bytes,
    }))
}

fn unchanged(path: &Path, expected: &Option<Vec<u8>>) -> Result<(), PickleError> {
    if &read_regular(path)? != expected {
        return Err(error(
            "Browser registration changed during disconnection; try again.",
        ));
    }
    Ok(())
}

#[cfg(any(not(target_os = "windows"), test))]
fn remove_files(removal: Removal) -> Result<(), PickleError> {
    remove_files_with_registry(removal, |_| Ok(()))
}

fn remove_files_with_registry(
    removal: Removal,
    remove_registry: impl FnOnce(&Path) -> Result<(), PickleError>,
) -> Result<(), PickleError> {
    unchanged(
        &removal.registration,
        &Some(removal.registration_bytes.clone()),
    )?;
    unchanged(&removal.manifest, &removal.manifest_bytes)?;
    // A running Windows host may lock its binary. Detect that before touching its live hook.
    match fs::remove_file(&removal.binary) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(error(
                "The browser host is in use. Finish the browser request and disconnect again.",
            ));
        }
    };
    unchanged(&removal.manifest, &removal.manifest_bytes)?;
    if removal.manifest_bytes.is_some() {
        fs::remove_file(&removal.manifest)
            .map_err(|_| error("Could not remove the browser's Pickle manifest."))?;
    }
    // Registry cleanup follows owned-file removal; retain registration.json for recovery on failure.
    remove_registry(&removal.manifest)?;
    unchanged(&removal.registration, &Some(removal.registration_bytes))?;
    fs::remove_file(&removal.registration)
        .map_err(|_| error("Could not remove Pickle's browser registration."))?;
    // Never recurse: unrelated files and parent directories are deliberately preserved.
    let _ = fs::remove_dir(removal.directory);
    Ok(())
}

/// Returns true when this installation was disconnected, false when no owned installation exists.
/// An origin/path mismatch is an error and preserves the existing browser hook and host copy.
pub fn uninstall_browser_host(
    browser: BrowserKind,
    extension_id: &str,
    support_root: &Path,
) -> Result<bool, PickleError> {
    uninstall_expected_application(browser, extension_id, support_root, None)
}

fn uninstall_expected_application(
    browser: BrowserKind,
    extension_id: &str,
    support_root: &Path,
    expected: Option<&Path>,
) -> Result<bool, PickleError> {
    let directory = host_directory(browser, support_root)?;
    #[cfg(target_os = "macos")]
    let manifest = {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| error("Could not locate browser settings."))?;
        base.data_dir()
            .join(browser.manifest_folder())
            .join("NativeMessagingHosts/com.pickle.reader.json")
    };
    #[cfg(target_os = "windows")]
    let manifest = directory.join("com.pickle.reader.json");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (directory, extension_id);
        return Err(error(
            "Browser disconnection is supported on macOS and Windows.",
        ));
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let _ = &directory;
        let Some(removal) = plan(browser, extension_id, support_root, manifest)? else {
            return Ok(false);
        };
        if let Some(application) = expected {
            let registration: HostRegistration =
                serde_json::from_slice(&removal.registration_bytes)
                    .map_err(|_| error("Browser registration changed during cleanup."))?;
            if !registration_owned(&registration, application)? {
                return Err(error(
                    "The browser host now belongs to another application; it was preserved.",
                ));
            }
        }
        #[cfg(target_os = "windows")]
        {
            if registry_value(browser)?.is_some_and(|current| current != removal.manifest) {
                return Err(error(
                    "The browser now uses another native host; it was preserved.",
                ));
            }
            remove_files_with_registry(removal, |manifest| {
                remove_registry_value(browser, manifest)
            })?;
        }
        #[cfg(not(target_os = "windows"))]
        remove_files(removal)?;
        Ok(true)
    }
}

fn same_application(registered: &Path, current: &Path) -> bool {
    registered.is_absolute()
        && fs::canonicalize(registered).is_ok_and(|registered| registered == current)
}

fn registration_owned(
    registration: &HostRegistration,
    current: &Path,
) -> Result<bool, PickleError> {
    let current_identity = match std::env::current_exe().and_then(fs::canonicalize) {
        Ok(executable) if executable == current => super::package::current_aumid()?,
        _ => None,
    };
    Ok(ownership_matches(
        registration,
        current,
        current_identity.as_deref(),
    ))
}

fn ownership_matches(
    registration: &HostRegistration,
    current: &Path,
    identity: Option<&str>,
) -> bool {
    match registration.application_aumid.as_deref() {
        Some(registered) => super::package::valid_aumid(registered) && identity == Some(registered),
        None => same_application(&registration.application, current),
    }
}

fn cleanup_for_application(
    application: &Path,
    support_root: &Path,
    mut disconnect: impl FnMut(BrowserKind, &str, &Path) -> Result<bool, PickleError>,
) -> Result<usize, PickleError> {
    let application = fs::canonicalize(application).map_err(|_| {
        error("Could not resolve this application before disconnecting its browser hosts.")
    })?;
    let mut candidates = Vec::new();
    // Preflight every known registration before performing any removal.
    for browser in BrowserKind::ALL {
        let directory = host_directory(browser, support_root)?;
        let registration_path = directory.join("registration.json");
        let Some(bytes) = read_regular(&registration_path)? else {
            continue;
        };
        let registration: HostRegistration = serde_json::from_slice(&bytes).map_err(|_| {
            error("A browser host registration is unreadable; no cleanup was started.")
        })?;
        if !registration_owned(&registration, &application)? {
            continue;
        }
        if registration.browser != browser {
            return Err(error(
                "A browser host registration has conflicting ownership; no cleanup was started.",
            ));
        }
        let id=registration.allowed_origin.strip_prefix("chrome-extension://").and_then(|value|value.strip_suffix('/')).filter(|value|value.len()==32 && value.bytes().all(|byte|(b'a'..=b'p').contains(&byte))).ok_or_else(||error("A browser host registration has an invalid extension origin; no cleanup was started."))?.to_owned();
        candidates.push((browser, id, registration_path, bytes));
    }
    let mut removed = 0;
    for (browser, id, path, bytes) in candidates {
        unchanged(&path, &Some(bytes))?;
        if disconnect(browser, &id, &application)? {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Installer cleanup: disconnect only this exact executable or this process's OS package identity.
/// Full MSIX registrations can survive versioned paths; sparse and unpackaged hosts require the
/// same canonical executable. Other application identities and installations are skipped.
pub fn uninstall_hosts_for_application(
    application: &Path,
    support_root: &Path,
) -> Result<usize, PickleError> {
    cleanup_for_application(application, support_root, |browser, id, expected| {
        uninstall_expected_application(browser, id, support_root, Some(expected))
    })
}

#[cfg(target_os = "windows")]
fn registry_value(browser: BrowserKind) -> Result<Option<PathBuf>, PickleError> {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW},
    };
    let key = browser
        .registry_key()
        .ok_or_else(|| error("This browser is unsupported on Windows."))?;
    let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let mut bytes = 0u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS || bytes > 65_536 || !bytes.is_multiple_of(2) {
        return Err(error(
            "Could not inspect the current user's browser host registry value.",
        ));
    }
    let mut value = vec![0u16; (bytes as usize) / 2];
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != ERROR_SUCCESS || bytes as usize > value.len() * 2 {
        return Err(error(
            "Browser host registry value changed during disconnection.",
        ));
    }
    value.truncate(bytes as usize / 2);
    while value.last() == Some(&0) {
        value.pop();
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(&value))))
}

#[cfg(target_os = "windows")]
fn remove_registry_value(browser: BrowserKind, expected: &Path) -> Result<(), PickleError> {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyValueW},
    };
    let Some(current) = registry_value(browser)? else {
        return Ok(());
    };
    if current != expected {
        return Err(error(
            "The browser now uses another native host; its registry value was preserved.",
        ));
    }
    let key: Vec<u16> = browser
        .registry_key()
        .unwrap()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // Remove only the default value in the current user's exact host key, never the key tree.
    let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), std::ptr::null()) };
    if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
        return Err(error(
            "Could not disconnect the current user's browser host.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    #[test]
    fn package_ownership_survives_version_paths_but_never_crosses_identity() {
        let root = tempfile::tempdir().unwrap();
        let current = root.path().join("new-version.exe");
        fs::write(&current, "fixture").unwrap();
        let current = fs::canonicalize(current).unwrap();
        let mut registration = HostRegistration {
            browser: BrowserKind::Chrome,
            allowed_origin: format!("chrome-extension://{ID}/"),
            bridge_directory: root.path().join("BrowserBridge"),
            application: root.path().join("removed-old-version.exe"),
            application_aumid: Some("Pickle.Rune_123456789abcd!Pickle".into()),
        };
        assert!(ownership_matches(
            &registration,
            &current,
            Some("Pickle.Rune_123456789abcd!Pickle")
        ));
        assert!(!ownership_matches(
            &registration,
            &current,
            Some("Pickle.Other_123456789abcd!Pickle")
        ));
        assert!(!ownership_matches(&registration, &current, None));
        registration.application = current.clone();
        assert!(!ownership_matches(&registration, &current, None));
        registration.application_aumid = None;
        assert!(ownership_matches(&registration, &current, None));
        registration.application = root.path().join("other.exe");
        assert!(!ownership_matches(
            &registration,
            &current,
            Some("Pickle.Rune_123456789abcd!Pickle")
        ));
    }
    fn installed(root: &Path) -> PathBuf {
        let directory = root.join("NativeHost/chrome");
        fs::create_dir_all(&directory).unwrap();
        let binary = directory.join(if cfg!(windows) {
            "pickle-browser-host.exe"
        } else {
            "pickle-browser-host"
        });
        fs::write(&binary, b"fixture host").unwrap();
        let registration = HostRegistration {
            browser: BrowserKind::Chrome,
            allowed_origin: format!("chrome-extension://{ID}/"),
            bridge_directory: root.join("BrowserBridge"),
            application: root.join("Pickle"),
            application_aumid: None,
        };
        fs::write(
            directory.join("registration.json"),
            serde_json::to_vec(&registration).unwrap(),
        )
        .unwrap();
        let manifest = root.join("fixture-browser-manifest.json");
        fs::write(&manifest,serde_json::to_vec(&serde_json::json!({"name":"com.pickle.reader","type":"stdio","path":binary,"allowed_origins":[registration.allowed_origin]})).unwrap()).unwrap();
        manifest
    }
    #[test]
    fn removal_is_origin_bound_and_preserves_unrelated_files() {
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        let untouched = root.path().join("NativeHost/chrome/unrelated.txt");
        fs::write(&untouched, "keep").unwrap();
        assert!(
            plan(
                BrowserKind::Chrome,
                &"b".repeat(32),
                root.path(),
                manifest.clone()
            )
            .is_err()
        );
        assert!(manifest.exists());
        let removal = plan(BrowserKind::Chrome, ID, root.path(), manifest.clone())
            .unwrap()
            .unwrap();
        remove_files(removal).unwrap();
        assert!(!manifest.exists());
        assert!(untouched.exists());
        assert!(
            !untouched
                .parent()
                .unwrap()
                .join("registration.json")
                .exists()
        );
    }
    #[test]
    fn another_installations_manifest_and_copy_are_not_removed() {
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        fs::write(&manifest, b"{\"name\":\"another.app\"}").unwrap();
        assert!(plan(BrowserKind::Chrome, ID, root.path(), manifest.clone()).is_err());
        assert!(manifest.exists());
        assert!(
            root.path()
                .join("NativeHost/chrome/registration.json")
                .exists()
        );
    }
    #[test]
    fn changed_registration_is_detected_before_removal() {
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        let removal = plan(BrowserKind::Chrome, ID, root.path(), manifest.clone())
            .unwrap()
            .unwrap();
        fs::write(&manifest, b"{\"name\":\"replacement\"}").unwrap();
        assert!(remove_files(removal).is_err());
        assert!(manifest.exists());
    }
    #[test]
    fn missing_installation_is_noop_and_partial_owned_installation_can_be_cleaned() {
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join("missing.json");
        assert!(
            plan(BrowserKind::Chrome, ID, root.path(), manifest)
                .unwrap()
                .is_none()
        );
        let manifest = installed(root.path());
        fs::remove_file(&manifest).unwrap();
        let removal = plan(BrowserKind::Chrome, ID, root.path(), manifest)
            .unwrap()
            .unwrap();
        remove_files(removal).unwrap();
        assert!(!root.path().join("NativeHost/chrome").exists());
    }
    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_refused_without_following_them() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        let binary = root.path().join("NativeHost/chrome/pickle-browser-host");
        fs::remove_file(&binary).unwrap();
        let outside = root.path().join("unrelated");
        fs::write(&outside, "keep").unwrap();
        symlink(&outside, &binary).unwrap();
        assert!(plan(BrowserKind::Chrome, ID, root.path(), manifest.clone()).is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "keep");
        assert!(manifest.exists());
    }

    #[test]
    fn installer_cleanup_matches_exact_application_and_skips_another_installation() {
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        let current = root.path().join("Pickle");
        fs::write(&current, "current app").unwrap();
        let other = root.path().join("other-version/Pickle");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, "another app").unwrap();
        let edge = root.path().join("NativeHost/edge");
        fs::create_dir_all(&edge).unwrap();
        let registration = HostRegistration {
            browser: BrowserKind::Edge,
            allowed_origin: format!("chrome-extension://{ID}/"),
            bridge_directory: root.path().join("BrowserBridge"),
            application: other,
            application_aumid: None,
        };
        fs::write(
            edge.join("registration.json"),
            serde_json::to_vec(&registration).unwrap(),
        )
        .unwrap();
        let removed = cleanup_for_application(&current, root.path(), |browser, id, expected| {
            assert_eq!(browser, BrowserKind::Chrome);
            assert_eq!(expected, fs::canonicalize(&current).unwrap());
            let removal = plan(browser, id, root.path(), manifest.clone())?.unwrap();
            remove_files(removal)?;
            Ok(true)
        })
        .unwrap();
        assert_eq!(removed, 1);
        assert!(edge.join("registration.json").exists());
        assert!(!manifest.exists());
    }

    #[test]
    fn installer_preflight_rejects_unsafe_registration_before_removing_any_host() {
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        let current = root.path().join("Pickle");
        fs::write(&current, "current app").unwrap();
        let edge = root.path().join("NativeHost/edge");
        fs::create_dir_all(&edge).unwrap();
        fs::write(edge.join("registration.json"), b"broken").unwrap();
        let mut called = false;
        let result = cleanup_for_application(&current, root.path(), |_, _, _| {
            called = true;
            Ok(true)
        });
        assert!(result.is_err());
        assert!(!called);
        assert!(manifest.exists());
    }

    #[test]
    fn registry_cleanup_runs_after_binary_and_manifest_but_before_recovery_metadata() {
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        let directory = root.path().join("NativeHost/chrome");
        let removal = plan(BrowserKind::Chrome, ID, root.path(), manifest.clone())
            .unwrap()
            .unwrap();
        let binary = removal.binary.clone();
        remove_files_with_registry(removal, |path| {
            assert_eq!(path, manifest);
            assert!(!binary.exists());
            assert!(!manifest.exists());
            assert!(directory.join("registration.json").exists());
            Ok(())
        })
        .unwrap();
        assert!(!directory.exists());
    }

    #[test]
    fn failed_registry_cleanup_retains_metadata_for_a_safe_retry() {
        let root = tempfile::tempdir().unwrap();
        let manifest = installed(root.path());
        let directory = root.path().join("NativeHost/chrome");
        let removal = plan(BrowserKind::Chrome, ID, root.path(), manifest.clone())
            .unwrap()
            .unwrap();
        assert!(
            remove_files_with_registry(removal, |_| Err(error("Registry fixture denied cleanup")))
                .is_err()
        );
        assert!(directory.join("registration.json").exists());
        let removal = plan(BrowserKind::Chrome, ID, root.path(), manifest)
            .unwrap()
            .unwrap();
        remove_files_with_registry(removal, |_| Ok(())).unwrap();
        assert!(!directory.exists());
    }
}
