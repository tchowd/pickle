//! Renderer-independent failure detection and bounded post-loop cleanup.
//! Never retain or report Rune's arbitrary error strings.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

// Each of two serial writer stages has this budget: at most ten seconds total
// after the loop and monitor have stopped, excluding explicit user dialog ACK.
pub const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

pub fn interactive_failure(arguments: &[String], headless: bool) -> bool {
    !headless
        && arguments
            .iter()
            .all(|arg| matches!(arg.as_str(), "--sample" | "--background"))
}

pub struct Cleanup {
    pub safe_to_release: bool,
    pub failed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    Success,
    ReaderFailure,
    WriteFailure,
    UnsafeShutdown,
}

/// An unsafe deadline must terminate immediately with the lease still held;
/// waiting for a modal acknowledgment would break the bounded-exit guarantee.
/// Safe cleanup retains the lease through presentation: the executable is still
/// loaded while a modal dialog waits, and installers must continue seeing it.
pub fn exit_decision(
    run_failed: bool,
    window_failed: bool,
    interactive: bool,
    cleaned: Cleanup,
    present: impl FnOnce(),
    release: impl FnOnce(),
) -> Exit {
    if !cleaned.safe_to_release {
        return Exit::UnsafeShutdown;
    }
    let decision = if run_failed || window_failed {
        if interactive {
            present();
        }
        Exit::ReaderFailure
    } else if cleaned.failed {
        Exit::WriteFailure
    } else {
        Exit::Success
    };
    release();
    decision
}

/// Confirm both writer stages stopped, retaining the lease for the caller's
/// terminal presentation/exit. A completed write error remains a failure.
pub fn cleanup(
    cancel: impl FnOnce(),
    runtime: impl FnOnce() -> bool,
    geometry: impl FnOnce() -> Result<Result<(), String>, ()>,
) -> Cleanup {
    cancel();
    if !runtime() {
        return Cleanup {
            safe_to_release: false,
            failed: true,
        };
    }
    match geometry() {
        Err(()) => Cleanup {
            safe_to_release: false,
            failed: true,
        },
        Ok(result) => Cleanup {
            safe_to_release: true,
            failed: result.is_err(),
        },
    }
}

pub struct WindowMonitor {
    stop: mpsc::Sender<()>,
    worker: JoinHandle<()>,
    failed: Arc<AtomicBool>,
}
impl WindowMonitor {
    pub fn start(
        failed_window: impl FnMut() -> bool + Send + 'static,
        quit: impl FnOnce() + Send + 'static,
    ) -> Result<Self, ()> {
        Self::with_interval(failed_window, quit, Duration::from_millis(200))
    }
    fn with_interval(
        mut failed_window: impl FnMut() -> bool + Send + 'static,
        quit: impl FnOnce() + Send + 'static,
        interval: Duration,
    ) -> Result<Self, ()> {
        let (stop, stopped) = mpsc::channel();
        let failed = Arc::new(AtomicBool::new(false));
        let signal = failed.clone();
        let worker = thread::Builder::new()
            .name("pickle-window-health".into())
            .spawn(move || {
                loop {
                    match stopped.try_recv() {
                        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => return,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut failed_window))
                        .unwrap_or(true)
                    {
                        signal.store(true, Ordering::Release);
                        quit();
                        return;
                    }
                    match stopped.recv_timeout(interval) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        _ => return,
                    }
                }
            })
            .map_err(|_| ())?;
        Ok(Self {
            stop,
            worker,
            failed,
        })
    }
    pub fn finish(self) -> bool {
        let _ = self.stop.send(());
        let panicked = self.worker.join().is_err();
        panicked || self.failed.load(Ordering::Acquire)
    }
}

/// A failure to confirm completion is unsafe: the caller must keep its instance
/// lease until process termination. A dropped JoinHandle does not stop a writer.
pub fn finish_with_timeout(
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
    timeout: Duration,
) -> Result<Result<(), String>, ()> {
    let (done, finished) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("pickle-final-write".into())
        .spawn(move || {
            let _ = done.send(work());
        })
        .map_err(|_| ())?;
    let result = finished.recv_timeout(timeout).map_err(|_| ())?;
    worker.join().map_err(|_| ())?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_window_error_latches_once_and_quits_without_a_ready_callback() {
        let fault = Arc::new(AtomicBool::new(false));
        let probe = fault.clone();
        let (quit, called) = mpsc::channel();
        let monitor = WindowMonitor::with_interval(
            move || probe.load(Ordering::Acquire),
            move || quit.send(()).unwrap(),
            Duration::from_millis(1),
        )
        .unwrap();
        assert!(called.recv_timeout(Duration::from_millis(10)).is_err());
        fault.store(true, Ordering::Release);
        called.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(monitor.finish());
        assert!(called.try_recv().is_err());
    }
    #[test]
    fn background_without_any_window_attempt_is_not_a_failure() {
        let monitor = WindowMonitor::with_interval(
            || false,
            || panic!("A windowless app must not be quit"),
            Duration::from_millis(1),
        )
        .unwrap();
        assert!(!monitor.finish());
    }
    #[test]
    fn cleanup_orders_cancellation_drain_and_flush_without_releasing_the_lease() {
        let order = std::cell::RefCell::new(Vec::new());
        let cleaned = cleanup(
            || order.borrow_mut().push("cancel"),
            || {
                order.borrow_mut().push("runtime");
                true
            },
            || {
                order.borrow_mut().push("geometry");
                Ok(Ok(()))
            },
        );
        assert!(cleaned.safe_to_release && !cleaned.failed);
        assert_eq!(*order.borrow(), ["cancel", "runtime", "geometry"]);
        let blocked = cleanup(|| {}, || false, || panic!("Runtime writers still running"));
        assert!(!blocked.safe_to_release && blocked.failed);
        let blocked = cleanup(|| {}, || true, || Err(()));
        assert!(!blocked.safe_to_release && blocked.failed);
    }
    #[test]
    fn returned_or_window_failure_presents_once_after_safe_cleanup_only() {
        for (returned, window, interactive, write_failed, expected, show) in [
            (true, false, true, false, Exit::ReaderFailure, true),
            (false, true, true, false, Exit::ReaderFailure, true),
            (true, true, true, false, Exit::ReaderFailure, true),
            (false, false, true, false, Exit::Success, false),
            (true, false, false, false, Exit::ReaderFailure, false),
            (false, false, true, true, Exit::WriteFailure, false),
            (true, false, true, true, Exit::ReaderFailure, true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let data = directory.path().join("Pickle");
            let lease = std::cell::RefCell::new(Some(
                match crate::instance::InstanceGuard::acquire(&data).unwrap() {
                    crate::instance::InstanceStatus::Primary(lease) => lease,
                    _ => panic!("Expected primary lease"),
                },
            ));
            let order = std::cell::RefCell::new(Vec::new());
            let cleaned = cleanup(
                || order.borrow_mut().push("cancel"),
                || true,
                || {
                    order.borrow_mut().push("flush");
                    Ok(if write_failed {
                        Err("Controlled write error".into())
                    } else {
                        Ok(())
                    })
                },
            );
            assert!(crate::instance::is_running(&data).unwrap());
            assert_eq!(
                exit_decision(
                    returned,
                    window,
                    interactive,
                    cleaned,
                    || {
                        assert!(show, "This exit must not present a launch dialog");
                        assert!(lease.borrow().is_some());
                        assert!(crate::instance::is_running(&data).unwrap());
                        order.borrow_mut().push("present");
                        // The injected modal acknowledgment returns here while the
                        // actual lease still excludes installers/other instances.
                    },
                    || {
                        assert!(crate::instance::is_running(&data).unwrap());
                        drop(lease.borrow_mut().take());
                        order.borrow_mut().push("release");
                    }
                ),
                expected
            );
            assert!(!crate::instance::is_running(&data).unwrap());
            let expected_order = if show {
                vec!["cancel", "flush", "present", "release"]
            } else {
                vec!["cancel", "flush", "release"]
            };
            assert_eq!(*order.borrow(), expected_order);
        }
    }
    #[test]
    fn cli_and_headless_failures_never_request_a_native_dialog() {
        assert!(interactive_failure(&[], false));
        assert!(interactive_failure(&["--background".into()], false));
        assert!(!interactive_failure(&[], true));
        for option in [
            "--help",
            "--version",
            "--snap",
            "--tree",
            "--verify-update-manifest",
        ] {
            assert!(!interactive_failure(&[option.into()], false));
        }
    }
    #[test]
    fn geometry_flush_finishes_before_the_instance_lease_is_released() {
        use crate::{geometry_store, instance};
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("Pickle");
        let lease = match instance::InstanceGuard::acquire(&data).unwrap() {
            instance::InstanceStatus::Primary(lease) => lease,
            _ => panic!("Expected primary lease"),
        };
        let worker = geometry_store::Worker::start(data.clone()).unwrap().worker;
        let frame = geometry_store::WindowFrame {
            position: (80, 90),
            logical_size: (560.0, 640.0),
            scale: 1.0,
        };
        worker.enqueue(frame).unwrap();
        assert_eq!(
            finish_with_timeout(move || worker.finish(), Duration::from_secs(2)),
            Ok(Ok(()))
        );
        assert!(instance::is_running(&data).unwrap());
        drop(lease);
        assert!(!instance::is_running(&data).unwrap());
        let lease = match instance::InstanceGuard::acquire(&data).unwrap() {
            instance::InstanceStatus::Primary(lease) => lease,
            _ => panic!("Completed cleanup must admit a new primary"),
        };
        let reloaded = geometry_store::Worker::start(data).unwrap();
        assert_eq!(reloaded.loaded.unwrap(), Some(frame));
        reloaded.worker.finish().unwrap();
        drop(lease);
    }
    #[test]
    fn timed_out_writer_requires_retaining_the_lease_until_termination() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("Pickle");
        let lease = match crate::instance::InstanceGuard::acquire(&data).unwrap() {
            crate::instance::InstanceStatus::Primary(lease) => lease,
            _ => panic!("Expected primary lease"),
        };
        let (release, wait) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        assert_eq!(
            finish_with_timeout(
                move || {
                    wait.recv().unwrap();
                    done.send(()).unwrap();
                    Ok(())
                },
                Duration::from_millis(10)
            ),
            Err(())
        );
        assert!(crate::instance::is_running(&data).unwrap());
        assert_eq!(
            exit_decision(
                true,
                true,
                true,
                Cleanup {
                    safe_to_release: false,
                    failed: true
                },
                || panic!("Unsafe shutdown must terminate without waiting for a dialog"),
                || panic!("Unsafe shutdown must not release the real lease")
            ),
            Exit::UnsafeShutdown
        );
        assert!(crate::instance::is_running(&data).unwrap());
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(lease);
    }
}
