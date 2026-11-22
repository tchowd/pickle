//! Content-free support reports. Construct fields explicitly; never serialize
//! settings, errors, environment variables, source state or stored answers.
use crate::{controller::AppState, storage::Settings};
use pickle_platform::{Capabilities, PermissionStatus};
use rune::{FileDialogOptions, FileDialogResult, Tx};
use serde::Serialize;
use std::{fs, io::Write, path::Path};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Report {
    schema_version: u8,
    build: Build,
    platform: Platform,
    policy: Policy,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
struct Build {
    app_version: &'static str,
    framework_revision: &'static str,
    target: &'static str,
    source_commit: Option<&'static str>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
struct Platform {
    os: &'static str,
    architecture: &'static str,
    capabilities: CapabilityFlags,
    permissions: PermissionFlags,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
struct CapabilityFlags {
    selection: bool,
    menu_bar: bool,
    modifier_gesture: bool,
    window_capture_ocr: bool,
    source_audio: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
struct PermissionFlags {
    accessibility: bool,
    screen_capture: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
struct Policy {
    mode: Mode,
    strict_offline: bool,
    same_device: bool,
    streaming: bool,
    paused: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
enum Mode {
    Local,
    Online,
    Unknown,
}

impl Report {
    fn capture(
        settings: &Settings,
        paused: bool,
        capabilities: Capabilities,
        permissions: PermissionStatus,
    ) -> Self {
        Self {
            schema_version: 1,
            build: Build {
                app_version: env!("CARGO_PKG_VERSION"),
                framework_revision: env!("PICKLE_RUNE_REVISION"),
                target: env!("PICKLE_BUILD_TARGET"),
                source_commit: match env!("PICKLE_BUILD_COMMIT") {
                    "" => None,
                    commit => Some(commit),
                },
            },
            platform: Platform {
                os: std::env::consts::OS,
                architecture: std::env::consts::ARCH,
                capabilities: CapabilityFlags {
                    selection: capabilities.selection,
                    menu_bar: capabilities.menu_bar,
                    modifier_gesture: capabilities.modifier_gesture,
                    window_capture_ocr: capabilities.window_capture_ocr,
                    source_audio: capabilities.source_audio,
                },
                permissions: PermissionFlags {
                    accessibility: permissions.accessibility,
                    screen_capture: permissions.screen_capture,
                },
            },
            policy: Policy {
                mode: match settings.mode.as_str() {
                    "Local" => Mode::Local,
                    "Online" => Mode::Online,
                    _ => Mode::Unknown,
                },
                strict_offline: settings.strict_offline,
                same_device: settings.same_device,
                streaming: settings.streaming,
                paused,
            },
        }
    }

    /// This exact JSON is previewed and exported. No timestamps or unique
    /// machine/session identifiers are added while writing the file.
    pub fn json(&self) -> String {
        let mut encoded = serde_json::to_string_pretty(self).expect("fixed diagnostic schema");
        encoded.push('\n');
        encoded
    }
}

#[derive(Default)]
pub struct DiagnosticsState {
    pub report: Option<Report>,
    pub status: Status,
    attempt: u64,
}
#[derive(Default)]
pub enum Status {
    #[default]
    Idle,
    Reviewed,
    Choosing,
    Writing,
    Exported,
    Failed(&'static str),
}
impl Status {
    pub fn busy(&self) -> bool {
        matches!(self, Self::Choosing | Self::Writing)
    }
}

impl AppState {
    pub fn review_diagnostics(&mut self) {
        if self.diagnostics.status.busy() {
            return;
        }
        // Capability/permission probes do not request access or collect source
        // content. Headless fixtures avoid native probes entirely.
        let (capabilities, permissions) = if self.native_enabled {
            (
                pickle_platform::capabilities(),
                pickle_platform::permission_status(),
            )
        } else {
            (
                Capabilities {
                    selection: false,
                    menu_bar: false,
                    modifier_gesture: false,
                    window_capture_ocr: false,
                    source_audio: false,
                },
                PermissionStatus {
                    accessibility: false,
                    screen_capture: false,
                },
            )
        };
        self.diagnostics.attempt = self.diagnostics.attempt.wrapping_add(1);
        self.diagnostics.report = Some(Report::capture(
            &self.settings,
            self.paused,
            capabilities,
            permissions,
        ));
        self.diagnostics.status = Status::Reviewed;
    }

    pub fn export_diagnostics(&mut self, tx: &mut Tx) {
        if self.diagnostics.status.busy() {
            return;
        }
        let Some(report) = self.diagnostics.report.clone() else {
            return;
        };
        let attempt = self.diagnostics.attempt;
        self.diagnostics.status = Status::Choosing;
        tx.save_file(
            FileDialogOptions {
                title: "Export reviewed diagnostics".into(),
                file_name: Some("Pickle-diagnostics.json".into()),
                ..Default::default()
            }
            .filter("JSON report", &["json"]),
            move |state: &mut AppState, result, _| {
                state.diagnostics_destination(attempt, report, result);
            },
        );
    }

    fn diagnostics_destination(&mut self, attempt: u64, report: Report, result: FileDialogResult) {
        if self.diagnostics.attempt != attempt
            || !matches!(self.diagnostics.status, Status::Choosing)
        {
            return;
        }
        let path = match result {
            Ok(None) => {
                self.diagnostics.status = Status::Reviewed;
                return;
            }
            Err(_) => {
                self.diagnostics.status =
                    Status::Failed("The save dialog could not open. No report was written.");
                return;
            }
            Ok(Some(path)) => path,
        };
        // Automated scenes cannot write a report to an arbitrary host path.
        let (true, Some(services), Some(handle)) = (
            self.native_enabled,
            self.services.clone(),
            self.handle.clone(),
        ) else {
            self.diagnostics.status = Status::Failed(
                "Export is available in the running desktop application. No report was written.",
            );
            return;
        };
        self.diagnostics.status = Status::Writing;
        services.runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || export_atomic(&path, &report)).await;
            handle.update(move |state, _| {
                if state.diagnostics.attempt != attempt {
                    return;
                }
                state.diagnostics.status = match result {
                    Ok(Ok(())) => Status::Exported,
                    _ => Status::Failed("Could not export diagnostics. The destination may be unavailable or protected."),
                };
            });
        });
    }
}

/// Write only after the user chooses a destination. Stage beside the destination
/// for atomic replacement; never create or change the permissions of its folder.
fn export_atomic(path: &Path, report: &Report) -> Result<(), &'static str> {
    if path.file_name().is_none() {
        return Err("Choose a report file.");
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            let linked = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let linked = metadata.file_type().is_symlink();
            if linked || !metadata.is_file() {
                return Err("Choose a regular report file.");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not inspect the export destination."),
    }
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut staged =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| "Could not stage the report.")?;
    protect_export(staged.path())?;
    staged
        .write_all(report.json().as_bytes())
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|_| "Could not write the report.")?;
    staged
        .persist(path)
        .map_err(|_| "Could not replace the report.")?;
    Ok(())
}

fn protect_export(path: &Path) -> Result<(), &'static str> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| "Could not protect the report.")
    }
    #[cfg(windows)]
    {
        use std::{ffi::c_void, os::windows::ffi::OsStrExt};
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text: *const u16,
                revision: u32,
                descriptor: *mut *mut c_void,
                size: *mut u32,
            ) -> i32;
            fn SetFileSecurityW(
                path: *const u16,
                information: u32,
                descriptor: *const c_void,
            ) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn LocalFree(memory: *mut c_void) -> *mut c_void;
        }
        let sddl: Vec<u16> = "D:P(A;;FA;;;SY)(A;;FA;;;OW)"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: terminated strings, valid output storage, and one LocalFree
        // for the descriptor Windows allocates after the DACL has been applied.
        unsafe {
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err("Could not create private report permissions.");
            }
            let result = SetFileSecurityW(filename.as_ptr(), 0x8000_0004, descriptor);
            LocalFree(descriptor);
            if result == 0 {
                Err("Could not protect the report.")
            } else {
                Ok(())
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err("Private report export is unavailable on this platform.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn report() -> Report {
        Report::capture(
            &Settings::default(),
            true,
            Capabilities {
                selection: true,
                menu_bar: true,
                modifier_gesture: false,
                window_capture_ocr: true,
                source_audio: false,
            },
            PermissionStatus {
                accessibility: false,
                screen_capture: true,
            },
        )
    }

    #[test]
    fn report_has_an_exact_allowlist_and_no_connection_or_content_data() {
        let secret = "PRIVATE_SENTINEL_94f129";
        let settings = Settings {
            local_address: secret.into(),
            local_model: secret.into(),
            local_vision_model: secret.into(),
            account_id: secret.into(),
            cloud_model: secret.into(),
            local_consent: secret.into(),
            context_consent_destination: secret.into(),
            transcriber_executable: secret.into(),
            transcriber_model: secret.into(),
            transcriber_language: secret.into(),
            exclusions: vec![secret.into()],
            ..Settings::default()
        };
        let report = Report::capture(
            &settings,
            false,
            Capabilities {
                selection: false,
                menu_bar: true,
                modifier_gesture: false,
                window_capture_ocr: false,
                source_audio: false,
            },
            PermissionStatus {
                accessibility: false,
                screen_capture: false,
            },
        );
        let encoded = report.json();
        assert!(!encoded.contains(secret));
        let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        fn keys(value: &serde_json::Value) -> Vec<&str> {
            let mut keys: Vec<_> = value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            keys
        }
        assert_eq!(
            keys(&value),
            ["build", "platform", "policy", "schema_version"]
        );
        assert_eq!(
            keys(&value["build"]),
            [
                "app_version",
                "framework_revision",
                "source_commit",
                "target"
            ]
        );
        assert_eq!(
            keys(&value["platform"]),
            ["architecture", "capabilities", "os", "permissions"]
        );
        assert_eq!(
            keys(&value["platform"]["capabilities"]),
            [
                "menu_bar",
                "modifier_gesture",
                "selection",
                "source_audio",
                "window_capture_ocr"
            ]
        );
        assert_eq!(
            keys(&value["platform"]["permissions"]),
            ["accessibility", "screen_capture"]
        );
        assert_eq!(
            keys(&value["policy"]),
            [
                "mode",
                "paused",
                "same_device",
                "streaming",
                "strict_offline"
            ]
        );
        assert_eq!(report.json(), encoded);
    }

    #[test]
    fn an_unrecognized_mode_is_not_exported_as_arbitrary_text() {
        let settings = Settings {
            mode: "PRIVATE_UNKNOWN_MODE".into(),
            ..Settings::default()
        };
        let mut value = report();
        value.policy = Report::capture(
            &settings,
            false,
            Capabilities {
                selection: false,
                menu_bar: false,
                modifier_gesture: false,
                window_capture_ocr: false,
                source_audio: false,
            },
            PermissionStatus {
                accessibility: false,
                screen_capture: false,
            },
        )
        .policy;
        assert!(value.json().contains("Unknown"));
        assert!(!value.json().contains("PRIVATE_UNKNOWN_MODE"));
    }

    #[test]
    fn atomic_export_matches_the_preview_and_leaves_no_staging_files() {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = directory.path().join("reviewed.json");
        fs::write(&path, b"previous report").unwrap();
        let report = report();
        export_atomic(&path, &report).unwrap();
        assert_eq!(fs::read(&path).unwrap(), report.json().as_bytes());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
    }

    #[test]
    fn invalid_destinations_never_create_directories_or_replace_existing_data() {
        let directory = tempfile::tempdir().unwrap();
        assert!(export_atomic(&directory.path().join("missing/report.json"), &report()).is_err());
        assert!(!directory.path().join("missing").exists());
        assert!(export_atomic(directory.path(), &report()).is_err());
        assert!(directory.path().is_dir());
        #[cfg(unix)]
        {
            let target = directory.path().join("retained");
            fs::write(&target, b"retained data").unwrap();
            let link = directory.path().join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(export_atomic(&link, &report()).is_err());
            assert_eq!(fs::read(target).unwrap(), b"retained data");
        }
    }

    #[test]
    fn preview_is_required_and_dialog_cancellation_does_not_write() {
        let mut runtime = Runtime::new(
            AppState::default(),
            |s, _| crate::views::diagnostics_tools(s),
            (560.0, 720.0),
            Env::new(true, 1.0),
        );
        runtime.frame(0.0);
        runtime.update(|s, tx| s.export_diagnostics(tx));
        assert!(runtime.file_dialog().is_none());
        runtime.update(|s, _| s.review_diagnostics());
        let reviewed = runtime.state.diagnostics.report.as_ref().unwrap().json();
        runtime.update(|s, tx| s.export_diagnostics(tx));
        assert_eq!(runtime.file_dialog().unwrap().0, rune::FileDialogKind::Save);
        assert!(matches!(runtime.state.diagnostics.status, Status::Choosing));
        runtime.resolve_file_dialog(None).unwrap();
        runtime.frame(1.0 / 60.0);
        assert!(matches!(runtime.state.diagnostics.status, Status::Reviewed));
        assert_eq!(
            runtime.state.diagnostics.report.as_ref().unwrap().json(),
            reviewed
        );
        assert!(runtime.state.services.is_none());
    }

    #[test]
    fn stale_dialog_answers_and_headless_export_cannot_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("not-written.json");
        let mut state = AppState::default();
        state.review_diagnostics();
        let report = state.diagnostics.report.clone().unwrap();
        state.diagnostics.status = Status::Choosing;
        state.diagnostics_destination(
            state.diagnostics.attempt + 1,
            report.clone(),
            Ok(Some(path.clone())),
        );
        assert!(matches!(state.diagnostics.status, Status::Choosing));
        state.diagnostics_destination(state.diagnostics.attempt, report, Ok(Some(path.clone())));
        assert!(matches!(state.diagnostics.status, Status::Failed(_)));
        assert!(!path.exists());
    }
}
