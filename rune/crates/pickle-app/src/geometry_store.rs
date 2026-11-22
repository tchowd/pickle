//! Independent, versioned window preferences. Call only from an I/O worker.
//! The persistent lock serializes validation and replacement; never delete its inode.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
};

const ARCHIVE: &str = "window.json";
const LOCK: &str = "window.lock";
const MAX_BYTES: u64 = 4096;

enum WorkerCommand {
    Save,
    Finish,
}

/// One serial I/O worker, with one coalescing pending frame instead of an unbounded
/// queue. Finish it after the native event loop returns, never inside a UI callback.
pub struct Worker {
    sender: mpsc::SyncSender<WorkerCommand>,
    pending: Arc<Mutex<Option<WindowFrame>>>,
    error: Arc<Mutex<Option<String>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

pub struct StartedWorker {
    pub worker: Arc<Worker>,
    pub loaded: Result<Option<WindowFrame>, String>,
}

impl Worker {
    /// Start before the event loop. Loading and subsequent writes happen on the
    /// worker; waiting for its initial result here cannot stall an active window.
    pub fn start(directory: PathBuf) -> Result<StartedWorker, String> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (ready, loaded) = mpsc::sync_channel(1);
        let pending = Arc::new(Mutex::new(None));
        let error = Arc::new(Mutex::new(None));
        let worker_pending = pending.clone();
        let worker_error = error.clone();
        let thread = std::thread::Builder::new()
            .name("pickle-window-preferences".into())
            .spawn(move || {
                let _ = ready.send(load(&directory));
                while let Ok(command) = receiver.recv() {
                    let frame = match worker_pending.lock() {
                        Ok(mut pending) => pending.take(),
                        Err(_) => break,
                    };
                    if let Some(frame) = frame
                        && let Err(problem) = save(&directory, &frame)
                        && let Ok(mut error) = worker_error.lock()
                    {
                        *error = Some(problem);
                    }
                    if matches!(command, WorkerCommand::Finish) {
                        break;
                    }
                }
            })
            .map_err(|_| "Could not start the window preferences worker.".to_string())?;
        let loaded = loaded
            .recv()
            .unwrap_or_else(|_| Err("Could not load window preferences.".into()));
        Ok(StartedWorker {
            worker: Arc::new(Self {
                sender,
                pending,
                error,
                thread: Mutex::new(Some(thread)),
            }),
            loaded,
        })
    }

    /// Nonblocking notification: repeated UI captures replace the single pending
    /// value, and a queued notification guarantees the latest frame is consumed.
    pub fn enqueue(&self, frame: WindowFrame) -> Result<(), String> {
        frame.validate()?;
        *self
            .pending
            .lock()
            .map_err(|_| "Window preferences are unavailable.".to_string())? = Some(frame);
        match self.sender.try_send(WorkerCommand::Save) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                Err("The window preferences worker has stopped.".into())
            }
        }
    }

    pub fn take_error(&self) -> Option<String> {
        self.error.lock().ok()?.take()
    }

    /// Called by main after app.run, so ordinary quit flushes the latest queued
    /// frame without putting filesystem work in an on_close/on_quit callback.
    pub fn finish(&self) -> Result<(), String> {
        let mut thread = self
            .thread
            .lock()
            .map_err(|_| "Window preferences are unavailable.".to_string())?;
        if let Some(thread) = thread.take() {
            let _ = self.sender.send(WorkerCommand::Finish);
            thread
                .join()
                .map_err(|_| "The window preferences worker could not finish.".to_string())?;
        }
        match self.take_error() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// Clamp a preferred logical size and physical position to the selected current
/// monitor. The caller supplies that monitor's current scale, never the old scale.
/// A monitor smaller than the app's minimum cannot restore safely; retain defaults.
pub fn clamp_to_work_area(
    frame: WindowFrame,
    current_scale: f64,
    area: (i32, i32, u32, u32),
) -> Option<WindowFrame> {
    frame.validate().ok()?;
    if !current_scale.is_finite() || !(0.25..=8.0).contains(&current_scale) {
        return None;
    }
    let (left, top, width, height) = area;
    let fit_axis = |preferred: f32, available: u32| {
        let logical = (f64::from(available) / current_scale).min(f64::from(preferred)) as f32;
        // f64 -> f32 can round upward by one ULP; keep fractional-DPI restores
        // inside the physical work area instead of overflowing by one pixel.
        if f64::from(logical) * current_scale > f64::from(available) {
            logical.next_down()
        } else {
            logical
        }
    };
    let logical_width = fit_axis(frame.logical_size.0, width);
    let logical_height = fit_axis(frame.logical_size.1, height);
    if logical_width < 480.0 || logical_height < 180.0 {
        return None;
    }
    let physical_width = (f64::from(logical_width) * current_scale).ceil() as i64;
    let physical_height = (f64::from(logical_height) * current_scale).ceil() as i64;
    let max_x = i64::from(left) + i64::from(width) - physical_width;
    let max_y = i64::from(top) + i64::from(height) - physical_height;
    let x = i64::from(frame.position.0).clamp(i64::from(left), max_x.max(i64::from(left)));
    let y = i64::from(frame.position.1).clamp(i64::from(top), max_y.max(i64::from(top)));
    let restored = WindowFrame {
        position: (i32::try_from(x).ok()?, i32::try_from(y).ok()?),
        logical_size: (logical_width, logical_height),
        scale: current_scale,
    };
    restored.validate().ok()?;
    Some(restored)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowFrame {
    /// Physical desktop coordinates; negative monitor origins are valid.
    pub position: (i32, i32),
    /// Logical pixels, independent of the monitor scale.
    pub logical_size: (f32, f32),
    pub scale: f64,
}

impl WindowFrame {
    pub fn validate(&self) -> Result<(), String> {
        if [self.position.0, self.position.1]
            .into_iter()
            .any(|value| i64::from(value).abs() > 1_000_000)
            || !self.logical_size.0.is_finite()
            || !self.logical_size.1.is_finite()
            || !(480.0..=16_384.0).contains(&self.logical_size.0)
            || !(180.0..=16_384.0).contains(&self.logical_size.1)
            || !self.scale.is_finite()
            || !(0.25..=8.0).contains(&self.scale)
        {
            return Err("The saved window geometry is outside supported bounds. Existing preferences have not been changed.".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    version: u32,
    frame: WindowFrame,
}

/// Returns no preference for a first launch. The caller must clamp a restored frame
/// to a current work area; recorded monitors and scale may no longer exist.
pub fn load(directory: &Path) -> Result<Option<WindowFrame>, String> {
    dedicated_directory(directory)?;
    reject_link(directory)?;
    match fs::metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Could not inspect the window preferences directory.".into()),
        Ok(metadata) if !metadata.is_dir() => {
            return Err("The window preferences location is not a directory.".into());
        }
        Ok(_) => {}
    }
    protect(directory, true)?;
    let _lease = lock(directory)?;
    read(directory)
}

/// Rejects damaged or future archives instead of overwriting them with defaults.
/// This store is separate from settings, so geometry cannot race connection saves.
pub fn save(directory: &Path, frame: &WindowFrame) -> Result<(), String> {
    frame.validate()?;
    dedicated_directory(directory)?;
    reject_link(directory)?;
    fs::create_dir_all(directory)
        .map_err(|_| "Could not create the window preferences directory.".to_string())?;
    protect(directory, true)?;
    let _lease = lock(directory)?;
    read(directory)?;
    let encoded = serde_json::to_vec_pretty(&Archive {
        version: 1,
        frame: *frame,
    })
    .map_err(|_| "Could not encode window preferences.".to_string())?;
    let mut staged = tempfile::NamedTempFile::new_in(directory)
        .map_err(|_| "Could not stage window preferences.".to_string())?;
    protect(staged.path(), false)?;
    staged
        .write_all(&encoded)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|_| {
            "Could not save window preferences. Existing preferences have not been changed."
                .to_string()
        })?;
    // Inspect again before replacing; malformed archives are preserved under the same lease.
    reject_link(&directory.join(ARCHIVE))?;
    staged.persist(directory.join(ARCHIVE)).map_err(|_| {
        "Could not replace window preferences. Existing preferences have not been changed."
            .to_string()
    })?;
    #[cfg(unix)]
    File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| {
            "Window preferences were replaced, but directory synchronization failed.".to_string()
        })?;
    Ok(())
}

fn lock(directory: &Path) -> Result<File, String> {
    let path = directory.join(LOCK);
    reject_non_file(&path)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lease = options
        .open(&path)
        .map_err(|_| "Could not open the window preferences lock.".to_string())?;
    protect(&path, false)?;
    FileExt::lock_exclusive(&lease)
        .map_err(|_| "Could not lock window preferences.".to_string())?;
    Ok(lease)
}

fn read(directory: &Path) -> Result<Option<WindowFrame>, String> {
    let path = directory.join(ARCHIVE);
    reject_non_file(&path)?;
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Could not open window preferences.".into()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "Could not inspect window preferences.".to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("The window preferences archive is too large or is not a file. It has not been changed.".into());
    }
    protect(&path, false)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read window preferences.".to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The window preferences archive is too large. It has not been changed.".into());
    }
    let archive: Archive = serde_json::from_slice(&bytes).map_err(|_| {
        "Window preferences are damaged or unsupported. They have not been changed. Move window.json aside to reset the window."
            .to_string()
    })?;
    if archive.version != 1 {
        return Err(
            "Window preferences were created by another version. They have not been changed."
                .into(),
        );
    }
    archive.frame.validate()?;
    Ok(Some(archive.frame))
}

fn dedicated_directory(directory: &Path) -> Result<(), String> {
    if directory.file_name().is_none() {
        Err("Window preferences require a dedicated user data directory.".into())
    } else {
        Ok(())
    }
}

fn reject_non_file(path: &Path) -> Result<(), String> {
    reject_link(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            Err("Window preferences require regular archive and lock files.".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Could not inspect the window preferences file.".into()),
    }
}

fn reject_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let link = metadata.file_type().is_symlink();
            #[cfg(windows)]
            let link = {
                use std::os::windows::fs::MetadataExt;
                link || metadata.file_attributes() & 0x400 != 0
            };
            if link {
                Err("Window preferences cannot use linked files or directories.".into())
            } else {
                Ok(())
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Could not inspect the window preferences path.".into()),
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
        .map_err(|_| "Could not protect window preferences.".into())
    }
    #[cfg(windows)]
    {
        let _ = directory;
        protect_windows(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, directory);
        Err("Private window preferences are unavailable on this platform.".into())
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
    // Protected owner/SYSTEM-only DACL; children inherit the same privacy boundary.
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;OW)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let filename: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: terminated strings, valid outputs, and one matching LocalFree for
    // the Windows-allocated descriptor after SetFileSecurityW completes.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err("Could not create private window preference permissions.".into());
        }
        let result = SetFileSecurityW(filename.as_ptr(), 0x8000_0004, descriptor);
        LocalFree(descriptor);
        if result == 0 {
            Err("Could not protect window preferences.".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, mpsc};

    fn frame() -> WindowFrame {
        WindowFrame {
            position: (-1920, 80),
            logical_size: (560.0, 720.0),
            scale: 2.0,
        }
    }

    #[test]
    fn first_launch_and_roundtrip_are_independent_of_settings() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("preferences");
        assert_eq!(load(&directory).unwrap(), None);
        assert!(!directory.exists());
        save(&directory, &frame()).unwrap();
        assert_eq!(load(&directory).unwrap(), Some(frame()));
        assert!(!directory.join("settings.json").exists());
        assert!(directory.join(LOCK).exists());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
    }

    #[test]
    fn invalid_values_never_create_or_replace_a_preference() {
        let root = tempfile::tempdir().unwrap();
        save(root.path(), &frame()).unwrap();
        let before = fs::read(root.path().join(ARCHIVE)).unwrap();
        let bad = [
            WindowFrame {
                position: (i32::MIN, 0),
                ..frame()
            },
            WindowFrame {
                position: (i32::MAX, 0),
                ..frame()
            },
            WindowFrame {
                logical_size: (f32::NAN, 720.0),
                ..frame()
            },
            WindowFrame {
                logical_size: (560.0, f32::INFINITY),
                ..frame()
            },
            WindowFrame {
                logical_size: (479.0, 180.0),
                ..frame()
            },
            WindowFrame {
                logical_size: (560.0, 17_000.0),
                ..frame()
            },
            WindowFrame {
                scale: f64::NAN,
                ..frame()
            },
            WindowFrame {
                scale: f64::INFINITY,
                ..frame()
            },
            WindowFrame {
                scale: 0.0,
                ..frame()
            },
            WindowFrame {
                scale: 8.01,
                ..frame()
            },
        ];
        for value in bad {
            assert!(save(root.path(), &value).is_err());
            assert_eq!(fs::read(root.path().join(ARCHIVE)).unwrap(), before);
        }
    }

    #[test]
    fn corruption_unknown_fields_overflow_and_versions_block_replacement() {
        let root = tempfile::tempdir().unwrap();
        let cases = [
            "damaged".to_owned(),
            " ".to_owned(),
            r#"{"version":2,"frame":{"position":[0,0],"logical_size":[560,720],"scale":1}}"#.to_owned(),
            r#"{"version":1,"frame":{"position":[2147483648,0],"logical_size":[560,720],"scale":1}}"#.to_owned(),
            r#"{"version":1,"frame":{"position":[0,0],"logical_size":[1e100,720],"scale":1}}"#.to_owned(),
            r#"{"version":1,"frame":{"position":[0,0],"logical_size":[560,720],"scale":1,"future":true}}"#.to_owned(),
            r#"{"version":1,"frame":{"position":[0,0],"logical_size":[560,720],"scale":1},"future":true}"#.to_owned(),
            "x".repeat(MAX_BYTES as usize + 1),
        ];
        for input in cases {
            fs::write(root.path().join(ARCHIVE), input.as_bytes()).unwrap();
            assert!(load(root.path()).is_err());
            assert!(save(root.path(), &frame()).is_err());
            assert_eq!(
                fs::read(root.path().join(ARCHIVE)).unwrap(),
                input.as_bytes()
            );
        }
    }

    #[test]
    fn valid_boundaries_and_negative_monitor_origins_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        for value in [
            WindowFrame {
                position: (-1_000_000, 1_000_000),
                logical_size: (480.0, 180.0),
                scale: 0.25,
            },
            WindowFrame {
                logical_size: (16_384.0, 16_384.0),
                scale: 8.0,
                ..frame()
            },
        ] {
            save(root.path(), &value).unwrap();
            assert_eq!(load(root.path()).unwrap(), Some(value));
        }
    }

    #[test]
    fn directories_and_root_paths_are_never_used_as_archives() {
        for name in [ARCHIVE, LOCK] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join(name)).unwrap();
            assert!(load(root.path()).is_err());
            assert!(save(root.path(), &frame()).is_err());
            assert!(root.path().join(name).is_dir());
            fs::remove_dir(root.path().join(name)).unwrap();
        }
        assert!(load(Path::new("")).is_err());
        assert!(save(Path::new(""), &frame()).is_err());
        #[cfg(unix)]
        assert!(load(Path::new("/")).is_err());
    }

    #[test]
    fn competing_writers_produce_one_complete_valid_archive() {
        let root = tempfile::tempdir().unwrap();
        let barrier = Arc::new(Barrier::new(9));
        let writers: Vec<_> = (0..8)
            .map(|index| {
                let directory = root.path().to_owned();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let value = WindowFrame {
                        position: (index, -index),
                        ..frame()
                    };
                    for _ in 0..5 {
                        save(&directory, &value).unwrap();
                        load(&directory).unwrap().unwrap().validate().unwrap();
                    }
                })
            })
            .collect();
        barrier.wait();
        for writer in writers {
            writer.join().unwrap();
        }
        let value = load(root.path()).unwrap().unwrap();
        assert!((0..8).contains(&value.position.0));
        assert_eq!(value.position.1, -value.position.0);
    }

    #[test]
    fn background_worker_coalesces_and_flushes_the_latest_frame() {
        let root = tempfile::tempdir().unwrap();
        let started = Worker::start(root.path().to_owned()).unwrap();
        assert_eq!(started.loaded.unwrap(), None);
        let lease = lock(root.path()).unwrap();
        for index in 0..500 {
            started
                .worker
                .enqueue(WindowFrame {
                    position: (index, -index),
                    ..frame()
                })
                .unwrap();
        }
        drop(lease);
        started.worker.finish().unwrap();
        assert_eq!(load(root.path()).unwrap().unwrap().position, (499, -499));
        started.worker.finish().unwrap();
        assert!(started.worker.enqueue(frame()).is_err());
    }

    #[test]
    fn background_worker_reports_corruption_and_never_replaces_it() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(ARCHIVE), b"damaged and retained").unwrap();
        let started = Worker::start(root.path().to_owned()).unwrap();
        assert!(started.loaded.is_err());
        started.worker.enqueue(frame()).unwrap();
        assert!(started.worker.finish().is_err());
        assert_eq!(
            fs::read(root.path().join(ARCHIVE)).unwrap(),
            b"damaged and retained"
        );
    }

    #[test]
    fn restore_clamps_removed_monitors_and_preserves_logical_size_across_dpi() {
        let restored = clamp_to_work_area(frame(), 1.5, (0, 40, 1920, 1040)).unwrap();
        assert_eq!(restored.position, (0, 40));
        assert_eq!(restored.logical_size.0, 560.0);
        assert!((restored.logical_size.1 - 1040.0 / 1.5).abs() < 0.001);
        assert_eq!(restored.scale, 1.5);
        let left_monitor = clamp_to_work_area(frame(), 2.0, (-3840, -200, 3840, 2160)).unwrap();
        assert_eq!(left_monitor.position, frame().position);
        assert_eq!(left_monitor.logical_size, frame().logical_size);
        let oversized = WindowFrame {
            position: (99_999, 99_999),
            logical_size: (8000.0, 8000.0),
            ..frame()
        };
        let fit = clamp_to_work_area(oversized, 2.0, (-1920, 0, 1920, 1080)).unwrap();
        assert_eq!(fit.position, (-1920, 0));
        assert_eq!(fit.logical_size, (960.0, 540.0));
        let fractional = clamp_to_work_area(oversized, 1.25, (0, 0, 1921, 1081)).unwrap();
        assert!((f64::from(fractional.logical_size.0) * 1.25).ceil() <= 1921.0);
        assert!((f64::from(fractional.logical_size.1) * 1.25).ceil() <= 1081.0);
    }

    #[test]
    fn restore_rejects_invalid_scales_tiny_displays_and_coordinate_overflow() {
        for scale in [0.0, f64::NAN, f64::INFINITY, 8.01] {
            assert!(clamp_to_work_area(frame(), scale, (0, 0, 1920, 1080)).is_none());
        }
        assert!(clamp_to_work_area(frame(), 2.0, (0, 0, 100, 100)).is_none());
        assert!(
            clamp_to_work_area(frame(), 1.0, (i32::MAX, i32::MAX, u32::MAX, u32::MAX)).is_none()
        );
        assert!(clamp_to_work_area(frame(), 1.0, (0, 0, 0, 0)).is_none());
    }

    #[test]
    fn validation_is_performed_after_the_transaction_lock() {
        let root = tempfile::tempdir().unwrap();
        let lease = lock(root.path()).unwrap();
        let directory = root.path().to_owned();
        let (started, waiting) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            started.send(()).unwrap();
            save(&directory, &frame())
        });
        waiting.recv().unwrap();
        fs::write(root.path().join(ARCHIVE), b"preserve damaged archive").unwrap();
        drop(lease);
        assert!(writer.join().unwrap().is_err());
        assert_eq!(
            fs::read(root.path().join(ARCHIVE)).unwrap(),
            b"preserve damaged archive"
        );
    }

    #[cfg(unix)]
    #[test]
    fn permissions_are_private_and_links_are_never_followed() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        save(&directory, &frame()).unwrap();
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for name in [ARCHIVE, LOCK] {
            assert_eq!(
                fs::metadata(directory.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let linked = root.path().join("linked");
        symlink(&directory, &linked).unwrap();
        assert!(load(&linked).is_err());
        assert!(save(&linked, &frame()).is_err());
        let outside = root.path().join("outside");
        fs::write(&outside, b"untouched").unwrap();
        for name in [ARCHIVE, LOCK] {
            fs::remove_file(directory.join(name)).unwrap();
            symlink(&outside, directory.join(name)).unwrap();
            assert!(save(&directory, &frame()).is_err());
            assert_eq!(fs::read(&outside).unwrap(), b"untouched");
            fs::remove_file(directory.join(name)).unwrap();
        }
    }
}
