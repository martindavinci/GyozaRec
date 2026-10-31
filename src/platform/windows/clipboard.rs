use crate::capture::ClipboardService;
use crate::pixels::PixelBuffer;
use clipboard_win::{Clipboard, Setter, formats};
use std::path::Path;

const ATTEMPTS: usize = 10;

/// `CF_DIB` payload: BITMAPINFOHEADER (32 bpp, BI_RGB) + bottom-up BGRA rows.
fn dib_bytes(img: &PixelBuffer) -> Vec<u8> {
    let (w, h) = (img.width(), img.height());
    let row = w as usize * 4;
    let mut out = Vec::with_capacity(40 + row * h as usize);
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes()); // positive: bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&((row * h as usize) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution and palette fields
    for y in (0..h as usize).rev() {
        out.extend_from_slice(&img.bgra[y * row..(y + 1) * row]);
    }
    out
}

pub struct Win32Clipboard;

impl Win32Clipboard {
    /// Puts `text` on the clipboard as Unicode text.
    pub fn set_text(&self, text: &str) -> bool {
        let Ok(_open) = Clipboard::new_attempts(ATTEMPTS) else { return false };
        clipboard_win::empty().is_ok() && formats::Unicode.write_clipboard(&text).is_ok()
    }
}

impl ClipboardService for Win32Clipboard {
    fn set_image(&self, img: &PixelBuffer) -> bool {
        let Ok(_open) = Clipboard::new_attempts(ATTEMPTS) else { return false };
        if clipboard_win::empty().is_err() {
            return false;
        }
        if img.is_opaque() {
            return formats::RawData(formats::CF_DIB).write_clipboard(&dib_bytes(img)).is_ok();
        }
        // A lasso cut: programs that read PNG keep the transparency, the
        // others get the picture on white.
        if let (Some(png), Ok(bytes)) =
            (clipboard_win::raw::register_format("PNG"), img.encode_png())
        {
            let _ = clipboard_win::raw::set_without_clear(png.get(), &bytes);
        }
        clipboard_win::raw::set_without_clear(formats::CF_DIB, &dib_bytes(&img.on_white())).is_ok()
    }

    fn set_file(&self, path: &Path) -> bool {
        let Ok(_open) = Clipboard::new_attempts(ATTEMPTS) else { return false };
        let path = path.to_string_lossy().into_owned();
        clipboard_win::empty().is_ok() && formats::FileList.write_clipboard(&[path]).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PixelPoint, PixelRect};

    #[test]
    fn dib_is_bottom_up_with_header() {
        let mut img = PixelBuffer::solid(PixelRect::new(0, 0, 2, 3), 0, 0, 0);
        img.put_pixel(PixelPoint { x: 0, y: 2 }, [9, 8, 7, 255]); // last row
        let dib = dib_bytes(&img);
        assert_eq!(dib.len(), 40 + 2 * 3 * 4);
        assert_eq!(&dib[0..4], &40u32.to_le_bytes());
        assert_eq!(&dib[8..12], &3i32.to_le_bytes());
        assert_eq!(&dib[14..16], &32u16.to_le_bytes());
        assert_eq!(&dib[40..44], &[9, 8, 7, 255]); // first stored row = last image row
    }
}
