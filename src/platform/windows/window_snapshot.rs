use super::monitors::rect_from;
use crate::capture::{WindowCandidate, WindowSnapshotter};
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed,
};
use windows::core::BOOL;

/// Visible top-level windows in z-order, excluding one process (ours).
pub struct Win32WindowList {
    pub exclude_pid: u32,
}

struct Collect {
    exclude_pid: u32,
    out: Vec<WindowCandidate>,
}

unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the `&mut Collect` owned by `snapshot`, alive during EnumWindows.
    let state = unsafe { &mut *(data.0 as *mut Collect) };
    // SAFETY: plain queries on a window handle supplied by EnumWindows.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut cloaked = 0u32;
        let cloaked_ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked_ok.is_ok() && cloaked != 0 {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == state.exclude_pid {
            return BOOL(1);
        }
        let mut rect = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_err()
        {
            return BOOL(1);
        }
        let bounds = rect_from(rect);
        if bounds.is_empty() {
            return BOOL(1);
        }
        let mut title = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        state.out.push(WindowCandidate {
            id: hwnd.0 as u64,
            pid,
            title: String::from_utf16_lossy(&title[..len]),
            bounds,
            maximized: IsZoomed(hwnd).as_bool(),
        });
    }
    BOOL(1)
}

impl WindowSnapshotter for Win32WindowList {
    fn snapshot(&self) -> Vec<WindowCandidate> {
        let mut state = Collect { exclude_pid: self.exclude_pid, out: Vec::new() };
        // SAFETY: the callback only touches `state` through the LPARAM during this call.
        unsafe {
            let _ = EnumWindows(Some(collect_window), LPARAM(&mut state as *mut _ as isize));
        }
        state.out
    }
}
