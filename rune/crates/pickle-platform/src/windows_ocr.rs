//! Content-free installed-package OCR probe. The only recognized pixels are a
//! fixed word drawn into an owned memory DIB; no desktop/window DC is obtained.
use super::PlatformError;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use windows::{
    Media::Ocr::OcrEngine,
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
    match receiver.recv_timeout(Duration::from_secs(8)) {
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
    let text = super::windows_context::recognize_with_engine(
        &fixture,
        cancel,
        &engine,
        Duration::from_secs(5),
    )
    .map_err(|_| {
        error("Windows OCR could not recognize the controlled local fixture within its deadline.")
    })?;
    if !super::windows_capabilities::ocr_fixture_matches(&text) {
        return Err(error(
            "Windows OCR did not recognize the expected controlled fixture word.",
        ));
    }
    Ok(())
}

fn fixture_image() -> Result<image::RgbaImage, PlatformError> {
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
            -96,
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
