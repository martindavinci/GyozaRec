use crate::capture::ScreenGrabber;
use crate::geometry::PixelRect;
use crate::pixels::PixelBuffer;
use std::ffi::c_void;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, HICON, ICONINFO,
};

/// GDI `BitBlt` grabber; draws the mouse cursor on request.
pub struct GdiGrabber;

impl ScreenGrabber for GdiGrabber {
    fn grab(&self, rect: PixelRect, include_cursor: bool) -> crate::Result<PixelBuffer> {
        if rect.is_empty() {
            return Err(crate::UserError::EmptyArea.into());
        }
        // SAFETY: every GDI object created here is released before returning;
        // `bits` points into the DIB section, valid until DeleteObject.
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: rect.width,
                    biHeight: -rect.height, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let result = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(crate::Error::from)
                .and_then(|bitmap| {
                    let old = SelectObject(mem, bitmap.into());
                    let copied = BitBlt(
                        mem,
                        0,
                        0,
                        rect.width,
                        rect.height,
                        Some(screen),
                        rect.x,
                        rect.y,
                        SRCCOPY | CAPTUREBLT,
                    );
                    if copied.is_ok() && include_cursor {
                        draw_cursor(mem, rect);
                    }
                    let _ = GdiFlush();
                    let pixels = copied.map(|_| {
                        let len = rect.width as usize * rect.height as usize * 4;
                        let mut data = std::slice::from_raw_parts(bits as *const u8, len).to_vec();
                        for px in data.chunks_exact_mut(4) {
                            px[3] = 255;
                        }
                        PixelBuffer::new(rect, data)
                    });
                    SelectObject(mem, old);
                    let _ = DeleteObject(bitmap.into());
                    pixels.map_err(crate::Error::from)
                });
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            result
        }
    }
}

/// Draws the current cursor into `dc`, whose origin is `rect`'s top-left.
///
/// # Safety
/// `dc` must be a valid memory DC.
unsafe fn draw_cursor(dc: HDC, rect: PixelRect) {
    let mut info =
        CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
    // SAFETY: valid out pointers; the icon bitmaps returned by GetIconInfo are freed.
    unsafe {
        if GetCursorInfo(&mut info).is_err() || info.flags != CURSOR_SHOWING {
            return;
        }
        let icon = HICON(info.hCursor.0);
        let mut icon_info = ICONINFO::default();
        let (hot_x, hot_y) = if GetIconInfo(icon, &mut icon_info).is_ok() {
            if !icon_info.hbmMask.is_invalid() {
                let _ = DeleteObject(icon_info.hbmMask.into());
            }
            if !icon_info.hbmColor.is_invalid() {
                let _ = DeleteObject(icon_info.hbmColor.into());
            }
            (icon_info.xHotspot as i32, icon_info.yHotspot as i32)
        } else {
            (0, 0)
        };
        let x = info.ptScreenPos.x - rect.x - hot_x;
        let y = info.ptScreenPos.y - rect.y - hot_y;
        let _ = DrawIconEx(dc, x, y, icon, 0, 0, 0, None, DI_NORMAL);
    }
}
