//! Installer-facing commands run before application services, source capture, or native windows.
use std::path::{Path, PathBuf};

pub const RUNNING_EXIT: i32 = 10;
pub const FAILED_EXIT: i32 = 1;

pub fn handle(arguments: &[String]) -> Option<i32> {
    let command = arguments.iter().find(|argument| {
        matches!(
            argument.as_str(),
            "--check-running" | "--disconnect-browsers"
        )
    })?;
    if arguments.len() != 1 {
        eprintln!("Installer maintenance commands cannot be combined with other options.");
        return Some(FAILED_EXIT);
    }
    let directory = crate::storage::Repository::default_directory();
    Some(run(command, &directory))
}

fn application_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|_| "Could not identify the application for browser cleanup.".to_string())?;
    let application = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .unwrap_or(&executable);
    std::fs::canonicalize(application)
        .map_err(|_| "Could not resolve the current application for browser cleanup.".to_string())
}

fn run(command: &str, directory: &Path) -> i32 {
    match crate::instance::is_running(directory) {
        Ok(true) => {
            eprintln!(
                "Quit Pickle from its menu bar or system tray before installing or removing it."
            );
            RUNNING_EXIT
        }
        Err(error) => {
            eprintln!("{error}");
            FAILED_EXIT
        }
        Ok(false) if command == "--check-running" => 0,
        Ok(false) => match application_path().and_then(|application| {
            pickle_providers::browser::uninstall_hosts_for_application(&application, directory)
                .map_err(|error| error.to_string())
        }) {
            Ok(count) => {
                println!(
                    "Disconnected {count} browser host(s) owned by this application. Saved data and credentials were retained."
                );
                0
            }
            Err(error) => {
                eprintln!("{error}");
                FAILED_EXIT
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_maintenance_options_continue_normal_dispatch() {
        assert_eq!(handle(&["--version".into()]), None);
        assert_eq!(
            handle(&["--check-running".into(), "--sample".into()]),
            Some(FAILED_EXIT)
        );
    }

    #[test]
    fn running_command_returns_installer_codes_without_reopening() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("absent");
        assert_eq!(run("--check-running", &directory), 0);
        assert!(!directory.exists());
        let mut guard = match crate::instance::InstanceGuard::acquire(root.path()).unwrap() {
            crate::instance::InstanceStatus::Primary(guard) => guard,
            _ => panic!("Expected primary"),
        };
        let events = guard.take_receiver().unwrap();
        assert_eq!(run("--check-running", root.path()), RUNNING_EXIT);
        assert_eq!(run("--disconnect-browsers", root.path()), RUNNING_EXIT);
        assert!(events.try_recv().is_err());
    }
}
