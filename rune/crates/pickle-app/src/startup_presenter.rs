//! A fresh Mac process owns the fatal dialog: AppKit's modal loop must never
//! re-enter the stopped winit event loop in the parent. No error/source payload
//! crosses this boundary, and the parent retains its instance lease until ACK.
#[cfg(any(target_os = "macos", test))]
use std::{
    path::Path,
    process::{Command, Stdio},
};

const HELPER_ARGUMENT: &str = "--pickle-native-reader-failure";

#[derive(Debug, PartialEq, Eq)]
enum HelperArguments {
    Unrelated,
    Present,
    Invalid,
}
fn classify(arguments: &[String]) -> HelperArguments {
    if arguments.len() == 1 && arguments[0] == HELPER_ARGUMENT {
        HelperArguments::Present
    } else if arguments.iter().any(|arg| arg.starts_with(HELPER_ARGUMENT)) {
        HelperArguments::Invalid
    } else {
        HelperArguments::Unrelated
    }
}

fn dispatch(arguments: &[String], supported: bool, show: impl FnOnce() -> bool) -> Option<i32> {
    match classify(arguments) {
        HelperArguments::Unrelated => None,
        HelperArguments::Invalid => Some(2),
        HelperArguments::Present if !supported => Some(2),
        HelperArguments::Present => Some(if show() { 0 } else { 1 }),
    }
}

/// Must be the first dispatch, before Rune, repositories, runtimes or leases.
/// Unsupported or combined helper arguments fail without showing anything.
pub fn handle_helper(arguments: &[String]) -> Option<i32> {
    dispatch(arguments, cfg!(target_os = "macos"), || {
        pickle_platform::present_reader_failure().is_ok()
    })
}

#[cfg(any(target_os = "macos", test))]
fn helper_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg(HELPER_ARGUMENT)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(any(target_os = "macos", test))]
fn wait_for_helper(executable: &Path, wait: impl FnOnce(&mut Command) -> bool) -> bool {
    wait(&mut helper_command(executable))
}

/// Wait for the helper's acknowledgment. The caller must retain its actual
/// instance lease through this entire call; a modal wait has no timeout.
pub fn present() -> bool {
    #[cfg(target_os = "macos")]
    {
        let Ok(executable) = std::env::current_exe() else {
            return false;
        };
        wait_for_helper(&executable, |command| {
            command.status().is_ok_and(|status| status.success())
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        pickle_platform::present_reader_failure().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sole_exact_helper_argument_is_the_only_presentation_request() {
        for (args, supported, expected, called) in [
            (vec![HELPER_ARGUMENT.into()], true, Some(0), true),
            (vec![HELPER_ARGUMENT.into()], false, Some(2), false),
            (
                vec![HELPER_ARGUMENT.into(), "--sample".into()],
                true,
                Some(2),
                false,
            ),
            (
                vec!["--snap".into(), HELPER_ARGUMENT.into()],
                true,
                Some(2),
                false,
            ),
            (
                vec![format!("{HELPER_ARGUMENT}=anything")],
                true,
                Some(2),
                false,
            ),
            (
                vec![format!("{HELPER_ARGUMENT}-extra")],
                true,
                Some(2),
                false,
            ),
            (vec![], true, None, false),
            (vec!["--sample".into()], true, None, false),
        ] {
            let shown = std::cell::Cell::new(false);
            assert_eq!(
                dispatch(&args, supported, || {
                    shown.set(true);
                    true
                }),
                expected
            );
            assert_eq!(shown.get(), called);
        }
        assert_eq!(dispatch(&[HELPER_ARGUMENT.into()], true, || false), Some(1));
    }

    #[test]
    fn helper_command_has_one_fixed_argument_and_no_environment_overrides() {
        let executable = Path::new("/fixed/PickleRune");
        let command = helper_command(executable);
        assert_eq!(command.get_program(), executable.as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), [HELPER_ARGUMENT]);
        // This checks overrides only. The explicit env_clear() above controls
        // inheritance; get_envs() cannot inspect that Command setting.
        assert!(command.get_envs().next().is_none());
    }

    #[test]
    fn parent_owns_actual_lease_through_helper_acknowledgment_and_failure() {
        for acknowledged in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let directory = root.path().join("Pickle");
            let lease = std::cell::RefCell::new(Some(
                match crate::instance::InstanceGuard::acquire(&directory).unwrap() {
                    crate::instance::InstanceStatus::Primary(lease) => lease,
                    _ => panic!("Expected primary lease"),
                },
            ));
            let order = std::cell::RefCell::new(Vec::new());
            let decision = crate::lifecycle::exit_decision(
                true,
                false,
                true,
                crate::lifecycle::Cleanup {
                    safe_to_release: true,
                    failed: false,
                },
                || {
                    // The real command's status() waits at this exact boundary.
                    // Never invoke a native dialog in a deterministic test.
                    assert!(lease.borrow().is_some());
                    assert!(crate::instance::is_running(&directory).unwrap());
                    assert_eq!(
                        wait_for_helper(Path::new("/fixed/PickleRune"), |command| {
                            assert_eq!(command.get_args().collect::<Vec<_>>(), [HELPER_ARGUMENT]);
                            assert!(crate::instance::is_running(&directory).unwrap());
                            acknowledged
                        }),
                        acknowledged
                    );
                    assert!(crate::instance::is_running(&directory).unwrap());
                    order.borrow_mut().push("helper-returned");
                },
                || {
                    drop(lease.borrow_mut().take());
                    order.borrow_mut().push("released");
                },
            );
            assert_eq!(decision, crate::lifecycle::Exit::ReaderFailure);
            assert_eq!(*order.borrow(), ["helper-returned", "released"]);
            assert!(!crate::instance::is_running(&directory).unwrap());
        }
    }
}
