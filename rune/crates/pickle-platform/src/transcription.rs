//! Explicit local whisper.cpp process adapter. Audio goes through stdin and is never
//! written to a file. No shell, model download, microphone, or cloud endpoint is used.
use super::PlatformError;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalTranscriberConfig {
    pub executable: PathBuf,
    pub model: PathBuf,
    #[serde(default = "default_language")]
    pub language: String,
}
fn default_language() -> String {
    "auto".into()
}
impl LocalTranscriberConfig {
    pub fn validate(&self) -> Result<(), PlatformError> {
        if !self.executable.is_absolute() || !self.model.is_absolute() {
            return Err(error(
                "Choose absolute paths to the local whisper.cpp executable and downloaded model.",
            ));
        }
        if !self.executable.is_file() || !self.model.is_file() {
            return Err(error(
                "The local transcription executable or model is missing. Choose existing files in Settings.",
            ));
        }
        #[cfg(target_os = "windows")]
        if !self
            .executable
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        {
            return Err(error(
                "Choose the local whisper-cli.exe program, not a batch file, script, or shortcut.",
            ));
        }
        if std::fs::metadata(&self.model)
            .map_err(|_| error("The local transcription model cannot be read."))?
            .len()
            == 0
        {
            return Err(error("The local transcription model is empty."));
        }
        if self.language.is_empty()
            || self.language.len() > 16
            || !self
                .language
                .bytes()
                .all(|c| c.is_ascii_alphabetic() || c == b'-')
        {
            return Err(error(
                "Use a language code such as en, or auto for local detection.",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&self.executable)
                .map_err(|_| error("The transcription executable cannot be read."))?
                .permissions()
                .mode()
                & 0o111
                == 0
            {
                return Err(error("The chosen transcription file is not executable."));
            }
        }
        Ok(())
    }
}
fn error(message: &str) -> PlatformError {
    PlatformError(message.into())
}
fn configuration() -> &'static RwLock<Option<LocalTranscriberConfig>> {
    static VALUE: OnceLock<RwLock<Option<LocalTranscriberConfig>>> = OnceLock::new();
    VALUE.get_or_init(|| RwLock::new(None))
}
/// Validated settings staged before a caller commits its persistent transaction.
/// Opaque ownership prevents callers from substituting an unvalidated value.
pub struct PreparedLocalTranscriber(Option<LocalTranscriberConfig>);

pub fn prepare_local_transcriber(
    value: Option<LocalTranscriberConfig>,
) -> Result<PreparedLocalTranscriber, PlatformError> {
    if let Some(value) = &value {
        value.validate()?;
    }
    Ok(PreparedLocalTranscriber(value))
}

/// Install an already prepared value after persistence succeeds. This performs
/// no file I/O or revalidation and cannot make a successful disk commit fail.
/// Real recording still revalidates engine/model files before capturing audio.
pub fn install_local_transcriber(value: PreparedLocalTranscriber) {
    *configuration()
        .write()
        .unwrap_or_else(|poison| poison.into_inner()) = value.0;
}

pub fn configure_local_transcriber(
    value: Option<LocalTranscriberConfig>,
) -> Result<(), PlatformError> {
    install_local_transcriber(prepare_local_transcriber(value)?);
    Ok(())
}
/// Returns cached, explicitly configured settings; no file I/O in a Rune view.
pub fn local_transcriber_config() -> Option<LocalTranscriberConfig> {
    configuration()
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn pcm16_wav(samples: &[u8]) -> Result<Vec<u8>, PlatformError> {
    if samples.is_empty() || !samples.len().is_multiple_of(2) || samples.len() > 30 * 16_000 * 2 {
        return Err(error("Audio exceeded the 30-second recording budget."));
    }
    let mut wav = Vec::with_capacity(44 + samples.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(samples);
    Ok(wav)
}
fn validate_wav(wav: &[u8]) -> Result<(), PlatformError> {
    if wav.len() <= 44
        || wav.len() > 960_044
        || !(wav.len() - 44).is_multiple_of(2)
        || &wav[0..4] != b"RIFF"
        || &wav[8..16] != b"WAVEfmt "
        || &wav[36..40] != b"data"
        || wav[16..20] != 16u32.to_le_bytes()
        || wav[20..24] != [1, 0, 1, 0]
        || wav[24..28] != 16_000u32.to_le_bytes()
        || wav[28..32] != 32_000u32.to_le_bytes()
        || wav[32..34] != 2u16.to_le_bytes()
        || wav[34..36] != 16u16.to_le_bytes()
        || wav[40..44] != (wav.len() as u32 - 44).to_le_bytes()
        || wav[4..8] != (wav.len() as u32 - 8).to_le_bytes()
    {
        return Err(error(
            "Local transcription requires a bounded 16 kHz mono PCM recording.",
        ));
    }
    Ok(())
}

/// Blocking worker operation with bounded input/output, cancellation, and a deadline.
/// The chosen executable is user-trusted and receives the recording; never run a shell.
pub fn transcribe_wav(
    wav: &[u8],
    config: &LocalTranscriberConfig,
    cancel: CancellationToken,
) -> Result<String, PlatformError> {
    if cancel.is_cancelled() {
        return Err(error("Listening was cancelled."));
    }
    validate_wav(wav)?;
    config.validate()?;
    let mut command = Command::new(&config.executable);
    command
        .arg("-m")
        .arg(&config.model)
        .args(["-f", "-", "-nt", "-np", "-l"])
        .arg(&config.language)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| error("The configured local transcription executable could not start."))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| error("Could not open the local audio input pipe."))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error("Could not open the local transcript output pipe."))?;
    let bytes = wav.to_vec();
    let writer = std::thread::spawn(move || {
        let mut stdin = stdin;
        stdin.write_all(&bytes)
    });
    let overflow = Arc::new(AtomicBool::new(false));
    let reader_overflow = overflow.clone();
    let reader = std::thread::spawn(move || -> Result<Vec<u8>, std::io::Error> {
        let mut result = Vec::new();
        let mut stdout = stdout;
        let mut buffer = [0u8; 4096];
        loop {
            let read = stdout.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            if result.len() + read > 12_000 {
                reader_overflow.store(true, Ordering::Release);
                break;
            }
            result.extend_from_slice(&buffer[..read]);
        }
        Ok(result)
    });
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if cancel.is_cancelled() || Instant::now() >= deadline || overflow.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            let _ = reader.join();
            return Err(error(if cancel.is_cancelled() {
                "Listening was cancelled."
            } else if overflow.load(Ordering::Acquire) {
                "The local transcript exceeded its size limit."
            } else {
                "Local transcription timed out. Try a smaller model or paste a transcript."
            }));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                let _ = reader.join();
                return Err(error("The local transcription process failed."));
            }
        }
    };
    let input = writer
        .join()
        .map_err(|_| error("The local audio input was interrupted."))?;
    let output = reader
        .join()
        .map_err(|_| error("The local transcript was interrupted."))?
        .map_err(|_| error("The local transcript could not be read."))?;
    if cancel.is_cancelled() {
        return Err(error("Listening was cancelled."));
    }
    if !status.success() || input.is_err() {
        return Err(error(
            "The local transcription engine could not read this recording. Check its executable and model in Settings.",
        ));
    }
    if overflow.load(Ordering::Acquire) {
        return Err(error("The local transcript exceeded its size limit."));
    }
    let text = String::from_utf8(output)
        .map_err(|_| error("The local transcript has an invalid text encoding."))?;
    let text = text.trim();
    if text.is_empty() {
        return Err(error(
            "No speech was heard. Play the source audio and try again.",
        ));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
    {
        return Err(error("The local transcript contains invalid text."));
    }
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_input_is_memory_only_bounded_and_well_formed() {
        let wav = pcm16_wav(&[0, 0, 1, 0]).unwrap();
        validate_wav(&wav).unwrap();
        assert!(pcm16_wav(&[0]).is_err());
        assert!(pcm16_wav(&vec![0; 960_002]).is_err());
    }
    #[test]
    fn relative_or_missing_engine_files_are_rejected() {
        let config = LocalTranscriberConfig {
            executable: "whisper-cli".into(),
            model: "model.bin".into(),
            language: "auto".into(),
        };
        assert!(config.validate().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn local_engine_receives_stdin_and_returns_only_bounded_transcript() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("whisper-cli");
        let model = directory.path().join("model.bin");
        std::fs::write(
            &executable,
            "#!/bin/sh\nwhile read -r line; do :; done\nprintf 'A local transcript.'\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&model, b"fixture model").unwrap();
        let config = LocalTranscriberConfig {
            executable,
            model,
            language: "auto".into(),
        };
        assert_eq!(
            transcribe_wav(
                &pcm16_wav(&[1, 0, 2, 0]).unwrap(),
                &config,
                CancellationToken::new()
            )
            .unwrap(),
            "A local transcript."
        );
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(
            transcribe_wav(&[], &config, cancelled)
                .unwrap_err()
                .0
                .contains("cancelled")
        );
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            2,
            "No audio or transcript files may be created"
        );
    }
    #[cfg(unix)]
    fn engine(script: &str) -> (tempfile::TempDir, LocalTranscriberConfig) {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("whisper-cli");
        let model = directory.path().join("model.bin");
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&model, b"fixture").unwrap();
        (
            directory,
            LocalTranscriberConfig {
                executable,
                model,
                language: "auto".into(),
            },
        )
    }
    #[cfg(unix)]
    #[test]
    fn cancelling_an_active_local_engine_terminates_the_process() {
        let (_directory, config) =
            engine("#!/bin/sh\nwhile read -r line; do :; done\nexec sleep 10\n");
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.cancel();
        });
        let began = Instant::now();
        let result = transcribe_wav(&pcm16_wav(&[1, 0, 2, 0]).unwrap(), &config, cancel);
        assert!(result.unwrap_err().0.contains("cancelled"));
        assert!(began.elapsed() < Duration::from_secs(2));
    }
    #[cfg(unix)]
    #[test]
    fn oversized_engine_output_is_killed_and_never_becomes_a_transcript() {
        let (_directory, config) = engine(
            "#!/bin/sh\nwhile read -r line; do :; done\nwhile :; do printf 'A long output that exceeds the permitted transcript size. '; done\n",
        );
        let began = Instant::now();
        let result = transcribe_wav(
            &pcm16_wav(&[1, 0, 2, 0]).unwrap(),
            &config,
            CancellationToken::new(),
        );
        assert!(result.unwrap_err().0.contains("size limit"));
        assert!(began.elapsed() < Duration::from_secs(2));
    }
}
