//! Windows 11 process-loopback capture. Includes only the selected PID and its child
//! processes, never the microphone or all-system loopback. PCM/WAV stays in memory.
use super::*;
use std::{
    sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use windows::{
    Win32::{
        Media::Audio::*,
        System::{
            Com::{BLOB, StructuredStorage::*, *},
            Variant::VT_BLOB,
        },
    },
    core::{AgileReference, HRESULT, Interface, Ref, implement},
};

fn error(message: &str) -> PlatformError {
    PlatformError(message.into())
}
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activation {
    sender: SyncSender<windows::core::Result<AgileReference<IAudioClient>>>,
    // The asynchronous activation's blob points here; retain it through completion.
    _parameters: Box<AUDIOCLIENT_ACTIVATION_PARAMS>,
}
impl IActivateAudioInterfaceCompletionHandler_Impl for Activation_Impl {
    fn ActivateCompleted(
        &self,
        operation: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let result = (|| unsafe {
            let operation = operation.ok()?;
            let mut status = HRESULT(0);
            let mut unknown = None;
            operation.GetActivateResult(&mut status, &mut unknown)?;
            status.ok()?;
            let client: IAudioClient = unknown
                .ok_or_else(windows::core::Error::from_thread)?
                .cast()?;
            AgileReference::new(&client)
        })();
        let _ = self.sender.send(result);
        Ok(())
    }
}

pub(super) fn record_audio(
    source: &CapturedSelection,
    cancel: CancellationToken,
) -> Result<String, PlatformError> {
    if cancel.is_cancelled() {
        return Err(error("Listening was cancelled."));
    }
    let config=local_transcriber_config().ok_or_else(||error("Configure a local whisper.cpp executable and downloaded model in Settings before listening on Windows."))?;
    config.validate()?;
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|_| error("Could not initialize source-app audio capture."))?;
    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    let _com = ComGuard;
    validate_source(source)?;
    let (sender, receiver) = sync_channel(1);
    let parameters = Box::new(AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: source.source_pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    });
    let data = (&*parameters as *const AUDIOCLIENT_ACTIVATION_PARAMS) as *mut u8;
    let handler: IActivateAudioInterfaceCompletionHandler = Activation {
        sender,
        _parameters: parameters,
    }
    .into();
    let variant = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB {
                        cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                        pBlobData: data,
                    },
                },
                ..Default::default()
            }),
        },
    };
    let _activation=unsafe{ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,&IAudioClient::IID,Some(&variant),&handler)}
        .map_err(|_|error("Process-specific audio capture is unavailable. Use Windows 11 and a supported source application."))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let client = loop {
        if cancel.is_cancelled() {
            return Err(error("Listening was cancelled."));
        }
        if Instant::now() >= deadline {
            return Err(error(
                "Source-app audio activation timed out. Play the source and try again.",
            ));
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => {
                break result.and_then(|client| client.resolve()).map_err(|_| {
                    error("Windows could not activate process-specific audio for this source.")
                })?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(error("Source-app audio activation ended unexpectedly.")),
        }
    };
    validate_source(source)?;
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: 16_000,
        nAvgBytesPerSec: 32_000,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    unsafe{client.Initialize(AUDCLNT_SHAREMODE_SHARED,AUDCLNT_STREAMFLAGS_LOOPBACK|AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM|AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,1_000_000,0,&format,None)}
        .map_err(|_|error("This source does not support the local recording format. Use captions or paste a transcript."))?;
    let capture: IAudioCaptureClient = unsafe { client.GetService() }
        .map_err(|_| error("Windows could not start the source-app audio stream."))?;
    unsafe { client.Start() }.map_err(|_| error("Source-app audio could not start."))?;
    struct StreamGuard(IAudioClient);
    impl Drop for StreamGuard {
        fn drop(&mut self) {
            let _ = unsafe { self.0.Stop() };
        }
    }
    let stream = StreamGuard(client);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut next_identity_check = Instant::now();
    let mut samples = Vec::with_capacity(960_000);
    while Instant::now() < deadline && samples.len() < 960_000 {
        if cancel.is_cancelled() {
            return Err(error("Listening was cancelled."));
        }
        if Instant::now() >= next_identity_check {
            validate_source(source)?;
            next_identity_check = Instant::now() + Duration::from_secs(1);
        }
        let mut count = unsafe { capture.GetNextPacketSize() }
            .map_err(|_| error("The source-app audio stream was interrupted."))?;
        while count > 0 {
            let mut data = std::ptr::null_mut();
            let mut frames = 0;
            let mut flags = 0;
            unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }
                .map_err(|_| error("The source-app audio stream was interrupted."))?;
            let remaining = (960_000 - samples.len()) / 2;
            let keep = (frames as usize).min(remaining);
            if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                samples.resize(samples.len() + keep * 2, 0)
            } else if !data.is_null() {
                samples.extend_from_slice(unsafe { std::slice::from_raw_parts(data, keep * 2) })
            } else {
                let _ = unsafe { capture.ReleaseBuffer(frames) };
                return Err(error("The source-app audio stream returned invalid data."));
            }
            unsafe { capture.ReleaseBuffer(frames) }
                .map_err(|_| error("The source-app audio stream was interrupted."))?;
            if samples.len() >= 960_000 {
                break;
            }
            count = unsafe { capture.GetNextPacketSize() }
                .map_err(|_| error("The source-app audio stream was interrupted."))?;
            if cancel.is_cancelled() {
                return Err(error("Listening was cancelled."));
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(stream);
    if cancel.is_cancelled() {
        return Err(error("Listening was cancelled."));
    }
    if !samples
        .as_chunks::<2>()
        .0
        .iter()
        .any(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 32)
    {
        return Err(error(
            "No source-app audio was heard. Play the source and try again.",
        ));
    }
    let wav = super::transcription::pcm16_wav(&samples)?;
    super::transcribe_wav(&wav, &config, cancel)
}

fn validate_source(source: &CapturedSelection) -> Result<(), PlatformError> {
    super::windows_impl::validate_source_window(source)
}
