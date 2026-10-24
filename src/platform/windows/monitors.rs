use crate::capture::{CursorLocator, MonitorProvider};
use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::BOOL;

pub(crate) fn rect_from(r: RECT) -> PixelRect {
    PixelRect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}

pub struct Win32Monitors;

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `data` is the `&mut Vec<MonitorInfo>` passed by `monitors` below,
    // alive for the whole synchronous enumeration.
    let out = unsafe { &mut *(data.0 as *mut Vec<MonitorInfo>) };
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` is a properly sized MONITORINFOEXW whose header is MONITORINFO.
    let ok = unsafe { GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO) };
    if ok.as_bool() {
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        // SAFETY: valid monitor handle and out pointers.
        let dpi = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
            .map(|_| dpi_x)
            .unwrap_or(96);
        let name_len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        out.push(MonitorInfo {
            device_name: String::from_utf16_lossy(&info.szDevice[..name_len]),
            bounds: rect_from(info.monitorInfo.rcMonitor),
            scale: f64::from(dpi) / 96.0,
        });
    }
    BOOL(1)
}

impl MonitorProvider for Win32Monitors {
    fn monitors(&self) -> Vec<MonitorInfo> {
        let mut out: Vec<MonitorInfo> = Vec::new();
        // SAFETY: the callback only touches `out` through the LPARAM during this call.
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(collect_monitor),
                LPARAM(&mut out as *mut _ as isize),
            );
        }
        out
    }

    fn virtual_screen(&self) -> PixelRect {
        let ms = self.monitors();
        let Some(first) = ms.first() else { return PixelRect::default() };
        ms.iter().skip(1).fold(first.bounds, |acc, m| {
            let x = acc.x.min(m.bounds.x);
            let y = acc.y.min(m.bounds.y);
            let right = acc.right().max(m.bounds.right());
            let bottom = acc.bottom().max(m.bounds.bottom());
            PixelRect::new(x, y, right - x, bottom - y)
        })
    }
}

pub struct Win32Cursor;

impl CursorLocator for Win32Cursor {
    fn position(&self) -> PixelPoint {
        let mut p = POINT::default();
        // SAFETY: valid out pointer.
        let _ = unsafe { GetCursorPos(&mut p) };
        PixelPoint { x: p.x, y: p.y }
    }
}
