//! Content-free installed-package OCR probe. The only recognized pixels are a
//! fixed word drawn into an owned memory DIB; no desktop/window DC is obtained.
use super::PlatformError;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use windows::{
    Graphics::Imaging::{
        BitmapAlphaMode, BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap,
    },
    Media::Ocr::OcrEngine,
    Storage::Streams::{Buffer, DataReader},
    Win32::{
        Foundation::{COLORREF, RECT},
        Graphics::Gdi::*,
        System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    },
    core::w,
};

fn error(message: &str) -> PlatformError {
    PlatformError(message.into())
}

pub(super) fn verify_ocr_runtime() -> Result<(), PlatformError> {
    if !super::windows_context::has_package_identity() {
        return Err(error(
            "Windows OCR verification requires actual package identity. Register the supported identity package for this exact installed executable first.",
        ));
    }
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    if ACTIVE.swap(true, Ordering::AcqRel) {
        return Err(error("An OCR verification is already running."));
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let worker = std::thread::Builder::new()
        .name("pickle-ocr-verification".into())
        .spawn(move || {
            struct ActiveGuard;
            impl Drop for ActiveGuard {
                fn drop(&mut self) {
                    ACTIVE.store(false, Ordering::Release);
                }
            }
            let _active = ActiveGuard;
            let _ = sender.send(verify_on_mta(&worker_cancel));
        });
    if worker.is_err() {
        ACTIVE.store(false, Ordering::Release);
        return Err(error("The local OCR verification worker could not start."));
    }
    // The normal recognition is bounded to five seconds. Failed fixed-fixture probes may
    // additionally collect four two-second comparisons; none can turn that failure green.
    match receiver.recv_timeout(Duration::from_secs(16)) {
        Ok(result) => result,
        Err(_) => {
            cancel.cancel();
            Err(error(
                "Windows OCR verification did not finish within its bounded local deadline.",
            ))
        }
    }
}

fn verify_on_mta(cancel: &CancellationToken) -> Result<(), PlatformError> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|_| error("The Windows Runtime OCR apartment could not initialize."))?;
    struct RuntimeGuard;
    impl Drop for RuntimeGuard {
        fn drop(&mut self) {
            unsafe { RoUninitialize() }
        }
    }
    let _runtime = RuntimeGuard;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| error(
        "The Windows OCR recognizer is unavailable. An installed OCR language matching the current user's language preferences is required; this probe does not install languages."))?;
    let fixture = fixture_image()?;
    write_owned_fixture_artifact(&fixture, 96);
    let bitmap = super::windows_context::ocr_software_bitmap(&fixture, BitmapAlphaMode::Ignore)
        .map_err(|_| error("The fixed OCR SoftwareBitmap could not be constructed."))?;
    let text = super::windows_context::recognize_software_bitmap(
        &bitmap,
        cancel,
        &engine,
        Duration::from_secs(5),
    )
    .map_err(|_| {
        error("Windows OCR could not recognize the controlled local fixture within its deadline.")
    })?;
    // This text comes only from fixture_image(), never the source/window OCR capture path.
    // Emit escaped, bounded evidence before the strict comparison so a failed CI probe is diagnosable.
    let language = engine
        .RecognizerLanguage()
        .and_then(|language| language.LanguageTag())
        .map(|tag| tag.to_string_lossy())
        .unwrap_or_else(|_| "unavailable".into());
    let dark_pixels = fixture
        .pixels()
        .filter(|pixel| pixel[0] < 32 && pixel[1] < 32 && pixel[2] < 32)
        .count();
    eprintln!(
        "{}",
        super::windows_capabilities::ocr_fixture_diagnostic(
            &text,
            &language,
            fixture.width(),
            fixture.height(),
            dark_pixels
        )
    );
    emit_bitmap_diagnostic("production-bgra-ignore", &bitmap, &fixture, 96, Some(&text));
    if !super::windows_capabilities::ocr_fixture_matches(&text) {
        compare_owned_fixture(&bitmap, &fixture, &engine, cancel);
        let _ = bitmap.Close();
        return Err(error(
            "Windows OCR did not recognize the expected controlled fixture word.",
        ));
    }
    let _ = bitmap.Close();
    Ok(())
}

fn write_owned_fixture_artifact(fixture: &image::RgbaImage, font_height: i32) {
    // Only an explicit verifier invocation reaches this function, and only its generated word
    // is written. Production screen/source OCR never reads this environment variable.
    let Some(path) = std::env::var_os("PICKLE_OCR_FIXTURE_PNG") else {
        return;
    };
    let mut path = std::path::PathBuf::from(path);
    if font_height != 96 {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            return;
        };
        path.set_file_name(format!("{stem}-font{font_height}.png"));
    }
    let result = (|| {
        if !path.is_absolute() || path.extension().is_none_or(|extension| extension != "png") {
            return Err(());
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| ())?;
        image::DynamicImage::ImageRgba8(fixture.clone())
            .write_to(&mut file, image::ImageFormat::Png)
            .map_err(|_| ())?;
        Ok(())
    })();
    eprintln!(
        "{}",
        serde_json::json!({"schema":1,"test":"windows-ocr-owned-fixture-artifact","written":result.is_ok(),"width":fixture.width(),"height":fixture.height(),"font_height":font_height})
    );
}

fn emit_bitmap_diagnostic(
    label: &str,
    bitmap: &SoftwareBitmap,
    fixture: &image::RgbaImage,
    font_height: i32,
    text: Option<&str>,
) {
    let detail = (|| -> windows::core::Result<serde_json::Value> {
        let format = bitmap.BitmapPixelFormat()?;
        let channels = if format == BitmapPixelFormat::Gray8 {
            1
        } else {
            4
        };
        let capacity = fixture.width() * fixture.height() * channels;
        let buffer = Buffer::Create(capacity)?;
        bitmap.CopyToBuffer(&buffer)?;
        let mut pixels = vec![0; buffer.Length()? as usize];
        let reader = DataReader::FromBuffer(&buffer)?;
        reader.ReadBytes(&mut pixels)?;
        let _ = reader.Close();
        let dark_pixels = if channels == 1 {
            pixels.iter().filter(|value| **value < 32).count()
        } else {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] < 32 && pixel[1] < 32 && pixel[2] < 32)
                .count()
        };
        let expected: Vec<u8> = if channels == 1 {
            fixture.pixels().map(|pixel| pixel[0]).collect()
        } else {
            fixture
                .pixels()
                .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
                .collect()
        };
        let locked = bitmap.LockBuffer(BitmapBufferAccessMode::Read)?;
        let plane = locked.GetPlaneDescription(0)?;
        let _ = locked.Close();
        Ok(
            serde_json::json!({"format":format.0,"alpha_mode":bitmap.BitmapAlphaMode()?.0,"dpi_x":bitmap.DpiX()?,"dpi_y":bitmap.DpiY()?,"width":bitmap.PixelWidth()?,"height":bitmap.PixelHeight()?,"buffer_length":pixels.len(),"round_trip_matches_cpu":pixels==expected,"dark_pixels":dark_pixels,"stride":plane.Stride,"start_index":plane.StartIndex,"plane_width":plane.Width,"plane_height":plane.Height}),
        )
    })();
    let mut evidence = serde_json::json!({"schema":1,"test":"windows-ocr-fixed-bitmap-comparison","variant":label,"font":"Segoe UI","font_height":font_height});
    match detail {
        Ok(detail) => evidence["bitmap"] = detail,
        Err(error) => {
            evidence["bitmap_error_hresult"] =
                serde_json::json!(format!("0x{:08x}", error.code().0 as u32))
        }
    }
    if let Some(text) = text {
        evidence["recognized_text"] = serde_json::json!(text.chars().take(128).collect::<String>());
        evidence["recognized_truncated"] = serde_json::json!(text.chars().count() > 128);
        evidence["matched"] =
            serde_json::json!(super::windows_capabilities::ocr_fixture_matches(text));
    }
    eprintln!("{evidence}");
}

fn compare_owned_fixture(
    original: &SoftwareBitmap,
    fixture: &image::RgbaImage,
    engine: &OcrEngine,
    cancel: &CancellationToken,
) {
    // Microsoft sample: https://github.com/microsoft/Windows-universal-samples/blob/main/Samples/OCR/cs/OcrFileImage.xaml.cs
    // Compare the same pixels, not alternate fonts/text; success here is diagnostic only.
    for (label, format, alpha) in [
        (
            "bgra-premultiplied",
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
        ),
        (
            "gray8-ignore",
            BitmapPixelFormat::Gray8,
            BitmapAlphaMode::Ignore,
        ),
        (
            "bgra-ignore-dpi96",
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Ignore,
        ),
    ] {
        if cancel.is_cancelled() {
            break;
        }
        let result = (|| -> windows::core::Result<()> {
            let bitmap = SoftwareBitmap::ConvertWithAlpha(original, format, alpha)?;
            if label == "bgra-ignore-dpi96" {
                bitmap.SetDpiX(96.0)?;
                bitmap.SetDpiY(96.0)?;
            }
            let result = super::windows_context::recognize_software_bitmap(
                &bitmap,
                cancel,
                engine,
                Duration::from_secs(2),
            );
            emit_bitmap_diagnostic(label, &bitmap, fixture, 96, result.as_deref().ok());
            let _ = bitmap.Close();
            result.map(|_| ())
        })();
        if let Err(error) = result {
            eprintln!(
                "{}",
                serde_json::json!({"schema":1,"test":"windows-ocr-fixed-comparison-error","variant":label,"hresult":format!("0x{:08x}",error.code().0 as u32)})
            );
        }
    }
    if !cancel.is_cancelled() {
        let result = (|| -> Result<(), PlatformError> {
            let fixture = fixture_image_with_font(48)?;
            write_owned_fixture_artifact(&fixture, 48);
            let bitmap =
                super::windows_context::ocr_software_bitmap(&fixture, BitmapAlphaMode::Ignore)
                    .map_err(|_| error("The font48 comparison bitmap could not be constructed."))?;
            let result = super::windows_context::recognize_software_bitmap(
                &bitmap,
                cancel,
                engine,
                Duration::from_secs(2),
            );
            emit_bitmap_diagnostic(
                "bgra-ignore-font48",
                &bitmap,
                &fixture,
                48,
                result.as_deref().ok(),
            );
            let _ = bitmap.Close();
            result
                .map(|_| ())
                .map_err(|_| error("The font48 comparison did not complete."))
        })();
        if result.is_err() {
            eprintln!(
                "{}",
                serde_json::json!({"schema":1,"test":"windows-ocr-fixed-comparison-error","variant":"bgra-ignore-font48"})
            );
        }
    }
}

fn fixture_image() -> Result<image::RgbaImage, PlatformError> {
    fixture_image_with_font(96)
}

fn fixture_image_with_font(font_height: i32) -> Result<image::RgbaImage, PlatformError> {
    const WIDTH: u32 = 768;
    const HEIGHT: u32 = 180;
    let failed = || error("The controlled in-memory OCR fixture could not be rendered.");
    struct Canvas {
        dc: HDC,
        bitmap: HBITMAP,
        font: HFONT,
        old_bitmap: HGDIOBJ,
        old_font: HGDIOBJ,
    }
    impl Drop for Canvas {
        fn drop(&mut self) {
            unsafe {
                if !self.old_font.0.is_null() && self.old_font.0 as isize != -1 {
                    SelectObject(self.dc, self.old_font);
                }
                if !self.old_bitmap.0.is_null() && self.old_bitmap.0 as isize != -1 {
                    SelectObject(self.dc, self.old_bitmap);
                }
                if !self.font.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(self.font.0));
                }
                if !self.bitmap.0.is_null() {
                    let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
                }
                let _ = DeleteDC(self.dc);
            }
        }
    }
    // SAFETY: this DC is memory-only, the DIB owns exactly WIDTH*HEIGHT BGRA
    // pixels, and all selected objects are restored before their RAII deletion.
    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.0.is_null() {
            return Err(failed());
        }
        let mut canvas = Canvas {
            dc,
            bitmap: HBITMAP::default(),
            font: HFONT::default(),
            old_bitmap: HGDIOBJ::default(),
            old_font: HGDIOBJ::default(),
        };
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: WIDTH as i32,
                biHeight: -(HEIGHT as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        canvas.bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|_| failed())?;
        if bits.is_null() {
            return Err(failed());
        }
        canvas.old_bitmap = SelectObject(dc, HGDIOBJ(canvas.bitmap.0));
        if canvas.old_bitmap.0.is_null() || canvas.old_bitmap.0 as isize == -1 {
            return Err(failed());
        }
        canvas.font = CreateFontW(
            -font_height,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            u32::from(DEFAULT_PITCH.0 | FF_DONTCARE.0),
            w!("Segoe UI"),
        );
        if canvas.font.0.is_null() {
            return Err(failed());
        }
        canvas.old_font = SelectObject(dc, HGDIOBJ(canvas.font.0));
        if canvas.old_font.0.is_null() || canvas.old_font.0 as isize == -1 {
            return Err(failed());
        }
        let byte_count = WIDTH as usize * HEIGHT as usize * 4;
        std::ptr::write_bytes(bits.cast::<u8>(), 255, byte_count);
        SetTextColor(dc, COLORREF(0));
        SetBkColor(dc, COLORREF(0x00FF_FFFF));
        let mut text: Vec<u16> = "PICKLE".encode_utf16().collect();
        let mut rectangle = RECT {
            left: 0,
            top: 0,
            right: WIDTH as i32,
            bottom: HEIGHT as i32,
        };
        if DrawTextW(
            dc,
            &mut text,
            &mut rectangle,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        ) == 0
            || !GdiFlush().as_bool()
        {
            return Err(failed());
        }
        let mut pixels = std::slice::from_raw_parts(bits.cast::<u8>(), byte_count).to_vec();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        image::RgbaImage::from_raw(WIDTH, HEIGHT, pixels).ok_or_else(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smaller_font_comparison_keeps_the_same_owned_canvas_and_text() {
        let original = fixture_image().unwrap();
        let smaller = fixture_image_with_font(48).unwrap();
        assert_eq!(original.dimensions(), smaller.dimensions());
        let dark = |image: &image::RgbaImage| image.pixels().filter(|pixel| pixel[0] < 32).count();
        assert!(dark(&smaller) > 0);
        assert!(dark(&smaller) < dark(&original));
    }
    #[test]
    fn probe_fixture_is_an_owned_bounded_black_and_white_canvas() {
        let image = fixture_image().unwrap();
        assert_eq!(image.dimensions(), (768, 180));
        assert!(image.pixels().all(|pixel| pixel[3] == 255));
        assert!(
            image
                .pixels()
                .any(|pixel| pixel[0] < 32 && pixel[1] < 32 && pixel[2] < 32)
        );
        assert!(
            image
                .pixels()
                .filter(|pixel| pixel[0] == 255 && pixel[1] == 255 && pixel[2] == 255)
                .count()
                > 768 * 180 / 2
        );
    }
}
