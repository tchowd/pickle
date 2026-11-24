//! Single-window Windows Graphics Capture, followed by on-device Windows OCR.
//! Never substitutes a monitor capture or microphone when a source is unavailable.
use super::*;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::*,
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        Imaging::*,
    },
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
    Win32::{
        Foundation::{ERROR_CANCELLED, ERROR_TIMEOUT, HMODULE, HWND},
        Graphics::{Direct3D::D3D_DRIVER_TYPE_HARDWARE, Direct3D11::*, Dxgi::IDXGIDevice},
        System::WinRT::{
            Direct3D11::*, Graphics::Capture::IGraphicsCaptureItemInterop, RO_INIT_MULTITHREADED,
            RoInitialize, RoUninitialize,
        },
        UI::WindowsAndMessaging::GetWindowThreadProcessId,
    },
    core::{HRESULT, Interface, factory},
};

fn unavailable() -> PlatformError {
    PlatformError("This source window could not be captured or read locally. Continue with the passage or use the browser extension.".into())
}
// Optional content-free observer for the explicit owned-window verifier only.
// Normal capture passes None and never emits diagnostic output.
pub(super) struct CaptureDiagnostics(Mutex<CaptureState>);
struct CaptureState {
    stage: &'static str,
    hresult: i32,
    supported: Option<bool>,
    frame_arrived: bool,
    width: u32,
    height: u32,
    failed: bool,
}
impl CaptureDiagnostics {
    pub(super) fn new() -> Self {
        Self(Mutex::new(CaptureState {
            stage: "start",
            hresult: 0,
            supported: None,
            frame_arrived: false,
            width: 0,
            height: 0,
            failed: false,
        }))
    }
    pub(super) fn snapshot(&self) -> serde_json::Value {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        serde_json::json!({"stage":state.stage,"hresult":state.hresult,"supported":state.supported,"frame_arrived":state.frame_arrived,"width":state.width,"height":state.height,"driver_type":D3D_DRIVER_TYPE_HARDWARE.0})
    }
}
type Observer = Option<Arc<CaptureDiagnostics>>;
fn observed(observer: &Observer, update: impl FnOnce(&mut CaptureState)) {
    if let Some(observer) = observer {
        let mut state = observer
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.failed {
            update(&mut state);
        }
    }
}
fn stage(observer: &Observer, value: &'static str) {
    observed(observer, |state| state.stage = value);
}
fn failed(observer: &Observer, code: i32) {
    observed(observer, |state| {
        state.hresult = code;
        state.failed = true;
    });
}
/// Windows.Media.Ocr requires package identity. A traditional unpackaged NSIS
/// installation does not gain identity merely from an application manifest.
pub(super) fn has_package_identity() -> bool {
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut length = 0;
    let status = unsafe { GetCurrentPackageFullName(&mut length, None) };
    super::windows_capabilities::identity_probe_succeeded(status.0, length)
}
pub(super) fn capture_page(source: &CapturedSelection) -> Result<CapturedPage, PlatformError> {
    capture_page_with_cancel(source, tokio_util::sync::CancellationToken::new())
}
pub(super) fn capture_page_with_cancel(
    source: &CapturedSelection,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<CapturedPage, PlatformError> {
    capture_page_observed(source, cancel, None)
}
pub(super) fn capture_page_observed(
    source: &CapturedSelection,
    cancel: tokio_util::sync::CancellationToken,
    observer: Observer,
) -> Result<CapturedPage, PlatformError> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
    if cancel.is_cancelled() {
        return Err(PlatformError("Page capture was cancelled.".into()));
    }
    if !has_package_identity() {
        return Err(PlatformError("Local Windows OCR requires a package-identified Pickle installation. This unpackaged build cannot capture page references; continue with selected text or use the browser extension.".into()));
    }
    if CAPTURE_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(PlatformError(
            "The previous window capture is still stopping. Continue with the passage or retry."
                .into(),
        ));
    }
    let source = source.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker_cancel = cancel.clone();
    let worker_observer = observer.clone();
    let worker = std::thread::Builder::new()
        .name("pickle-window-capture".into())
        .spawn(move || {
            struct CaptureGuard;
            impl Drop for CaptureGuard {
                fn drop(&mut self) {
                    CAPTURE_ACTIVE.store(false, Ordering::Release)
                }
            }
            let _active = CaptureGuard;
            let _ = sender.send(capture(&source, &worker_cancel, &worker_observer));
        });
    if worker.is_err() {
        stage(&observer, "worker-start");
        failed(&observer, 0);
        CAPTURE_ACTIVE.store(false, Ordering::Release);
        return Err(unavailable());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if cancel.is_cancelled() {
            stage(&observer, "caller-cancelled");
            failed(&observer, 0);
            return Err(PlatformError("Page capture was cancelled.".into()));
        }
        if Instant::now() >= deadline {
            stage(&observer, "caller-timeout");
            failed(&observer, 0);
            cancel.cancel();
            return Err(PlatformError(
                "Page capture timed out. Continue with the selected passage.".into(),
            ));
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(unavailable()),
        }
    }
}

fn capture(
    source: &CapturedSelection,
    cancel: &tokio_util::sync::CancellationToken,
    observer: &Observer,
) -> Result<CapturedPage, PlatformError> {
    stage(observer, "runtime-init");
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|error| {
        failed(observer, error.code().0);
        unavailable()
    })?;
    struct RuntimeGuard;
    impl Drop for RuntimeGuard {
        fn drop(&mut self) {
            unsafe { RoUninitialize() }
        }
    }
    let _runtime = RuntimeGuard;
    let value = capture_pixels(source, cancel, observer).map_err(|error| {
        failed(observer, error.code().0);
        unavailable()
    })?;
    if cancel.is_cancelled() {
        return Err(PlatformError("Page capture was cancelled.".into()));
    }
    let mut rgba = value.2;
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let original = image::RgbaImage::from_raw(value.0, value.1, rgba).ok_or_else(unavailable)?;
    let factor = (2000.0 / value.0.max(value.1) as f64).min(1.0);
    let ocr_image = image::imageops::resize(
        &original,
        ((value.0 as f64 * factor) as u32).max(1),
        ((value.1 as f64 * factor) as u32).max(1),
        image::imageops::FilterType::Triangle,
    );
    stage(observer, "ocr");
    let text = recognize(&ocr_image, cancel).map_err(|error| {
        failed(observer, error.code().0);
        PlatformError(ocr_failure_notice(error.code()).into())
    })?;
    if cancel.is_cancelled() {
        return Err(PlatformError("Page capture was cancelled.".into()));
    }
    let bounded: String = text
        .chars()
        .scan(0usize, |used, c| {
            *used += c.len_utf8();
            (*used <= 6000).then_some(c)
        })
        .collect();
    let factor = (1440.0 / value.0.max(value.1) as f64).min(1.0);
    let retained = image::imageops::resize(
        &original,
        ((value.0 as f64 * factor) as u32).max(1),
        ((value.1 as f64 * factor) as u32).max(1),
        image::imageops::FilterType::Triangle,
    );
    let rgb = image::DynamicImage::ImageRgba8(retained).to_rgb8();
    stage(observer, "jpeg");
    for quality in [70, 50, 30] {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode(
                &rgb,
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|_| unavailable())?;
        if jpeg.len() <= 750_000 {
            stage(observer, "complete");
            return Ok(CapturedPage {
                jpeg,
                truncated: bounded.len() < text.len(),
                text: bounded,
            });
        }
    }
    Err(PlatformError(
        "The screenshot is too detailed to retain. Continue with the passage.".into(),
    ))
}

fn validate_target(hwnd: HWND, pid: u32) -> windows::core::Result<()> {
    let mut current = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut current)) };
    if current != pid || current == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(())
}
fn capture_pixels(
    source: &CapturedSelection,
    cancel: &tokio_util::sync::CancellationToken,
    observer: &Observer,
) -> windows::core::Result<(u32, u32, Vec<u8>)> {
    stage(observer, "source");
    super::windows_impl::validate_source_window(source)
        .map_err(|_| windows::core::Error::from_thread())?;
    let hwnd = HWND(
        source
            .source_window
            .ok_or_else(windows::core::Error::from_thread)? as *mut _,
    );
    validate_target(hwnd, source.source_pid)?;
    stage(observer, "support");
    let supported = GraphicsCaptureSession::IsSupported()?;
    observed(observer, |state| state.supported = Some(supported));
    if !supported {
        return Err(windows::core::Error::from_thread());
    }
    stage(observer, "item");
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd) }?;
    let size = item.Size()?;
    if size.Width <= 0 || size.Height <= 0 || size.Width as u64 * size.Height as u64 > 16_000_000 {
        return Err(windows::core::Error::from_thread());
    }
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    stage(observer, "d3d-device");
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    let device = device.ok_or_else(windows::core::Error::from_thread)?;
    let context = context.ok_or_else(windows::core::Error::from_thread)?;
    stage(observer, "dxgi-device");
    let dxgi: IDXGIDevice = device.cast()?;
    stage(observer, "runtime-device");
    let runtime_device: IDirect3DDevice =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }?.cast()?;
    stage(observer, "frame-pool");
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &runtime_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )?;
    stage(observer, "session");
    let session = pool.CreateCaptureSession(&item)?;
    struct CaptureGuard(Direct3D11CaptureFramePool, GraphicsCaptureSession);
    impl Drop for CaptureGuard {
        fn drop(&mut self) {
            let _ = self.1.Close();
            let _ = self.0.Close();
        }
    }
    let _capture = CaptureGuard(pool.clone(), session.clone());
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let event = pool.FrameArrived(&TypedEventHandler::<
        Direct3D11CaptureFramePool,
        windows::core::IInspectable,
    >::new(move |pool, _| {
        if let Some(pool) = pool.as_ref()
            && let Ok(frame) = pool.TryGetNextFrame()
        {
            let _ = sender.try_send(frame);
        }
        Ok(())
    }))?;
    stage(observer, "start-capture");
    session.StartCapture()?;
    stage(observer, "frame-wait");
    let deadline = Instant::now() + Duration::from_secs(2);
    let frame = loop {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            return Err(windows::core::Error::from_thread());
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(frame) => {
                observed(observer, |state| state.frame_arrived = true);
                break frame;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(windows::core::Error::from_thread()),
        }
    };
    let _ = pool.RemoveFrameArrived(event);
    validate_target(hwnd, source.source_pid)?;
    super::windows_impl::validate_source_window(source)
        .map_err(|_| windows::core::Error::from_thread())?;
    stage(observer, "surface");
    let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
    stage(observer, "texture");
    let texture: ID3D11Texture2D = unsafe { access.GetInterface() }?;
    let mut descriptor = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut descriptor) };
    observed(observer, |state| {
        state.width = descriptor.Width;
        state.height = descriptor.Height;
    });
    if descriptor.Width == 0
        || descriptor.Height == 0
        || descriptor.Width as u64 * descriptor.Height as u64 > 16_000_000
    {
        return Err(windows::core::Error::from_thread());
    }
    descriptor.Usage = D3D11_USAGE_STAGING;
    descriptor.BindFlags = 0;
    descriptor.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    descriptor.MiscFlags = 0;
    let mut staging = None;
    stage(observer, "staging");
    unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut staging)) }?;
    let staging = staging.ok_or_else(windows::core::Error::from_thread)?;
    unsafe { context.CopyResource(&staging, &texture) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    stage(observer, "map");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if cancel.is_cancelled() || Instant::now() >= deadline {
            return Err(windows::core::Error::from_thread());
        }
        match unsafe {
            context.Map(
                &staging,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            )
        } {
            Ok(()) => break,
            Err(err)
                if err.code() == windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(err) => return Err(err),
        }
    }
    struct MapGuard<'a>(&'a ID3D11DeviceContext, &'a ID3D11Texture2D);
    impl Drop for MapGuard<'_> {
        fn drop(&mut self) {
            unsafe { self.0.Unmap(self.1, 0) }
        }
    }
    let _mapped = MapGuard(&context, &staging);
    stage(observer, "pixels");
    if mapped.pData.is_null() || mapped.RowPitch < descriptor.Width * 4 {
        return Err(windows::core::Error::from_thread());
    }
    let mut pixels = Vec::with_capacity(descriptor.Width as usize * descriptor.Height as usize * 4);
    for row in 0..descriptor.Height as usize {
        let data = unsafe {
            std::slice::from_raw_parts(
                (mapped.pData as *const u8).add(row * mapped.RowPitch as usize),
                descriptor.Width as usize * 4,
            )
        };
        pixels.extend_from_slice(data);
    }
    let _ = frame.Close();
    Ok((descriptor.Width, descriptor.Height, pixels))
}

fn recognize(
    image: &image::RgbaImage,
    cancel: &tokio_util::sync::CancellationToken,
) -> windows::core::Result<String> {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    recognize_with_engine(image, cancel, &engine, Duration::from_secs(2))
}

pub(super) fn recognize_with_engine(
    image: &image::RgbaImage,
    cancel: &tokio_util::sync::CancellationToken,
    engine: &OcrEngine,
    timeout: Duration,
) -> windows::core::Result<String> {
    let bitmap = ocr_software_bitmap(image, BitmapAlphaMode::Ignore)?;
    let result = recognize_software_bitmap(&bitmap, cancel, engine, timeout);
    let _ = bitmap.Close();
    result
}

pub(super) fn ocr_software_bitmap(
    image: &image::RgbaImage,
    alpha: BitmapAlphaMode,
) -> windows::core::Result<SoftwareBitmap> {
    let mut pixels = image.as_raw().clone();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2)
    }
    let writer = DataWriter::new()?;
    writer.WriteBytes(&pixels)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &writer.DetachBuffer()?,
        BitmapPixelFormat::Bgra8,
        image.width() as i32,
        image.height() as i32,
        alpha,
    )?;
    // CreateCopyWithAlphaFromBuffer is a deep copy; the writer does not own the bitmap pixels.
    let _ = writer.Close();
    Ok(bitmap)
}

// Local stop conditions have no failed Win32 call whose last error can be read.
fn ocr_stop_error(cancelled: bool, timed_out: bool) -> Option<windows::core::Error> {
    let code = if cancelled {
        ERROR_CANCELLED
    } else if timed_out {
        ERROR_TIMEOUT
    } else {
        return None;
    };
    Some(windows::core::Error::from_hresult(HRESULT::from_win32(
        code.0,
    )))
}

fn ocr_failure_notice(code: HRESULT) -> &'static str {
    if code == HRESULT::from_win32(ERROR_CANCELLED.0) {
        "Page capture was cancelled."
    } else if code == HRESULT::from_win32(ERROR_TIMEOUT.0) {
        "Reading source-window text timed out. Continue with the selected passage."
    } else {
        "Windows could not read text from this source window. Continue with the selected passage."
    }
}

pub(super) fn recognize_software_bitmap(
    bitmap: &SoftwareBitmap,
    cancel: &tokio_util::sync::CancellationToken,
    engine: &OcrEngine,
    timeout: Duration,
) -> windows::core::Result<String> {
    let operation = engine.RecognizeAsync(bitmap)?;
    let deadline = Instant::now() + timeout;
    // WinRT AsyncStatus::Started has value zero; bindings expose it from windows-future.
    while operation.Status()?.0 == 0 {
        if let Some(error) = ocr_stop_error(cancel.is_cancelled(), Instant::now() >= deadline) {
            let _ = operation.Cancel();
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let result = operation.GetResults()?;
    let text = result.Text()?.to_string_lossy();
    Ok(text)
}

#[cfg(test)]
mod observer_tests {
    use super::*;
    #[test]
    fn local_ocr_stop_codes_and_notices_do_not_depend_on_thread_last_error() {
        assert!(ocr_stop_error(false, false).is_none());
        let timeout = ocr_stop_error(false, true).unwrap();
        assert_eq!(timeout.code().0, 0x800705b4u32 as i32);
        assert_eq!(
            ocr_failure_notice(timeout.code()),
            "Reading source-window text timed out. Continue with the selected passage."
        );
        for deadline_reached in [false, true] {
            let cancelled = ocr_stop_error(true, deadline_reached).unwrap();
            assert_eq!(cancelled.code().0, 0x800704c7u32 as i32);
            assert_eq!(
                ocr_failure_notice(cancelled.code()),
                "Page capture was cancelled."
            );
        }
        assert_eq!(
            ocr_failure_notice(HRESULT(0x80070057u32 as i32)),
            "Windows could not read text from this source window. Continue with the selected passage."
        );
    }

    #[test]
    fn first_native_failure_stays_latched_after_late_worker_progress() {
        let sink = Arc::new(CaptureDiagnostics::new());
        let observer = Some(sink.clone());
        stage(&observer, "d3d-device");
        failed(&observer, 0x887a0004u32 as i32);
        stage(&observer, "complete");
        observed(&observer, |state| {
            state.frame_arrived = true;
            state.width = 100;
        });
        failed(&observer, 123);
        let snapshot = sink.snapshot();
        assert_eq!(snapshot["stage"], "d3d-device");
        assert_eq!(snapshot["hresult"], 0x887a0004u32 as i32);
        assert_eq!(snapshot["frame_arrived"], false);
        assert_eq!(snapshot["width"], 0);
    }
    #[test]
    fn ordinary_capture_has_no_observer_side_effects() {
        observed(&None, |_| {
            panic!("No diagnostic callback is admitted for normal capture")
        });
    }
}
