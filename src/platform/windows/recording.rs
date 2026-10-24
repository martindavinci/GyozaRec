use super::mp4::Mp4Sink;
use crate::capture::{SinkFactory, SinkKind};
use crate::geometry::PixelSize;
use crate::recording::{FrameSink, GifSink};
use std::path::Path;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, LWA_COLORKEY, SetLayeredWindowAttributes,
    SetWindowDisplayAffinity, SetWindowLongPtrW, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED,
    WS_EX_TRANSPARENT,
};

/// Lets the running instance take the foreground when this process asks it to.
pub fn allow_foreground_for_others() {
    use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    // SAFETY: no pointers; only grants foreground permission.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
}

/// Makes pixels of `key` (`0x00RRGGBB`) fully transparent and the whole
/// window click-through.
pub fn make_color_keyed(hwnd: isize, key: u32) {
    let (r, g, b) = ((key >> 16) & 0xFF, (key >> 8) & 0xFF, key & 0xFF);
    // SAFETY: `hwnd` is a live top-level window owned by this process.
    unsafe {
        let hwnd = HWND(hwnd as *mut _);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0) as isize);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(r | g << 8 | b << 16), 0, LWA_COLORKEY);
    }
}

/// MP4 through Media Foundation, GIF through the streaming encoder.
pub struct DefaultSinkFactory;

impl SinkFactory for DefaultSinkFactory {
    fn create(
        &self,
        kind: &SinkKind,
        path: &Path,
        size: PixelSize,
        fps: u32,
    ) -> crate::Result<Box<dyn FrameSink>> {
        Ok(match kind {
            SinkKind::Mp4 => Box::new(Mp4Sink::new(path, size, fps)?),
            SinkKind::Gif { max_long_side } => {
                Box::new(GifSink::create(path, size, *max_long_side, fps)?)
            }
        })
    }
}

/// Keeps a window out of screen captures (Windows 10 2004+; ignored before).
pub fn exclude_from_capture(hwnd: isize) {
    // SAFETY: `hwnd` is a live top-level window owned by this process.
    unsafe {
        let _ = SetWindowDisplayAffinity(HWND(hwnd as *mut _), WDA_EXCLUDEFROMCAPTURE);
    }
}
