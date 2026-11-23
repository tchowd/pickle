//! Background handles are shareable; the actual runtime belongs to main alone.
use std::{
    future::Future,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use tokio::{runtime::Runtime, task::JoinHandle};

#[derive(Default)]
struct Jobs {
    closing: bool,
    blocking: usize,
}

pub struct ServiceRuntime {
    handle: tokio::runtime::Handle,
    jobs: Arc<Mutex<Jobs>>,
}

/// Kept outside `Arc<Services>`, on the creating main thread. Tasks cannot own
/// their executor, nor become responsible for dropping it on a Tokio worker.
pub struct RuntimeOwner {
    runtime: Option<Runtime>,
    jobs: Arc<Mutex<Jobs>>,
    _main_thread: PhantomData<Rc<()>>,
}

#[derive(Debug)]
pub struct ShutdownReport {
    /// Only true after the entire runtime, including nested Tokio blocking
    /// jobs, has finished shutting down. A timeout requires process termination
    /// while the instance lease is still held; it does not permit lease release.
    pub finished: bool,
    /// Supplemental count of admitted app blocking jobs, not a completion test.
    pub unfinished_blocking: usize,
}

impl ServiceRuntime {
    pub fn new() -> std::io::Result<(Self, RuntimeOwner)> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let jobs = Arc::new(Mutex::new(Jobs::default()));
        let service = Self {
            handle: runtime.handle().clone(),
            jobs: jobs.clone(),
        };
        Ok((
            service,
            RuntimeOwner {
                runtime: Some(runtime),
                jobs,
                _main_thread: PhantomData,
            },
        ))
    }

    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        if jobs.closing {
            return self.cancelled();
        }
        self.handle.spawn(future)
    }

    pub fn spawn_blocking<F, R>(&self, work: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        if jobs.closing {
            return self.cancelled();
        }
        jobs.blocking += 1;
        let guard = BlockingJob(self.jobs.clone());
        self.handle.spawn_blocking(move || {
            let _guard = guard;
            work()
        })
    }

    fn cancelled<T: Send + 'static>(&self) -> JoinHandle<T> {
        let task = self.handle.spawn(std::future::pending());
        task.abort();
        task
    }
}

struct BlockingJob(Arc<Mutex<Jobs>>);
impl Drop for BlockingJob {
    fn drop(&mut self) {
        let mut jobs = self.0.lock().unwrap_or_else(|p| p.into_inner());
        jobs.blocking -= 1;
    }
}

impl RuntimeOwner {
    pub fn shutdown(mut self, timeout: Duration) -> ShutdownReport {
        self.drain(timeout)
    }

    fn drain(&mut self, timeout: Duration) -> ShutdownReport {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner()).closing = true;
        let runtime = self.runtime.take().expect("Runtime owner consumed once");
        let runtime = Arc::new(Mutex::new(Some(runtime)));
        let worker = runtime.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        // Ordinary Runtime drop waits for all blocking jobs, including jobs
        // spawned internally by diagnostics, updater, Tokio or a provider. A
        // shutdown_timeout call cannot establish that those jobs have stopped.
        let shutdown = std::thread::Builder::new()
            .name("pickle-runtime-shutdown".into())
            .spawn(move || {
                let runtime = worker.lock().unwrap_or_else(|p| p.into_inner()).take();
                drop(runtime);
                let _ = sender.send(());
            })
            .unwrap_or_else(|_| {
                // `runtime` still owns the executor here. Neither an unbounded
                // synchronous drop nor detached writers followed by lease release
                // is safe if a cleanup thread cannot be created.
                eprintln!("Pickle could not start safe background shutdown.");
                std::process::exit(1);
            });
        let finished = receiver.recv_timeout(timeout).is_ok();
        if finished {
            let _ = shutdown.join();
        }
        ShutdownReport {
            finished,
            unfinished_blocking: self.jobs.lock().unwrap_or_else(|p| p.into_inner()).blocking,
        }
    }
}

impl Drop for RuntimeOwner {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // An independent geometry worker may already have lost its owner
            // earlier in this unwind. Draining Tokio alone cannot establish
            // that all writers stopped. Keep the outer lease until termination.
            eprintln!("Pickle stopped after an unexpected application failure.");
            std::process::exit(1);
        }
        if self.runtime.is_some() && !self.drain(Duration::from_secs(5)).finished {
            // A startup/panic unwind must not release the outer instance lease
            // while detached writers remain. Process termination keeps that
            // lease held until the OS also terminates every worker thread.
            eprintln!("Pickle background shutdown did not finish safely.");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{controller::Services, storage::Settings};
    use std::sync::atomic::{AtomicBool, Ordering};

    const DEADLINE: Duration = Duration::from_secs(2);

    #[test]
    fn last_services_clone_can_drop_inside_its_async_job() {
        let (services, owner) = Services::new(None, &Settings::default()).unwrap();
        let services = Arc::new(services);
        let weak = Arc::downgrade(&services);
        let worker = services.clone();
        let (release, wait) = tokio::sync::oneshot::channel();
        let (finished, receive) = mpsc::channel();
        services.runtime.spawn(async move {
            let _ = wait.await;
            drop(worker);
            // A Runtime inside Services would panic before this acknowledgement.
            finished.send(()).unwrap();
        });
        drop(services);
        release.send(()).unwrap();
        receive.recv_timeout(DEADLINE).unwrap();
        assert!(weak.upgrade().is_none());
        assert!(owner.shutdown(DEADLINE).finished);
    }

    #[test]
    fn shutdown_aborts_async_cycles_and_rejects_late_work() {
        struct Finished(Arc<AtomicBool>);
        impl Drop for Finished {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let (runtime, owner) = ServiceRuntime::new().unwrap();
        let runtime = Arc::new(runtime);
        let dropped = Arc::new(AtomicBool::new(false));
        let held = runtime.clone();
        let guard = Finished(dropped.clone());
        runtime.spawn(async move {
            let _held = held;
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        let report = owner.shutdown(DEADLINE);
        assert!(report.finished && report.unfinished_blocking == 0);
        assert!(dropped.load(Ordering::Acquire));
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        assert!(
            runtime
                .spawn_blocking(move || flag.store(true, Ordering::Release))
                .is_finished()
        );
        let flag = ran.clone();
        assert!(
            runtime
                .spawn(async move { flag.store(true, Ordering::Release) })
                .is_finished()
        );
        assert!(!ran.load(Ordering::Acquire));
    }

    #[test]
    fn blocking_writer_is_drained_before_success_is_reported() {
        let (runtime, owner) = ServiceRuntime::new().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("fixed-fixture.txt");
        let written = destination.clone();
        runtime.spawn_blocking(move || std::fs::write(written, b"committed").unwrap());
        let report = owner.shutdown(DEADLINE);
        assert!(report.finished && report.unfinished_blocking == 0);
        assert_eq!(std::fs::read(destination).unwrap(), b"committed");
    }

    #[test]
    fn timed_out_blocking_work_is_reported_and_admission_stays_closed() {
        let (runtime, owner) = ServiceRuntime::new().unwrap();
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (finished, receive) = mpsc::channel();
        runtime.spawn_blocking(move || {
            started.send(()).unwrap();
            wait.recv().unwrap();
            finished.send(()).unwrap();
        });
        ready.recv_timeout(DEADLINE).unwrap();
        let report = owner.shutdown(Duration::ZERO);
        assert!(!report.finished);
        assert_eq!(report.unfinished_blocking, 1);
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        runtime.spawn_blocking(move || flag.store(true, Ordering::Release));
        release.send(()).unwrap();
        receive.recv_timeout(DEADLINE).unwrap();
        assert!(!ran.load(Ordering::Acquire));
    }

    #[test]
    fn nested_tokio_blocking_work_prevents_a_false_success_report() {
        let (runtime, owner) = ServiceRuntime::new().unwrap();
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (finished, receive) = mpsc::channel();
        runtime.spawn(async move {
            tokio::task::spawn_blocking(move || {
                started.send(()).unwrap();
                wait.recv().unwrap();
                finished.send(()).unwrap();
            })
            .await
            .unwrap();
        });
        ready.recv_timeout(DEADLINE).unwrap();
        let report = owner.shutdown(Duration::ZERO);
        assert!(!report.finished);
        // Nested Tokio jobs bypass the app counter, so this count alone must
        // never authorize releasing the instance lease.
        assert_eq!(report.unfinished_blocking, 0);
        release.send(()).unwrap();
        receive.recv_timeout(DEADLINE).unwrap();
    }

    #[test]
    fn panicking_blocking_job_does_not_leave_an_outstanding_writer() {
        let (runtime, owner) = ServiceRuntime::new().unwrap();
        let task = runtime.spawn_blocking(|| panic!("controlled fixture panic"));
        assert!(runtime.handle.block_on(task).unwrap_err().is_panic());
        let report = owner.shutdown(DEADLINE);
        assert!(report.finished && report.unfinished_blocking == 0);
    }

    #[test]
    fn panic_exit_subprocess_fixture() {
        use std::io::{BufRead, Write};
        let Some(directory) = std::env::var_os("PICKLE_PANIC_LEASE_FIXTURE") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        let caught = std::panic::catch_unwind(move || {
            let _instance = match crate::instance::InstanceGuard::acquire(&directory).unwrap() {
                crate::instance::InstanceStatus::Primary(guard) => guard,
                crate::instance::InstanceStatus::Secondary => panic!("Fixture requires primary"),
            };
            let (_runtime, _owner) = ServiceRuntime::new().unwrap();
            // This independent writer is deliberately declared after the owner,
            // matching main's order and the original unwind lifetime hazard.
            let worker = crate::geometry_store::Worker::start(directory.clone()).unwrap();
            assert!(worker.loaded.unwrap().is_none());
            println!("FIXTURE_READY");
            std::io::stdout().flush().unwrap();
            let mut command = String::new();
            std::io::stdin().lock().read_line(&mut command).unwrap();
            assert_eq!(command.trim(), "PANIC");
            worker
                .worker
                .enqueue(crate::geometry_store::WindowFrame {
                    position: (100, 100),
                    logical_size: (560.0, 640.0),
                    scale: 1.0,
                })
                .unwrap();
            panic!("controlled process-owned lifecycle fixture");
        });
        assert!(caught.is_err());
        // Reached only by the old unsafe unwind behavior. Keep the process and
        // detached writer live so the parent can detect premature lease release.
        println!("UNSAFE_UNWIND_RETURNED");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }

    #[test]
    fn panic_exit_keeps_the_instance_lease_until_independent_writers_terminate() {
        use fs2::FileExt;
        use std::{
            io::{BufRead, BufReader, Write},
            process::{Command, Stdio},
            time::Instant,
        };
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut child = Child(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "service_runtime::tests::panic_exit_subprocess_fixture",
                    "--nocapture",
                ])
                .env("PICKLE_PANIC_LEASE_FIXTURE", directory.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut stdout = BufReader::new(child.0.stdout.take().unwrap());
        let (ready, received) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut line = String::new();
            while stdout.read_line(&mut line).unwrap() > 0 {
                if line.trim() == "FIXTURE_READY" {
                    let _ = ready.send(());
                    break;
                }
                line.clear();
            }
            stdout
        });
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        let _stdout = reader.join().unwrap();
        assert!(crate::instance::is_running(directory.path()).unwrap());
        // Hold the real geometry transaction lock until the subprocess ends;
        // its admitted writer cannot finish or mutate the archive meanwhile.
        let geometry_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.path().join("window.lock"))
            .unwrap();
        FileExt::lock_exclusive(&geometry_lock).unwrap();
        child
            .0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"PANIC\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = 'wait: loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            if !crate::instance::is_running(directory.path()).unwrap() {
                // OS exit closes handles just before the child becomes
                // reapable. Allow that transition, but never a returned unwind
                // that leaves its detached writer and process running.
                let exiting = Instant::now() + Duration::from_millis(100);
                loop {
                    if let Some(status) = child.0.try_wait().unwrap() {
                        break 'wait status;
                    }
                    assert!(
                        Instant::now() < exiting,
                        "The instance lease was released while the process and blocked geometry writer remained live"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            assert!(
                Instant::now() < deadline,
                "Panic cleanup did not terminate promptly"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.code(), Some(1));
        // Explicit InstanceGuard Drop removes this file. Its retention proves
        // the panic path did not unlock through Rust unwinding before exit.
        assert!(directory.path().join("instance.json").is_file());
        assert!(!crate::instance::is_running(directory.path()).unwrap());
        FileExt::unlock(&geometry_lock).unwrap();
        assert!(!directory.path().join("window.json").exists());
        let mut stderr = String::new();
        std::io::Read::read_to_string(&mut child.0.stderr.take().unwrap(), &mut stderr).unwrap();
        assert!(stderr.contains("Pickle stopped after an unexpected application failure."));
    }
}
