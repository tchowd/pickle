//! Version-independent package activation; identities are read from the calling app's OS token.
use super::error;
use pickle_core::PickleError;

#[cfg(not(target_os = "windows"))]
pub(super) fn uses_external_location() -> Result<bool, PickleError> {
    Ok(false)
}

#[cfg(target_os = "windows")]
pub(super) fn uses_external_location() -> Result<bool, PickleError> {
    use windows::{
        Win32::{
            Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
            Storage::Packaging::Appx::{
                GetCurrentPackagePath2, PackagePathType, PackagePathType_Effective,
                PackagePathType_Install,
            },
        },
        core::PWSTR,
    };
    fn path(kind: PackagePathType) -> Result<std::path::PathBuf, PickleError> {
        let mut length = 0;
        if unsafe { GetCurrentPackagePath2(kind, &mut length, None) } != ERROR_INSUFFICIENT_BUFFER
            || !(2..=32768).contains(&length)
        {
            return Err(error(
                "Could not inspect this package's installation location.",
            ));
        }
        let mut value = vec![0u16; length as usize];
        let status =
            unsafe { GetCurrentPackagePath2(kind, &mut length, Some(PWSTR(value.as_mut_ptr()))) };
        if status != ERROR_SUCCESS
            || length == 0
            || length as usize > value.len()
            || value[length as usize - 1] != 0
        {
            return Err(error(
                "Could not read this package's installation location.",
            ));
        }
        let value = String::from_utf16(&value[..length as usize - 1])
            .map_err(|_| error("Invalid package location."))?;
        std::fs::canonicalize(value).map_err(|_| error("This package location is unavailable."))
    }
    Ok(path(PackagePathType_Effective)? != path(PackagePathType_Install)?)
}

// Keep the application ID tied to our checked-in MSIX manifest, not arbitrary shell arguments.
pub(super) fn valid_aumid(value: &str) -> bool {
    let Some((family, application)) = value.split_once('!') else {
        return false;
    };
    let Some((name, publisher)) = family.rsplit_once('_') else {
        return false;
    };
    application == "Pickle"
        && (3..=50).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && publisher.len() == 13
        && publisher
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

#[cfg(not(target_os = "windows"))]
pub(super) fn current_aumid() -> Result<Option<String>, PickleError> {
    Ok(None)
}

#[cfg(target_os = "windows")]
pub(super) fn current_aumid() -> Result<Option<String>, PickleError> {
    use windows::{
        Win32::{
            Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
            Storage::Packaging::Appx::{
                GetCurrentApplicationUserModelId, GetCurrentPackageFamilyName,
            },
        },
        core::PWSTR,
    };
    let mut family_size = 0;
    let status = unsafe { GetCurrentPackageFamilyName(&mut family_size, None) };
    if status == APPMODEL_ERROR_NO_PACKAGE {
        return Ok(None);
    }
    if status != ERROR_INSUFFICIENT_BUFFER || family_size > 256 {
        return Err(error(
            "Could not verify this application's Windows package identity.",
        ));
    }
    let mut length = 0;
    let status = unsafe { GetCurrentApplicationUserModelId(&mut length, None) };
    if status != ERROR_INSUFFICIENT_BUFFER || !(2..=256).contains(&length) {
        return Err(error(
            "Could not inspect this packaged application's activation identity.",
        ));
    }
    let mut buffer = vec![0u16; length as usize];
    let status =
        unsafe { GetCurrentApplicationUserModelId(&mut length, Some(PWSTR(buffer.as_mut_ptr()))) };
    if status != ERROR_SUCCESS
        || length as usize > buffer.len()
        || length == 0
        || buffer[length as usize - 1] != 0
    {
        return Err(error(
            "Could not read this application's activation identity.",
        ));
    }
    let value = String::from_utf16(&buffer[..length as usize - 1])
        .map_err(|_| error("Invalid package activation identity."))?;
    if !valid_aumid(&value) {
        return Err(error(
            "This package does not expose the supported Pickle application identity.",
        ));
    }
    Ok(Some(value))
}

pub(super) fn activate(aumid: &str) -> Result<(), PickleError> {
    if !valid_aumid(aumid) {
        return Err(error("Invalid packaged browser activation identity."));
    }
    #[cfg(target_os = "windows")]
    {
        let aumid = aumid.to_owned();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = send.send(activate_windows(&aumid));
        });
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| error("Windows package activation timed out. Open Pickle and retry."))?
    }
    #[cfg(not(target_os = "windows"))]
    Err(error(
        "Packaged Windows activation is unavailable on this platform.",
    ))
}

#[cfg(target_os = "windows")]
fn activate_windows(aumid: &str) -> Result<(), PickleError> {
    use windows::{
        Win32::{
            System::Com::{
                CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            UI::Shell::{
                AO_NOERRORUI, ApplicationActivationManager, IApplicationActivationManager,
            },
        },
        core::HSTRING,
    };
    // This runs in the dedicated native-host process, never on the app's UI thread.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|_| error("Could not initialize Windows app activation."))?;
        let result = (|| {
            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
                    .map_err(|_| error("Windows package activation is unavailable."))?;
            manager.ActivateApplication(&HSTRING::from(aumid), &HSTRING::from("--background"), AO_NOERRORUI).map_err(|_| error("Pickle's package could not be activated. Open Pickle and reconnect the browser host, or disconnect it before removing the package."))?;
            Ok(())
        })();
        CoUninitialize();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_identity_accepts_only_our_application_without_shell_syntax() {
        assert!(valid_aumid("Pickle.Rune.CI_123456789abcd!Pickle"));
        for bad in [
            "",
            "Pickle.Rune.CI_123456789abcd!Other",
            "Pickle.Rune.CI_123456789abcd!Pickle --evil",
            "Pickle.Rune.CI_123456789abcd!Pickle!Other",
            "Pickle;Run_123456789abcd!Pickle",
            "Pickle.Rune.CI_123!Pickle",
        ] {
            assert!(!valid_aumid(bad), "{bad}");
        }
    }
}
