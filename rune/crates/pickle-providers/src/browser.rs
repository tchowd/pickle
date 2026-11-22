//! Chromium native messaging and an authenticated loopback relay. Source content stays in memory.
use chrono::Utc;
use pickle_core::{PickleError, WebReference};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore, mpsc::UnboundedSender, oneshot},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::transport::error;

mod uninstall;
pub use uninstall::{uninstall_browser_host, uninstall_hosts_for_application};

const MAX_NATIVE: usize = 90_000;
const MAX_RELAY: usize = 100_000;
const MAX_REPLY: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPayload {
    pub id: Uuid,
    pub captured_at: f64,
    pub selection: String,
    pub reference: WebReference,
    #[serde(rename = "bundleID")]
    pub browser_id: String,
}
impl BrowserPayload {
    pub fn browser(&self) -> Option<BrowserKind> {
        BrowserKind::ALL
            .into_iter()
            .find(|kind| kind.identity() == self.browser_id)
    }
    pub fn is_excluded(&self, exclusions: &[String]) -> bool {
        exclusions.iter().any(|excluded| {
            let excluded = excluded.trim().to_ascii_lowercase();
            excluded == self.browser_id.to_ascii_lowercase()
                || self.browser().is_some_and(|browser| {
                    let executable = match browser {
                        BrowserKind::Chrome => "chrome.exe",
                        BrowserKind::Edge => "msedge.exe",
                        BrowserKind::Brave => "brave.exe",
                        BrowserKind::Arc => "arc.exe",
                    };
                    let basename = excluded.rsplit(['\\', '/']).next().unwrap_or(&excluded);
                    basename == executable
                        || excluded
                            .strip_prefix("windows:")
                            .is_some_and(|value| value == executable)
                })
        })
    }
    pub fn validate(&self) -> Result<(), PickleError> {
        if !self.captured_at.is_finite()
            || (Utc::now().timestamp_millis() as f64 / 1000.0 - self.captured_at).abs() >= 60.0
            || self.selection.len() > 12_000
            || !BrowserKind::ALL
                .iter()
                .any(|kind| kind.identity() == self.browser_id)
        {
            return Err(error(
                "This browser capture expired or is unsupported. Try again.",
            ));
        }
        self.reference.validate()
    }
}

pub struct BrowserRequest {
    pub payload: BrowserPayload,
    /// Reply after the app validates its current pause, exclusion, context, and session policy.
    pub reply: oneshot::Sender<Result<(), String>>,
}

#[derive(Serialize, Deserialize)]
struct Connection {
    port: u16,
    token: String,
}

pub struct BrowserBridge {
    directory: PathBuf,
    token: String,
    cancel: CancellationToken,
}
impl BrowserBridge {
    pub async fn start(
        directory: PathBuf,
        receive: UnboundedSender<BrowserRequest>,
        parent_cancel: CancellationToken,
    ) -> Result<Self, PickleError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| error("Could not start the local browser bridge."))?;
        let port = listener
            .local_addr()
            .map_err(|_| error("Could not inspect the browser bridge."))?
            .port();
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        private_dir(&directory)?;
        private_json(
            &directory.join("connection.json"),
            &Connection {
                port,
                token: token.clone(),
            },
        )?;
        let cancel = parent_cancel.child_token();
        let running_cancel = cancel.clone();
        let auth = token.clone();
        let seen = Arc::new(Mutex::new(HashMap::new()));
        tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(4));
            loop {
                let incoming = tokio::select! {
                    _ = running_cancel.cancelled() => break,
                    result = listener.accept() => result,
                };
                let Ok((socket, address)) = incoming else {
                    break;
                };
                if !address.ip().is_loopback() {
                    continue;
                }
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let token = auth.clone();
                let sender = receive.clone();
                let seen = seen.clone();
                let task_cancel = running_cancel.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    tokio::select! {
                        _ = task_cancel.cancelled() => {},
                        _ = tokio::time::timeout(Duration::from_secs(5), relay(socket, &token, sender, seen)) => {},
                    }
                });
            }
        });
        Ok(Self {
            directory,
            token,
            cancel,
        })
    }
    pub fn stop(&self) {
        self.cancel.cancel();
        self.remove_owned_config();
    }
    fn remove_owned_config(&self) {
        let path = self.directory.join("connection.json");
        if let Ok(bytes) = std::fs::read(&path)
            && serde_json::from_slice::<Connection>(&bytes)
                .is_ok_and(|value| constant_time_equal(&value.token, &self.token))
        {
            let _ = std::fs::remove_file(path);
        }
    }
}
impl Drop for BrowserBridge {
    fn drop(&mut self) {
        self.stop();
    }
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let mut difference = usize::from(left.len() != right.len());
    for (index, byte) in left.bytes().enumerate() {
        difference |= usize::from(byte ^ right.as_bytes().get(index).copied().unwrap_or(0));
    }
    difference == 0
}

async fn relay(
    mut socket: TcpStream,
    token: &str,
    receive: UnboundedSender<BrowserRequest>,
    seen: Arc<Mutex<HashMap<Uuid, Instant>>>,
) -> Result<(), PickleError> {
    let result = async {
        let mut bytes = Vec::new();
        loop {
            let mut part = [0u8; 4096];
            let count = socket
                .read(&mut part)
                .await
                .map_err(|_| error("Browser connection interrupted."))?;
            if count == 0 {
                return Err(error("Incomplete browser message."));
            }
            if bytes.len().saturating_add(count) > MAX_RELAY {
                return Err(error("Browser message is too large."));
            }
            bytes.extend_from_slice(&part[..count]);
            if bytes.contains(&b'\n') {
                break;
            }
        }
        let end = bytes.iter().position(|byte| *byte == b'\n').unwrap();
        if bytes[end + 1..]
            .iter()
            .any(|byte| !byte.is_ascii_whitespace())
        {
            return Err(error("Unexpected browser message data."));
        }
        let envelope: Value =
            serde_json::from_slice(&bytes[..end]).map_err(|_| error("Invalid browser message."))?;
        let supplied = envelope["token"].as_str().unwrap_or("");
        if supplied.len() > 128 || !constant_time_equal(supplied, token) {
            return Err(error("Browser connection rejected."));
        }
        let payload: BrowserPayload = serde_json::from_value(envelope["payload"].clone())
            .map_err(|_| error("Invalid browser capture."))?;
        payload.validate()?;
        {
            let mut seen = seen.lock().await;
            seen.retain(|_, time| time.elapsed() < Duration::from_secs(60));
            if seen.contains_key(&payload.id) {
                return Err(error("This browser capture was already used."));
            }
            if seen.len() >= 1024 {
                return Err(error("Too many browser captures. Wait and try again."));
            }
            seen.insert(payload.id, Instant::now());
        }
        let (reply, acknowledgment) = oneshot::channel();
        receive
            .send(BrowserRequest { payload, reply })
            .map_err(|_| error("Open Pickle and enable browser context."))?;
        acknowledgment
            .await
            .map_err(|_| error("Pickle could not accept this browser context."))?
            .map_err(error)
    }
    .await;
    let response = match result {
        Ok(()) => json!({"ok":"true"}),
        Err(error) => json!({"error":error.to_string()}),
    };
    socket
        .write_all(response.to_string().as_bytes())
        .await
        .map_err(|_| error("Browser response interrupted."))?;
    socket
        .shutdown()
        .await
        .map_err(|_| error("Browser response interrupted."))
}

fn private_dir(path: &Path) -> Result<(), PickleError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(error(
            "Browser configuration directory must not be a symbolic link.",
        ));
    }
    std::fs::create_dir_all(path)
        .map_err(|_| error("Could not create the browser configuration directory."))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| error("Could not protect browser configuration."))?;
    }
    Ok(())
}

fn private_json(path: &Path, value: &impl Serialize) -> Result<(), PickleError> {
    let parent = path
        .parent()
        .ok_or_else(|| error("Invalid browser configuration path."))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| error("Could not stage browser configuration."))?;
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| error("Invalid browser configuration."))?;
    temp.write_all(&bytes)
        .map_err(|_| error("Could not save browser configuration."))?;
    temp.as_file()
        .sync_all()
        .map_err(|_| error("Could not sync browser configuration."))?;
    temp.persist(path)
        .map_err(|_| error("Could not replace browser configuration."))?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BrowserKind {
    Chrome,
    Edge,
    Brave,
    Arc,
}
impl BrowserKind {
    pub const ALL: [Self; 4] = [Self::Chrome, Self::Edge, Self::Brave, Self::Arc];
    pub fn label(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Brave => "Brave",
            Self::Arc => "Arc",
        }
    }
    pub fn identity(self) -> &'static str {
        #[cfg(target_os = "windows")]
        {
            match self {
                Self::Chrome => "windows-browser:chrome",
                Self::Edge => "windows-browser:edge",
                Self::Brave => "windows-browser:brave",
                Self::Arc => "windows-browser:arc",
            }
        }
        #[cfg(not(target_os = "windows"))]
        match self {
            Self::Chrome => "com.google.Chrome",
            Self::Edge => "com.microsoft.edgemac",
            Self::Brave => "com.brave.Browser",
            Self::Arc => "company.thebrowser.Browser",
        }
    }
    #[cfg(target_os = "macos")]
    fn manifest_folder(self) -> &'static str {
        match self {
            Self::Chrome => "Google/Chrome",
            Self::Edge => "Microsoft Edge",
            Self::Brave => "BraveSoftware/Brave-Browser",
            Self::Arc => "Arc/User Data",
        }
    }
    #[cfg(target_os = "windows")]
    fn registry_key(self) -> Option<&'static str> {
        match self {
            Self::Chrome => {
                Some("Software\\Google\\Chrome\\NativeMessagingHosts\\com.pickle.reader")
            }
            Self::Edge => {
                Some("Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.pickle.reader")
            }
            Self::Brave => Some(
                "Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts\\com.pickle.reader",
            ),
            Self::Arc => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct HostRegistration {
    pub browser: BrowserKind,
    pub allowed_origin: String,
    pub bridge_directory: PathBuf,
    pub application: PathBuf,
}

/// Explicit setup action. Installs a browser-specific binary copy and origin-bound registration.
pub fn install_browser_host(
    browser: BrowserKind,
    extension_id: &str,
    host_binary: &Path,
    application: &Path,
    support_directory: &Path,
) -> Result<PathBuf, PickleError> {
    if extension_id.len() != 32
        || !extension_id
            .bytes()
            .all(|byte| (b'a'..=b'p').contains(&byte))
    {
        return Err(error(
            "Paste the 32-character extension ID shown by your browser.",
        ));
    }
    if !host_binary.is_file() || !application.exists() {
        return Err(error(
            "Use the packaged Pickle application to connect the extension.",
        ));
    }
    #[cfg(target_os = "windows")]
    if browser.registry_key().is_none() {
        return Err(error("This browser is not supported on Windows yet."));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    return Err(error(
        "Browser host installation is supported on macOS and Windows.",
    ));
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let directory = support_directory
            .join("NativeHost")
            .join(browser.label().to_ascii_lowercase());
        private_dir(&directory)?;
        let destination = directory.join(if cfg!(windows) {
            "pickle-browser-host.exe"
        } else {
            "pickle-browser-host"
        });
        let mut staged = tempfile::NamedTempFile::new_in(&directory)
            .map_err(|_| error("Could not stage the browser host."))?;
        let mut source = std::fs::File::open(host_binary)
            .map_err(|_| error("Could not read the packaged browser host."))?;
        std::io::copy(&mut source, &mut staged)
            .map_err(|_| error("Could not copy the browser host."))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o700))
                .map_err(|_| error("Could not make the browser host executable."))?;
        }
        staged.persist(&destination).map_err(|_| {
            error("Could not install the browser host. Close browser context requests and retry.")
        })?;
        let registration = HostRegistration {
            browser,
            allowed_origin: format!("chrome-extension://{extension_id}/"),
            bridge_directory: support_directory.join("BrowserBridge"),
            application: application.to_path_buf(),
        };
        private_json(&directory.join("registration.json"), &registration)?;
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
        std::fs::create_dir_all(manifest.parent().unwrap())
            .map_err(|_| error("Could not create the native host manifest directory."))?;
        private_json(
            &manifest,
            &json!({"name":"com.pickle.reader","description":"Pickle page context","path":destination,"type":"stdio","allowed_origins":[registration.allowed_origin]}),
        )?;
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let status = std::process::Command::new("reg.exe")
                .args([
                    "add",
                    &format!("HKCU\\{}", browser.registry_key().unwrap()),
                    "/ve",
                    "/t",
                    "REG_SZ",
                    "/d",
                ])
                .arg(&manifest)
                .arg("/f")
                .creation_flags(0x08000000)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|_| error("Could not register the browser host."))?;
            if !status.success() {
                return Err(error("Windows could not register the browser host."));
            }
        }
        Ok(manifest)
    }
}

fn read_frame(input: &mut impl Read) -> Result<Value, PickleError> {
    let mut header = [0u8; 4];
    input
        .read_exact(&mut header)
        .map_err(|_| error("Incomplete browser message."))?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_NATIVE {
        return Err(error("Page reference is too large."));
    }
    let mut bytes = vec![0; length];
    input
        .read_exact(&mut bytes)
        .map_err(|_| error("Incomplete browser message."))?;
    serde_json::from_slice(&bytes).map_err(|_| error("Invalid browser message."))
}

fn write_frame(output: &mut impl Write, value: Value) -> Result<(), PickleError> {
    let bytes = serde_json::to_vec(&value).map_err(|_| error("Invalid browser response."))?;
    if bytes.len() > MAX_REPLY {
        return Err(error("Browser response is too large."));
    }
    output
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .and_then(|_| output.write_all(&bytes))
        .and_then(|_| output.flush())
        .map_err(|_| error("Could not write browser response."))
}

fn launch_app(path: &Path) {
    #[cfg(target_os = "macos")]
    if path.extension().is_some_and(|value| value == "app") {
        let _ = std::process::Command::new("/usr/bin/open")
            .args(["-g", "-a"])
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        return;
    }
    let mut command = std::process::Command::new(path);
    command
        .arg("--background")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let _ = command.spawn();
}

pub fn relay_native_message(
    mut payload: Value,
    registration: &HostRegistration,
) -> Result<Value, PickleError> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| error("Invalid browser message."))?;
    object.insert("bundleID".into(), json!(registration.browser.identity()));
    let parsed: BrowserPayload =
        serde_json::from_value(payload.clone()).map_err(|_| error("Invalid browser capture."))?;
    parsed.validate()?;
    let start = Instant::now();
    let mut launched = false;
    while start.elapsed() < Duration::from_secs(6) {
        let result = (|| {
            let bytes = std::fs::read(registration.bridge_directory.join("connection.json"))
                .map_err(|_| error("Open Pickle and enable browser context."))?;
            if bytes.len() > 4096 {
                return Err(error("Invalid browser connection configuration."));
            }
            let connection: Connection = serde_json::from_slice(&bytes)
                .map_err(|_| error("Invalid browser connection configuration."))?;
            if connection.port == 0 || connection.token.len() != 64 {
                return Err(error("Invalid browser connection configuration."));
            }
            let mut socket = std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], connection.port)),
                Duration::from_millis(300),
            )
            .map_err(|_| error("Open Pickle and enable browser context."))?;
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| error("Could not configure browser connection."))?;
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .map_err(|_| error("Could not configure browser connection."))?;
            let mut message =
                serde_json::to_vec(&json!({"token":connection.token,"payload":payload}))
                    .map_err(|_| error("Invalid browser capture."))?;
            if message.len() >= MAX_RELAY {
                return Err(error("Browser capture is too large."));
            }
            message.push(b'\n');
            socket
                .write_all(&message)
                .map_err(|_| error("Browser connection interrupted."))?;
            let mut response = Vec::new();
            socket
                .take((MAX_REPLY + 1) as u64)
                .read_to_end(&mut response)
                .map_err(|_| error("Browser response interrupted."))?;
            if response.len() > MAX_REPLY {
                return Err(error("Browser response is too large."));
            }
            serde_json::from_slice(&response).map_err(|_| error("Invalid browser response."))
        })();
        if let Ok(response) = result {
            return Ok(response);
        }
        if !launched {
            launch_app(&registration.application);
            launched = true;
        }
        thread_sleep();
    }
    Err(error(
        "Open Pickle and check that browser context is enabled.",
    ))
}

fn thread_sleep() {
    std::thread::sleep(Duration::from_millis(200));
}

/// Native host entry point. The only stdout bytes are one length-prefixed JSON response.
pub fn run_native_host() {
    let result = (|| {
        let executable =
            std::env::current_exe().map_err(|_| error("Could not locate the browser host."))?;
        let path = executable
            .parent()
            .ok_or_else(|| error("Invalid browser host location."))?
            .join("registration.json");
        let bytes = std::fs::read(path)
            .map_err(|_| error("Connect the browser extension in Pickle Settings first."))?;
        if bytes.len() > 16_384 {
            return Err(error("Invalid browser host registration."));
        }
        let registration: HostRegistration = serde_json::from_slice(&bytes)
            .map_err(|_| error("Invalid browser host registration."))?;
        if std::env::args().nth(1).as_deref() != Some(&registration.allowed_origin) {
            return Err(error("Browser extension origin rejected."));
        }
        let payload = read_frame(&mut std::io::stdin().lock())?;
        relay_native_message(payload, &registration)
    })();
    let response = result.unwrap_or_else(|error| json!({"error":error.to_string()}));
    let _ = write_frame(&mut std::io::stdout().lock(), response);
}

#[cfg(test)]
mod tests;
