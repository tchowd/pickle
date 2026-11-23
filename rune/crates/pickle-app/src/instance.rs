//! Per-user single-instance lease and authenticated, content-free reopen forwarding.
//! The lock file is never deleted: removing it could let two processes lock different inodes.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const LOCK_FILE: &str = "instance.lock";
const CONTROL_FILE: &str = "instance.json";
const MAX_BYTES: usize = 1024;
const IO_TIMEOUT: Duration = Duration::from_millis(400);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceEvent {
    Reopen,
}
pub enum InstanceStatus {
    Primary(InstanceGuard),
    Secondary,
}
pub struct InstanceGuard {
    lock: File,
    control_file: PathBuf,
    token: String,
    address: SocketAddr,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    receiver: Option<Receiver<InstanceEvent>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    version: u8,
    port: u16,
    token: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlRequest {
    version: u8,
    token: String,
    action: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlResponse {
    version: u8,
    accepted: bool,
}

impl InstanceGuard {
    /// Acquire before credentials, capture services, history migration, or native UI start.
    /// A secondary forwards Reopen to the lock holder and performs no app initialization.
    pub fn acquire(directory: &Path) -> Result<InstanceStatus, String> {
        if directory.file_name().is_none() {
            return Err("Pickle needs a dedicated user data directory.".into());
        }
        fs::create_dir_all(directory)
            .map_err(|_| "Could not create Pickle's instance directory.")?;
        reject_symlink(directory)?;
        protect(directory, true)?;
        let lock_path = directory.join(LOCK_FILE);
        reject_symlink(&lock_path)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|_| "Could not open Pickle's instance lock.")?;
        protect(&lock_path, false)?;
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) => {}
            Err(error) if lock_contended(&error) => {
                // The first process can own the lease before its control file is published.
                let deadline = Instant::now() + STARTUP_TIMEOUT;
                loop {
                    if notify_existing(directory).is_ok() {
                        return Ok(InstanceStatus::Secondary);
                    }
                    if Instant::now() >= deadline {
                        return Err("Pickle is already running but could not be reopened. Wait a moment or quit the existing instance before trying again.".into());
                    }
                    thread::sleep(Duration::from_millis(25));
                }
            }
            Err(_) => return Err("Could not acquire Pickle's instance lock.".into()),
        }
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| "Could not start Pickle's private reopen service.")?;
        let address = listener
            .local_addr()
            .map_err(|_| "Could not inspect Pickle's reopen service.")?;
        // Two independent UUIDs provide ample random entropy; the token never enters logs.
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let endpoint = Endpoint {
            version: 1,
            port: address.port(),
            token: token.clone(),
        };
        let control_file = directory.join(CONTROL_FILE);
        publish_endpoint(directory, &control_file, &endpoint)?;
        let stopped = Arc::new(AtomicBool::new(false));
        let signal = stopped.clone();
        // Repeated launches coalesce when the bounded queue already contains reopen events.
        let (sender, receiver) = mpsc::sync_channel(8);
        let secret = token.clone();
        let worker = thread::Builder::new()
            .name("pickle-reopen".into())
            .spawn(move || {
                while !signal.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, peer)) => {
                            if signal.load(Ordering::Acquire) {
                                break;
                            }
                            if peer.ip().is_loopback() {
                                handle_request(stream, &secret, &sender);
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .map_err(|_| {
                let _ = fs::remove_file(&control_file);
                "Could not start Pickle's reopen listener.".to_owned()
            })?;
        Ok(InstanceStatus::Primary(Self {
            lock,
            control_file,
            token,
            address,
            stopped,
            worker: Some(worker),
            receiver: Some(receiver),
        }))
    }

    pub fn take_receiver(&mut self) -> Option<Receiver<InstanceEvent>> {
        self.receiver.take()
    }
}

/// Read-only lease probe for installers. Never creates files, contacts an endpoint, or reopens UI.
/// A contended lock counts as running even while the primary's control endpoint is starting.
pub fn is_running(directory: &Path) -> Result<bool, String> {
    reject_symlink(directory)?;
    let lock_path = directory.join(LOCK_FILE);
    reject_symlink(&lock_path)?;
    match fs::symlink_metadata(&lock_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("Could not inspect Pickle's instance lock.".into()),
        Ok(metadata) if !metadata.is_file() => {
            return Err("Pickle's instance lock is not a regular file.".into());
        }
        Ok(_) => {}
    }
    let lock = File::open(&lock_path)
        .map_err(|_| "Could not open Pickle's instance lock for inspection.".to_string())?;
    match FileExt::try_lock_exclusive(&lock) {
        Ok(()) => {
            FileExt::unlock(&lock)
                .map_err(|_| "Could not finish inspecting Pickle's instance lock.".to_string())?;
            Ok(false)
        }
        Err(error) if lock_contended(&error) => Ok(true),
        Err(_) => Err("Could not inspect Pickle's running status safely.".into()),
    }
}

/// Forward only a Reopen command to a live lease holder. No selected content crosses this API.
pub fn notify_existing(directory: &Path) -> Result<(), String> {
    let lock_path = directory.join(LOCK_FILE);
    reject_symlink(&lock_path)?;
    // Do not send an endpoint token to a stale/reused port when no primary holds the lease.
    let probe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|_| "No running Pickle instance was found.")?;
    match FileExt::try_lock_exclusive(&probe) {
        Ok(()) => {
            let _ = FileExt::unlock(&probe);
            return Err("No running Pickle instance was found.".into());
        }
        Err(error) if lock_contended(&error) => {}
        Err(_) => return Err("Could not inspect Pickle's instance lock.".into()),
    }
    let endpoint = read_endpoint(&directory.join(CONTROL_FILE))?;
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, endpoint.port));
    let mut stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)
        .map_err(|_| "The existing Pickle instance could not be reached.")?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|_| "Could not bound the reopen response.")?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|_| "Could not bound the reopen request.")?;
    let bytes = serde_json::to_vec(&ControlRequest {
        version: 1,
        token: endpoint.token,
        action: "reopen".into(),
    })
    .map_err(|_| "Could not encode the reopen request.")?;
    stream
        .write_all(&bytes)
        .map_err(|_| "Could not send the reopen request.")?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|_| "Could not finish the reopen request.")?;
    let response = read_control(&mut stream)
        .map_err(|error| format!("The reopen request could not complete ({error})."))?;
    if response.len() > MAX_BYTES {
        return Err("The reopen response was too large.".into());
    }
    let reply: ControlResponse = serde_json::from_slice(&response)
        .map_err(|_| "The existing instance returned an invalid response.")?;
    if reply.version != 1 || !reply.accepted {
        return Err("The existing instance rejected the reopen request.".into());
    }
    Ok(())
}

fn handle_request(mut stream: TcpStream, secret: &str, sender: &SyncSender<InstanceEvent>) {
    if stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return;
    }
    let valid = read_control(&mut stream).is_ok_and(|bytes| {
        bytes.len() <= MAX_BYTES
            && serde_json::from_slice::<ControlRequest>(&bytes).is_ok_and(|request| {
                request.version == 1
                    && request.action == "reopen"
                    && token_equal(&request.token, secret)
            })
    });
    let accepted = valid
        && match sender.try_send(InstanceEvent::Reopen) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        };
    let response = serde_json::to_vec(&ControlResponse {
        version: 1,
        accepted,
    })
    .unwrap_or_default();
    let _ = stream.write_all(&response);
    let _ = stream.shutdown(Shutdown::Both);
}

/// Absolute deadline rather than an idle timeout, so a trickle cannot hold shutdown open.
fn read_control(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    // Poll with an absolute deadline. Re-setting SO_RCVTIMEO after a peer's close can
    // return EINVAL on macOS even while acknowledgement bytes remain buffered.
    stream.set_nonblocking(true)?;
    let result = read_control_nonblocking(stream);
    let _ = stream.set_nonblocking(false);
    result
}
fn read_control_nonblocking(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 256];
    loop {
        if Instant::now() >= deadline {
            return Err(std::io::Error::from(std::io::ErrorKind::TimedOut));
        }
        let room = (MAX_BYTES + 1 - bytes.len()).min(buffer.len());
        let read = match stream.read(&mut buffer[..room]) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(error) => return Err(error),
        };
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_BYTES {
            return Ok(bytes);
        }
    }
}
fn token_equal(candidate: &str, secret: &str) -> bool {
    if candidate.len() != secret.len() {
        return false;
    }
    candidate
        .bytes()
        .zip(secret.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
fn lock_contended(error: &std::io::Error) -> bool {
    error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
        || error.kind() == std::io::ErrorKind::WouldBlock
}
fn read_endpoint(path: &Path) -> Result<Endpoint, String> {
    reject_symlink(path)?;
    let file = File::open(path).map_err(|_| "The existing instance is still starting.")?;
    if file
        .metadata()
        .map_err(|_| "Could not inspect the instance endpoint.")?
        .len()
        > MAX_BYTES as u64
    {
        return Err("The instance endpoint is invalid.".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the instance endpoint.")?;
    if bytes.len() > MAX_BYTES {
        return Err("The instance endpoint is invalid.".into());
    }
    let endpoint: Endpoint = serde_json::from_slice(&bytes)
        .map_err(|_| "The instance endpoint is invalid.".to_owned())?;
    if endpoint.version != 1
        || endpoint.port == 0
        || endpoint.token.len() != 64
        || !endpoint.token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("The instance endpoint is invalid.".into());
    }
    Ok(endpoint)
}
fn publish_endpoint(directory: &Path, path: &Path, endpoint: &Endpoint) -> Result<(), String> {
    let temporary = directory.join(format!(".instance-{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Could not stage the instance endpoint.")?;
        protect(&temporary, false)?;
        let bytes =
            serde_json::to_vec(endpoint).map_err(|_| "Could not encode the instance endpoint.")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Could not save the instance endpoint.")?;
        drop(file);
        // The exclusive instance lease is held; any previous metadata is stale.
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Could not replace the instance endpoint.".into()),
        }
        fs::rename(&temporary, path).map_err(|_| "Could not publish the instance endpoint.")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn reject_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("Pickle's instance files must not be symbolic links.".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Could not inspect Pickle's instance files.".into()),
    }
}
fn protect(path: &Path, directory: bool) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
        )
        .map_err(|_| "Could not protect Pickle's instance files.".into())
    }
    #[cfg(windows)]
    {
        let _ = directory;
        protect_windows(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, directory);
        Err("Single-instance protection is unavailable on this platform.".into())
    }
}

#[cfg(windows)]
fn protect_windows(path: &Path) -> Result<(), String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *const c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    // Protected DACL: only SYSTEM and the object's owner receive full control.
    // OI/CI ensure newly created endpoint files inherit the same boundary.
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;OW)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: terminated UTF-16 strings and valid output storage; the Windows allocation
    // is released exactly once with LocalFree after SetFileSecurityW returns.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err("Could not create private instance permissions.".into());
        }
        let result = SetFileSecurityW(filename.as_ptr(), 0x8000_0004, descriptor);
        LocalFree(descriptor);
        if result == 0 {
            Err("Could not protect Pickle's instance files.".into())
        } else {
            Ok(())
        }
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        // Wake the blocking accept; a connected slow client is bounded by IO_TIMEOUT.
        let _ = TcpStream::connect_timeout(&self.address, IO_TIMEOUT);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if read_endpoint(&self.control_file)
            .is_ok_and(|endpoint| token_equal(&endpoint.token, &self.token))
        {
            let _ = fs::remove_file(&self.control_file);
        }
        let _ = FileExt::unlock(&self.lock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn primary(directory: &Path) -> InstanceGuard {
        match InstanceGuard::acquire(directory).unwrap() {
            InstanceStatus::Primary(guard) => guard,
            InstanceStatus::Secondary => panic!("Expected a primary instance"),
        }
    }
    fn request(directory: &Path, token: String, action: &str, extra: bool) -> ControlResponse {
        let endpoint = read_endpoint(&directory.join(CONTROL_FILE)).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut value = serde_json::json!({"version":1,"token":token,"action":action});
        if extra {
            value["selection"] = serde_json::json!("must never be accepted");
        }
        stream.write_all(value.to_string().as_bytes()).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut bytes = vec![];
        stream.read_to_end(&mut bytes).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
    #[test]
    fn second_acquisition_forwards_reopen_without_starting_another_primary() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = primary(dir.path());
        let events = guard.take_receiver().unwrap();
        assert!(matches!(
            InstanceGuard::acquire(dir.path()).unwrap(),
            InstanceStatus::Secondary
        ));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            InstanceEvent::Reopen
        );
        assert!(guard.take_receiver().is_none());
    }

    #[test]
    fn running_probe_never_creates_state_or_queues_reopen() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("not-created");
        assert!(!is_running(&absent).unwrap());
        assert!(!absent.exists());
        // Windows byte-range locks are mandatory, so another handle cannot
        // read the lease bytes while the primary owns it. Seed the invariant
        // before acquisition and inspect it after release on every platform.
        let lock_before = b"pre-existing lease contents";
        fs::write(root.path().join(LOCK_FILE), lock_before).unwrap();
        let mut guard = primary(root.path());
        let events = guard.take_receiver().unwrap();
        assert!(is_running(root.path()).unwrap());
        assert!(events.try_recv().is_err());
        drop(guard);
        assert!(!is_running(root.path()).unwrap());
        assert_eq!(fs::read(root.path().join(LOCK_FILE)).unwrap(), lock_before);
    }
    #[test]
    fn rejects_wrong_tokens_unknown_actions_and_content_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = primary(dir.path());
        let events = guard.take_receiver().unwrap();
        let endpoint = read_endpoint(&dir.path().join(CONTROL_FILE)).unwrap();
        assert!(!request(dir.path(), "0".repeat(64), "reopen", false).accepted);
        assert!(!request(dir.path(), endpoint.token.clone(), "generate", false).accepted);
        assert!(!request(dir.path(), endpoint.token, "reopen", true).accepted);
        assert!(events.try_recv().is_err());
    }
    #[test]
    fn dropping_guard_closes_listener_removes_metadata_and_recovers_lock() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = primary(dir.path());
        let events = guard.take_receiver().unwrap();
        let started = Instant::now();
        drop(guard);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!dir.path().join(CONTROL_FILE).exists());
        assert!(dir.path().join(LOCK_FILE).exists());
        // Another parallel fixture may immediately reuse the ephemeral port. The old
        // listener's worker is joined and its event producer must be disconnected.
        assert!(matches!(
            events.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        let _replacement = primary(dir.path());
    }
    #[test]
    fn stale_metadata_without_a_lock_holder_is_not_contacted() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOCK_FILE), b"").unwrap();
        fs::write(dir.path().join(CONTROL_FILE),br#"{"version":1,"port":1,"token":"0000000000000000000000000000000000000000000000000000000000000000"}"#).unwrap();
        assert!(notify_existing(dir.path()).is_err());
        let _guard = primary(dir.path());
    }
    #[test]
    fn malformed_or_oversized_requests_never_queue_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = primary(dir.path());
        let events = guard.take_receiver().unwrap();
        let endpoint = read_endpoint(&dir.path().join(CONTROL_FILE)).unwrap();
        for bytes in [b"not json".to_vec(), vec![b'x'; MAX_BYTES + 1]] {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream.write_all(&bytes).unwrap();
            stream.shutdown(Shutdown::Write).unwrap();
            let mut response = vec![];
            let _ = stream.read_to_end(&mut response);
        }
        assert!(events.try_recv().is_err());
    }
    #[test]
    fn reopen_queue_is_bounded_and_slow_clients_do_not_block_shutdown_forever() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = primary(dir.path());
        let events = guard.take_receiver().unwrap();
        for _ in 0..20 {
            notify_existing(dir.path()).unwrap();
        }
        assert_eq!(events.try_iter().count(), 8);
        let endpoint = read_endpoint(&dir.path().join(CONTROL_FILE)).unwrap();
        let _slow = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
        let start = Instant::now();
        drop(guard);
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn trickled_input_obeys_an_absolute_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = primary(dir.path());
        let events = guard.take_receiver().unwrap();
        let endpoint = read_endpoint(&dir.path().join(CONTROL_FILE)).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
        let writer = thread::spawn(move || {
            for _ in 0..30 {
                if stream.write_all(b" ").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(30));
            }
        });
        thread::sleep(Duration::from_millis(20));
        let started = Instant::now();
        drop(guard);
        assert!(started.elapsed() < Duration::from_secs(2));
        writer.join().unwrap();
        assert!(events.try_recv().is_err());
    }
    #[test]
    fn child_lease_holder() {
        let Some(directory) = std::env::var_os("PICKLE_INSTANCE_TEST_DIRECTORY") else {
            return;
        };
        let directory = PathBuf::from(directory);
        let _guard = primary(&directory);
        fs::write(directory.join("ready"), b"ready").unwrap();
        let mut input = [0u8; 1];
        let _ = std::io::stdin().read(&mut input);
    }
    #[test]
    fn process_exit_recovers_the_lock_without_trusting_stale_metadata() {
        use std::process::{Command, Stdio};
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "instance::tests::child_lease_holder",
                "--nocapture",
            ])
            .env("PICKLE_INSTANCE_TEST_DIRECTORY", dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.path().join("ready").exists() {
            if child.try_wait().unwrap().is_some() {
                panic!("Child instance failed to acquire its lease");
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Child instance did not become ready");
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(
            InstanceGuard::acquire(dir.path()).unwrap(),
            InstanceStatus::Secondary
        ));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(dir.path().join(CONTROL_FILE).exists());
        assert!(notify_existing(dir.path()).is_err());
        let _replacement = primary(dir.path());
    }
    #[cfg(unix)]
    #[test]
    fn instance_files_are_owner_only_and_symlinks_are_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let guard = primary(dir.path());
        assert_eq!(
            fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(dir.path().join(CONTROL_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(dir.path().join(LOCK_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(guard);
        let another = tempfile::tempdir().unwrap();
        let target = another.path().join("target");
        fs::write(&target, b"keep").unwrap();
        symlink(&target, another.path().join(LOCK_FILE)).unwrap();
        assert!(InstanceGuard::acquire(another.path()).is_err());
        assert_eq!(fs::read(target).unwrap(), b"keep");
    }
}
