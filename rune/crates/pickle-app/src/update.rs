//! Explicit signed update discovery and download. This module never installs or executes code.
//!
//! A manifest is an envelope containing hex-encoded canonical JSON and an Ed25519 signature.
//! Only the outer envelope is parsed before verification; installer URLs are parsed afterwards.
use std::{
    cmp::Ordering,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::StreamExt;
use reqwest::Response;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use url::Url;

const DOMAIN: &[u8] = b"Pickle update manifest v1\0";
const MAX_ENVELOPE: usize = 65_536;
const MAX_PAYLOAD: usize = 24_000;
const MAX_ARTIFACT: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Config {
    pub manifest_url: String,
    pub public_key_hex: String,
    pub channel: String,
}

impl Config {
    /// Trust is compiled into the application, never fetched from the manifest being verified.
    /// An unconfigured development build is an explicit unavailable state, not an unsigned mode.
    pub fn from_build_environment() -> Result<Option<Self>, String> {
        match (option_env!("PICKLE_UPDATE_MANIFEST_URL"), option_env!("PICKLE_UPDATE_PUBLIC_KEY_HEX")) {
            (None, None) => Ok(None),
            (Some(url), Some(key)) if !url.is_empty() && !key.is_empty() => {
                let config=Self { manifest_url:url.into(), public_key_hex:key.into(), channel:option_env!("PICKLE_UPDATE_CHANNEL").unwrap_or("stable").into() };
                config.validate()?;
                Ok(Some(config))
            }
            _ => Err("This build has incomplete update configuration. Download a release from the publisher.".into()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        secure_url(&self.manifest_url)?;
        public_key(&self.public_key_hex)?;
        if !matches!(self.channel.as_str(), "stable" | "beta") {
            return Err("This build has an unsupported update channel.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub channel: String,
    pub published_at: String,
    pub notes: String,
    artifact: Artifact,
    expires_at: DateTime<Utc>,
}
impl Release {
    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub target: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub filename: String,
}

/// Field order defines the canonical payload encoding used by the release signing tool.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: String,
    channel: String,
    published_at: String,
    expires_at: String,
    notes: String,
    artifacts: Vec<Artifact>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: u32,
    payload_hex: String,
    signature_hex: String,
}

fn cancelled(cancel: &CancellationToken) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err("Update request cancelled.".into())
    } else {
        Ok(())
    }
}

fn secure_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "The update address is invalid.".to_string())?;
    if value.len() > 4096
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Updates require a valid HTTPS address without embedded credentials.".into());
    }
    Ok(url)
}

fn public_key(value: &str) -> Result<VerifyingKey, String> {
    if value.len() != 64 {
        return Err("The update verification key is invalid.".into());
    }
    let bytes =
        hex::decode(value).map_err(|_| "The update verification key is invalid.".to_string())?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "The update verification key is invalid.".to_string())?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| "The update verification key is invalid.".into())
}

pub fn current_target() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        _ => Err("Updates are unavailable for this operating system or architecture.".into()),
    }
}

fn validate_artifact(artifact: &Artifact) -> Result<(), String> {
    secure_url(&artifact.url)?;
    let stem = artifact
        .filename
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if artifact.size == 0
        || artifact.size > MAX_ARTIFACT
        || artifact.filename.len() > 120
        || artifact.filename.starts_with('.')
        || artifact.filename.contains("..")
        || reserved
        || !artifact
            .filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        || hex::decode(&artifact.sha256).map_or(true, |digest| digest.len() != 32)
    {
        return Err("The signed release contains invalid installer metadata.".into());
    }
    let extension = Path::new(&artifact.filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let valid = match artifact.target.as_str() {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => matches!(extension, "dmg" | "zip"),
        "x86_64-pc-windows-msvc" | "aarch64-pc-windows-msvc" => {
            matches!(extension, "exe" | "msi" | "msix" | "msixbundle")
        }
        _ => false,
    };
    if !valid {
        return Err("The signed release has an unsupported installer target or format.".into());
    }
    Ok(())
}

fn verify_manifest(
    config: &Config,
    current: &str,
    bytes: &[u8],
    target: &str,
    now: DateTime<Utc>,
) -> Result<Option<Release>, String> {
    if bytes.len() > MAX_ENVELOPE {
        return Err("The update manifest is too large.".into());
    }
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|_| "The signed update envelope is invalid.".to_string())?;
    if envelope.schema != 1
        || envelope.payload_hex.len() > MAX_PAYLOAD * 2
        || envelope.signature_hex.len() != 128
    {
        return Err("The signed update envelope has an unsupported format.".into());
    }
    let payload = hex::decode(envelope.payload_hex)
        .map_err(|_| "The signed update payload is invalid.".to_string())?;
    let signature = hex::decode(envelope.signature_hex)
        .map_err(|_| "The update signature is invalid.".to_string())?;
    let signature = Signature::from_slice(&signature)
        .map_err(|_| "The update signature is invalid.".to_string())?;
    let mut signed = DOMAIN.to_vec();
    signed.extend_from_slice(&payload);
    public_key(&config.public_key_hex)?
        .verify_strict(&signed, &signature)
        .map_err(|_| {
            "The update manifest signature could not be verified. Nothing was downloaded."
                .to_string()
        })?;

    // Installer URLs and other payload fields are untrusted until the signature passes above.
    let manifest: Manifest = serde_json::from_slice(&payload)
        .map_err(|_| "The verified update manifest is invalid.".to_string())?;
    if serde_json::to_vec(&manifest)
        .map_err(|_| "Could not validate update encoding.".to_string())?
        != payload
    {
        return Err("The signed update manifest is not in the required canonical format.".into());
    }
    if manifest.channel != config.channel
        || manifest.notes.len() > 4000
        || manifest.artifacts.is_empty()
        || manifest.artifacts.len() > 16
    {
        return Err(
            "The signed release does not match this update channel or exceeds supported limits."
                .into(),
        );
    }
    let published = DateTime::parse_from_rfc3339(&manifest.published_at)
        .map_err(|_| "The release publication date is invalid.".to_string())?
        .with_timezone(&Utc);
    let expires = DateTime::parse_from_rfc3339(&manifest.expires_at)
        .map_err(|_| "The update expiry date is invalid.".to_string())?
        .with_timezone(&Utc);
    if published > now + chrono::Duration::minutes(5)
        || expires <= now
        || expires <= published
        || expires - published > chrono::Duration::days(30)
    {
        return Err("The signed update manifest has expired or has invalid dates. Check your clock and retry.".into());
    }
    let latest = Version::parse(&manifest.version)
        .map_err(|_| "The release version is invalid.".to_string())?;
    let current = Version::parse(current)
        .map_err(|_| "The installed application version is invalid.".to_string())?;
    if config.channel == "stable" && !latest.pre.is_empty() {
        return Err("A prerelease cannot be installed from the stable channel.".into());
    }
    let mut targets = std::collections::BTreeSet::new();
    for artifact in &manifest.artifacts {
        validate_artifact(artifact)?;
        if !targets.insert(&artifact.target) {
            return Err("The signed release contains duplicate platform artifacts.".into());
        }
    }
    if latest.cmp_precedence(&current) != Ordering::Greater {
        return Ok(None);
    }
    let artifact = manifest
        .artifacts
        .into_iter()
        .find(|artifact| artifact.target == target)
        .ok_or_else(|| {
            "This release has no installer for this operating system and architecture.".to_string()
        })?;
    Ok(Some(Release {
        version: manifest.version,
        channel: manifest.channel,
        published_at: manifest.published_at,
        notes: manifest.notes,
        artifact,
        expires_at: expires,
    }))
}

async fn response(url: Url, cancel: &CancellationToken, seconds: u64) -> Result<Response, String> {
    cancelled(cancel)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(seconds))
        .build()
        .map_err(|_| "Could not initialize a secure update connection.".to_string())?;
    let response = tokio::select! {
        _=cancel.cancelled()=>return Err("Update request cancelled.".into()),
        result=client.get(url).send()=>result.map_err(|_|"Could not reach the update server. Check your connection and retry.".to_string())?,
    };
    if response.status().as_u16() != 200 {
        return Err(
            "The update server did not return a downloadable release. Try again later.".into(),
        );
    }
    Ok(response)
}

async fn bounded_manifest(
    response: Response,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|bytes| bytes > MAX_ENVELOPE as u64)
    {
        return Err("The update manifest is too large.".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {_=cancel.cancelled()=>return Err("Update request cancelled.".into()),chunk=stream.next()=>chunk};
        let Some(chunk) = chunk else { break };
        let chunk =
            chunk.map_err(|_| "The update manifest download was interrupted.".to_string())?;
        if bytes.len().saturating_add(chunk.len()) > MAX_ENVELOPE {
            return Err("The update manifest is too large.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    cancelled(cancel)?;
    Ok(bytes)
}

/// Explicit user action. This function is never called automatically during launch.
pub async fn check_update(
    config: &Config,
    current_version: &str,
    cancel: CancellationToken,
) -> Result<Option<Release>, String> {
    config.validate()?;
    let target = current_target()?;
    let response = response(secure_url(&config.manifest_url)?, &cancel, 15).await?;
    let bytes = bounded_manifest(response, &cancel).await?;
    cancelled(&cancel)?;
    verify_manifest(config, current_version, &bytes, target, Utc::now())
}

fn private_cache(directory: &Path) -> Result<(), String> {
    if fs::symlink_metadata(directory).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("The update cache must not be a symbolic link.".into());
    }
    fs::create_dir_all(directory).map_err(|_| "Could not create the update cache.".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Could not protect the update cache.".to_string())?;
    }
    Ok(())
}

fn matches_file(
    path: &Path,
    artifact: &Artifact,
    cancel: &CancellationToken,
) -> Result<bool, String> {
    cancelled(cancel)?;
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(false);
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("An unsafe file occupies the installer cache path.".into());
    }
    if metadata.len() != artifact.size {
        return Ok(false);
    }
    let mut file =
        fs::File::open(path).map_err(|_| "Could not inspect the cached installer.".to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65_536];
    let mut count = 0u64;
    loop {
        cancelled(cancel)?;
        let read = file
            .read(&mut buffer)
            .map_err(|_| "Could not verify the cached installer.".to_string())?;
        if read == 0 {
            break;
        }
        count = count.saturating_add(read as u64);
        if count > artifact.size || count > MAX_ARTIFACT {
            return Ok(false);
        }
        hash.update(&buffer[..read]);
    }
    Ok(count == artifact.size
        && hex::encode(hash.finalize()).eq_ignore_ascii_case(&artifact.sha256))
}

/// Downloads to an owner-local cache and publishes the file only after size/hash verification.
/// The returned file is never executed, installed, or opened by this module.
pub async fn download_update(
    release: &Release,
    cache_directory: &Path,
    cancel: CancellationToken,
) -> Result<PathBuf, String> {
    cancelled(&cancel)?;
    if release.expires_at <= Utc::now() {
        return Err("This update offer has expired. Check for updates again.".into());
    }
    if release.artifact.target != current_target()? {
        return Err("This installer is for another operating system or architecture.".into());
    }
    validate_artifact(&release.artifact)?;
    let directory = cache_directory.to_path_buf();
    let destination = directory.join(&release.artifact.filename);
    let inspect_path = destination.clone();
    let inspect_artifact = release.artifact.clone();
    let inspect_cancel = cancel.clone();
    let cached = tokio::task::spawn_blocking(move || {
        private_cache(&directory)?;
        matches_file(&inspect_path, &inspect_artifact, &inspect_cancel)
    })
    .await
    .map_err(|_| "Could not inspect the update cache.".to_string())??;
    if cached {
        cancelled(&cancel)?;
        return Ok(destination);
    }
    let response = response(secure_url(&release.artifact.url)?, &cancel, 300).await?;
    download_response(
        response,
        &release.artifact,
        cache_directory,
        destination,
        &cancel,
    )
    .await
}

async fn download_response(
    response: Response,
    artifact: &Artifact,
    cache_directory: &Path,
    destination: PathBuf,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    if response
        .content_length()
        .is_some_and(|size| size != artifact.size)
    {
        return Err("The installer download size does not match the signed release.".into());
    }
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    let directory = cache_directory.to_path_buf();
    let artifact = artifact.clone();
    let expected_size = artifact.size;
    let write_cancel = cancel.clone();
    let writer = tokio::task::spawn_blocking(move || {
        write_download(receiver, &artifact, &directory, destination, &write_cancel)
    });
    let result=async {
    let mut stream = response.bytes_stream();
    let mut count = 0u64;
    loop {
        let chunk = tokio::select! {_=cancel.cancelled()=>return Err("Update request cancelled.".into()),chunk=stream.next()=>chunk};
        let Some(chunk) = chunk else { break };
        let chunk = chunk
            .map_err(|_| "The installer download was interrupted. Please retry.".to_string())?;
        count = count.saturating_add(chunk.len() as u64);
        if count > expected_size || count > MAX_ARTIFACT {
            return Err("The installer exceeded its signed size. Nothing was installed.".into());
        }
        for part in chunk.chunks(65_536) {
            tokio::select! {
                _=cancel.cancelled()=>return Err("Update request cancelled.".into()),
                result=sender.send(DownloadPart::Data(part.to_vec()))=>result.map_err(|_|"The installer could not be written to the update cache.".to_string())?,
            }
        }
    }
    cancelled(cancel)?;
    sender.send(DownloadPart::Finish).await.map_err(|_|"The installer could not be written to the update cache.".to_string())?;
    Ok::<(),String>(())
    }.await;
    drop(sender);
    let written = writer
        .await
        .map_err(|_| "The installer could not be saved.".to_string())?;
    result?;
    written
}

enum DownloadPart {
    Data(Vec<u8>),
    Finish,
}

fn write_download(
    mut receiver: tokio::sync::mpsc::Receiver<DownloadPart>,
    artifact: &Artifact,
    cache_directory: &Path,
    destination: PathBuf,
    cancel: &CancellationToken,
) -> Result<PathBuf, String> {
    let mut temporary = tempfile::NamedTempFile::new_in(cache_directory)
        .map_err(|_| "Could not stage the installer download.".to_string())?;
    let mut count = 0u64;
    let mut hash = Sha256::new();
    let mut finished = false;
    while let Some(part) = receiver.blocking_recv() {
        cancelled(cancel)?;
        match part {
            DownloadPart::Finish => {
                finished = true;
                break;
            }
            DownloadPart::Data(bytes) => {
                count = count.saturating_add(bytes.len() as u64);
                if count > artifact.size || count > MAX_ARTIFACT {
                    return Err(
                        "The installer exceeded its signed size. Nothing was installed.".into(),
                    );
                }
                hash.update(&bytes);
                temporary.write_all(&bytes).map_err(|_| {
                    "Could not save the installer download. Check available storage.".to_string()
                })?;
            }
        }
    }
    cancelled(cancel)?;
    if !finished {
        return Err("The installer download ended before verification. Please retry.".into());
    }
    if count != artifact.size
        || !hex::encode(hash.finalize()).eq_ignore_ascii_case(&artifact.sha256)
    {
        return Err("The installer checksum or size does not match the signed release. Nothing was installed.".into());
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Could not finish saving the verified installer.".to_string())?;
    cancelled(cancel)?;
    temporary.persist(&destination).map_err(|_| {
        "Could not publish the verified installer. Close any open copy and retry.".to_string()
    })?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::{net::TcpListener, thread};

    // Public, deterministic unit-test material. Never use this key for a real release.
    fn key() -> SigningKey {
        SigningKey::from_bytes(
            &Sha256::digest(b"Pickle test-only update signing fixture; not a production secret")
                .into(),
        )
    }
    fn config() -> Config {
        Config {
            manifest_url: "https://updates.example.com/stable.json".into(),
            public_key_hex: hex::encode(key().verifying_key().as_bytes()),
            channel: "stable".into(),
        }
    }
    fn artifact(bytes: &[u8]) -> Artifact {
        Artifact {
            target: current_target().unwrap().into(),
            url: "https://updates.example.com/Pickle-installer.dmg".into(),
            sha256: hex::encode(Sha256::digest(bytes)),
            size: bytes.len() as u64,
            filename: if cfg!(windows) {
                "Pickle-1.1.0.exe"
            } else {
                "Pickle-1.1.0.dmg"
            }
            .into(),
        }
    }
    fn manifest() -> Manifest {
        let now = Utc::now();
        Manifest {
            version: "1.1.0".into(),
            channel: "stable".into(),
            published_at: (now - chrono::Duration::minutes(1)).to_rfc3339(),
            expires_at: (now + chrono::Duration::days(7)).to_rfc3339(),
            notes: "A tested release.".into(),
            artifacts: vec![artifact(b"installer")],
        }
    }
    fn signed_bytes(payload: Vec<u8>) -> Vec<u8> {
        let mut message = DOMAIN.to_vec();
        message.extend_from_slice(&payload);
        let signature = key().sign(&message);
        serde_json::to_vec(&Envelope {
            schema: 1,
            payload_hex: hex::encode(payload),
            signature_hex: hex::encode(signature.to_bytes()),
        })
        .unwrap()
    }
    fn signed(value: &Manifest) -> Vec<u8> {
        signed_bytes(serde_json::to_vec(value).unwrap())
    }
    fn verify(bytes: &[u8]) -> Result<Option<Release>, String> {
        verify_manifest(
            &config(),
            "1.0.0",
            bytes,
            current_target().unwrap(),
            Utc::now(),
        )
    }

    #[test]
    fn signed_release_is_selected_for_current_platform() {
        let release = verify(&signed(&manifest())).unwrap().unwrap();
        assert_eq!(release.version, "1.1.0");
        assert_eq!(release.artifact().target, current_target().unwrap());
        assert!(release.notes.contains("tested"));
    }

    #[test]
    fn signature_is_checked_before_any_payload_url_is_parsed() {
        let mut value = manifest();
        value.artifacts[0].url = "http://127.0.0.1/private".into();
        let mut envelope: Envelope = serde_json::from_slice(&signed(&value)).unwrap();
        envelope.signature_hex = "00".repeat(64);
        let error = verify(&serde_json::to_vec(&envelope).unwrap()).unwrap_err();
        assert!(error.contains("signature"));
        let mut envelope: Envelope = serde_json::from_slice(&signed(&manifest())).unwrap();
        let mut bytes = hex::decode(&envelope.payload_hex).unwrap();
        bytes[5] ^= 1;
        envelope.payload_hex = hex::encode(bytes);
        assert!(
            verify(&serde_json::to_vec(&envelope).unwrap())
                .unwrap_err()
                .contains("signature")
        );
    }

    #[test]
    fn wrong_root_key_and_noncanonical_payloads_are_rejected() {
        let mut config = config();
        config.public_key_hex =
            hex::encode(SigningKey::from_bytes(&[42; 32]).verifying_key().as_bytes());
        assert!(
            verify_manifest(
                &config,
                "1.0.0",
                &signed(&manifest()),
                current_target().unwrap(),
                Utc::now()
            )
            .is_err()
        );
        assert!(
            verify(&signed_bytes(
                serde_json::to_vec_pretty(&manifest()).unwrap()
            ))
            .unwrap_err()
            .contains("canonical")
        );
        assert!(verify(&vec![b'x'; MAX_ENVELOPE + 1]).is_err());
    }

    #[test]
    fn stale_channel_and_platform_mismatches_are_rejected() {
        let mut value = manifest();
        value.expires_at = (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
        assert!(verify(&signed(&value)).is_err());
        value = manifest();
        value.published_at = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        assert!(verify(&signed(&value)).is_err());
        value = manifest();
        value.channel = "beta".into();
        assert!(verify(&signed(&value)).is_err());
        value = manifest();
        value.version = "1.1.0-beta.1".into();
        assert!(verify(&signed(&value)).is_err());
        value = manifest();
        value.artifacts[0].target = if cfg!(windows) {
            "aarch64-apple-darwin"
        } else {
            "x86_64-pc-windows-msvc"
        }
        .into();
        value.artifacts[0].filename = if cfg!(windows) {
            "Pickle.dmg"
        } else {
            "Pickle.exe"
        }
        .into();
        assert!(
            verify(&signed(&value))
                .unwrap_err()
                .contains("no installer")
        );
    }

    #[test]
    fn same_version_and_downgrades_are_never_offered() {
        for version in ["0.9.0", "1.0.0", "1.0.0+newbuild"] {
            let mut value = manifest();
            value.version = version.into();
            assert!(verify(&signed(&value)).unwrap().is_none());
        }
        let mut value = manifest();
        value.version = "1.0.0".into();
        assert!(
            verify_manifest(
                &config(),
                "1.0.0+installed",
                &signed(&value),
                current_target().unwrap(),
                Utc::now()
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn installer_metadata_cannot_escape_the_cache_or_select_unknown_targets() {
        for filename in [
            "../Pickle.exe",
            "/tmp/Pickle.dmg",
            "C:\\Pickle.exe",
            ".hidden.dmg",
            "Pickle.sh",
            "Pickle..dmg",
            "CON.exe",
            "NUL.dmg",
        ] {
            let mut value = manifest();
            value.artifacts[0].filename = filename.into();
            assert!(verify(&signed(&value)).is_err(), "{filename}");
        }
        let mut value = manifest();
        value.artifacts[0].size = MAX_ARTIFACT + 1;
        assert!(verify(&signed(&value)).is_err());
        value = manifest();
        value.artifacts[0].sha256 = "bad-hash".into();
        assert!(verify(&signed(&value)).is_err());
        value = manifest();
        value.artifacts.push(value.artifacts[0].clone());
        assert!(verify(&signed(&value)).is_err());
        assert!(
            Config {
                manifest_url: "http://updates.example.com/manifest".into(),
                ..config()
            }
            .validate()
            .is_err()
        );
        assert!(
            Config {
                public_key_hex: "bad".into(),
                ..config()
            }
            .validate()
            .is_err()
        );
    }

    async fn fixture_response(
        bytes: &[u8],
        advertised: Option<usize>,
        delay: Duration,
    ) -> Response {
        // HTTP is used only inside this private fixture; public update APIs require HTTPS.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let bytes = bytes.to_vec();
        thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut received = Vec::new();
            while !received.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if socket.read_exact(&mut byte).is_err() {
                    return;
                }
                received.push(byte[0]);
            }
            let length = advertised
                .map(|n| format!("Content-Length: {n}\r\n"))
                .unwrap_or_default();
            let headers = format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{length}\r\n");
            let _ = socket.write_all(headers.as_bytes());
            let _ = socket.flush();
            thread::sleep(delay);
            let _ = socket.write_all(&bytes);
        });
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/fixture"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn verified_download_is_published_atomically_and_cached_hash_is_checked() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("Pickle.dmg");
        let bytes = b"verified installer";
        let artifact = artifact(bytes);
        let response = fixture_response(bytes, Some(bytes.len()), Duration::ZERO).await;
        let path = download_response(
            response,
            &artifact,
            directory.path(),
            destination.clone(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(path, destination);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(matches_file(&path, &artifact, &CancellationToken::new()).unwrap());
        fs::write(&path, b"tampered installer").unwrap();
        assert!(!matches_file(&path, &artifact, &CancellationToken::new()).unwrap());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn tampered_or_oversized_download_leaves_no_installer_or_partial_file() {
        for (bytes, size_header) in [
            (b"bad-bytes".as_slice(), Some(9)),
            (b"too-large-installer".as_slice(), None),
            (b"installer".as_slice(), Some(500)),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let artifact = artifact(b"installer");
            let response = fixture_response(bytes, size_header, Duration::ZERO).await;
            assert!(
                download_response(
                    response,
                    &artifact,
                    directory.path(),
                    directory.path().join("Pickle.dmg"),
                    &CancellationToken::new()
                )
                .await
                .is_err()
            );
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }

    #[tokio::test]
    async fn cancellation_removes_partial_download_and_never_publishes() {
        let directory = tempfile::tempdir().unwrap();
        let artifact = artifact(b"installer");
        let response = fixture_response(b"installer", Some(9), Duration::from_millis(500)).await;
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            trigger.cancel();
        });
        let result = download_response(
            response,
            &artifact,
            directory.path(),
            directory.path().join("Pickle.dmg"),
            &cancel,
        )
        .await;
        assert!(result.unwrap_err().contains("cancelled"));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn dropped_network_producer_cannot_publish_even_if_all_expected_bytes_arrived() {
        let directory = tempfile::tempdir().unwrap();
        let artifact = artifact(b"installer");
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        sender
            .try_send(DownloadPart::Data(b"installer".to_vec()))
            .unwrap();
        drop(sender);
        assert!(
            write_download(
                receiver,
                &artifact,
                directory.path(),
                directory.path().join("Pickle.dmg"),
                &CancellationToken::new()
            )
            .unwrap_err()
            .contains("before verification")
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn manifest_body_budget_is_enforced_and_cancelled_checks_do_not_connect() {
        let response = fixture_response(&vec![b'x'; MAX_ENVELOPE + 1], None, Duration::ZERO).await;
        assert!(
            bounded_manifest(response, &CancellationToken::new())
                .await
                .is_err()
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            check_update(&config(), "1.0.0", cancel)
                .await
                .unwrap_err()
                .contains("cancelled")
        );
    }

    #[cfg(unix)]
    #[test]
    fn cache_rejects_symlinks() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let link = directory.path().join("unsafe");
        symlink(directory.path(), &link).unwrap();
        assert!(private_cache(&link).is_err());
        let artifact = artifact(b"installer");
        assert!(matches_file(&link, &artifact, &CancellationToken::new()).is_err());
    }

    #[test]
    #[ignore = "release-tool integration requires Python 3 and OpenSSL 3; uses a public test-only key"]
    fn signing_script_produces_a_manifest_the_application_accepts() {
        let directory = tempfile::tempdir().unwrap();
        // Deterministic, publicly known fixture material; no publisher key is read or created.
        let mut der = hex::decode("302e020100300506032b657004220420").unwrap();
        der.extend_from_slice(&key().to_bytes());
        let der_path = directory.path().join("public-test-only-key.der");
        let pem_path = directory.path().join("public-test-only-key.pem");
        fs::write(&der_path, der).unwrap();
        let result = std::process::Command::new("openssl")
            .args(["pkey", "-inform", "DER", "-in"])
            .arg(&der_path)
            .arg("-out")
            .arg(&pem_path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "OpenSSL 3 is required for this explicit release-tool check."
        );
        let artifact = artifact(b"installer");
        let installer = directory.path().join(&artifact.filename);
        fs::write(&installer, b"installer").unwrap();
        let output_path = directory.path().join("signed.json");
        let source = Path::new(file!());
        let script = if source.is_absolute() {
            source
                .parent()
                .unwrap()
                .join("../../../scripts/sign-update.py")
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/sign-update.py")
        };
        let python = if cfg!(windows) { "python" } else { "python3" };
        let result = std::process::Command::new(python)
            .arg(script)
            .arg("--key")
            .arg(&pem_path)
            .args([
                "--version",
                "1.1.0",
                "--notes",
                "Révision 日本語 \"quoted\"\nnew line",
                "--artifact",
            ])
            .arg(current_target().unwrap())
            .arg(&installer)
            .arg(&artifact.url)
            .arg("--output")
            .arg(&output_path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let release = verify(&fs::read(output_path).unwrap()).unwrap().unwrap();
        assert_eq!(release.version, "1.1.0");
        assert!(release.notes.contains("日本語"));
        assert_eq!(release.artifact.sha256, artifact.sha256);
    }
}
