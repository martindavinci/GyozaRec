//! Capture abstractions implemented by each platform backend.

use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};
use crate::pixels::PixelBuffer;

pub trait MonitorProvider: Send {
    fn monitors(&self) -> Vec<MonitorInfo>;
    /// Union of all monitor bounds.
    fn virtual_screen(&self) -> PixelRect;
}

pub trait ScreenGrabber: Send + Sync {
    fn grab(&self, rect: PixelRect, include_cursor: bool) -> crate::Result<PixelBuffer>;
}

pub trait CursorLocator: Send {
    fn position(&self) -> PixelPoint;
}

/// A top-level window that Window mode can pick.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowCandidate {
    pub id: u64,
    pub pid: u32,
    pub title: String,
    pub bounds: PixelRect,
}

pub trait WindowSnapshotter: Send {
    /// Visible top-level windows, topmost first.
    fn snapshot(&self) -> Vec<WindowCandidate>;
}

/// The monitor containing `p`, or the one whose center is nearest.
pub fn monitor_from_point(monitors: &[MonitorInfo], p: PixelPoint) -> &MonitorInfo {
    monitors.iter().find(|m| m.bounds.contains(p)).unwrap_or_else(|| {
        monitors
            .iter()
            .min_by_key(|m| {
                let c = m.bounds.center();
                let (dx, dy) = (i64::from(c.x - p.x), i64::from(c.y - p.y));
                dx * dx + dy * dy
            })
            .expect("at least one monitor")
    })
}

/// Topmost window containing `p`.
pub fn window_at(z_ordered: &[WindowCandidate], p: PixelPoint) -> Option<&WindowCandidate> {
    z_ordered.iter().find(|w| w.bounds.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};

    fn w(id: u64, x: i32, y: i32, wd: i32, h: i32) -> WindowCandidate {
        WindowCandidate { id, pid: 1, title: format!("w{id}"), bounds: PixelRect::new(x, y, wd, h) }
    }

    #[test]
    fn topmost_window_wins_on_overlap() {
        let ws = [w(1, 0, 0, 100, 100), w(2, 50, 50, 100, 100)];
        assert_eq!(window_at(&ws, PixelPoint { x: 60, y: 60 }).unwrap().id, 1);
    }

    #[test]
    fn point_outside_all_windows_returns_none() {
        assert!(window_at(&[w(1, 0, 0, 10, 10)], PixelPoint { x: 500, y: 500 }).is_none());
    }

    #[test]
    fn monitor_from_point_picks_negative_origin() {
        let ms = [
            MonitorInfo {
                device_name: "A".into(),
                bounds: PixelRect::new(0, 0, 1920, 1080),
                scale: 1.0,
            },
            MonitorInfo {
                device_name: "B".into(),
                bounds: PixelRect::new(-1920, 0, 1920, 1080),
                scale: 1.5,
            },
        ];
        assert_eq!(monitor_from_point(&ms, PixelPoint { x: -10, y: 500 }).device_name, "B");
    }
}
