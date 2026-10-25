//! The small notification shown after a capture: what it carries, where it
//! goes and when it leaves.

use crate::geometry::{PixelRect, PixelSize, fit};
use crate::pixels::PixelBuffer;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Image,
    Video,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub path: PathBuf,
    pub kind: NoticeKind,
    pub thumbnail: PixelBuffer,
    /// Work area and DPI scale of the monitor the capture came from.
    pub work_area: PixelRect,
    pub scale: f64,
}

pub const NOTICE_TIMEOUT: Duration = Duration::from_secs(6);
pub const THUMBNAIL_MAX: PixelSize = PixelSize { width: 320, height: 180 };
/// Gap between the notification and the screen edges, in logical pixels.
const EDGE: f64 = 16.0;

/// `img` scaled down to fit `THUMBNAIL_MAX`; small images keep their size.
pub fn thumbnail(img: &PixelBuffer) -> PixelBuffer {
    let size = img.bounds.size();
    let ratio = (f64::from(THUMBNAIL_MAX.width) / f64::from(size.width.max(1)))
        .min(f64::from(THUMBNAIL_MAX.height) / f64::from(size.height.max(1)));
    if ratio >= 1.0 {
        return img.clone();
    }
    let long = (f64::from(size.width.max(size.height)) * ratio).round() as i32;
    img.resized(fit(size, long.max(1)))
}

/// Physical rectangle of a notification `logical` units big, in the
/// bottom-right corner of `work_area`.
pub fn notice_rect(work_area: PixelRect, scale: f64, logical: (f32, f32)) -> PixelRect {
    let w = (f64::from(logical.0) * scale).round() as i32;
    let h = (f64::from(logical.1) * scale).round() as i32;
    let edge = (EDGE * scale).round() as i32;
    PixelRect::new(work_area.right() - edge - w, work_area.bottom() - edge - h, w, h)
}

/// When the notification closes: `NOTICE_TIMEOUT` after it opened or after
/// the pointer left it; never while the pointer is over it. Each start
/// gets a new generation, so timeouts scheduled earlier are ignored.
#[derive(Default)]
pub struct NoticeTimer {
    generation: u64,
    hovered: bool,
}

impl NoticeTimer {
    /// A new notification: returns the generation its timeout must carry.
    pub fn start(&mut self) -> u64 {
        self.generation += 1;
        self.hovered = false;
        self.generation
    }

    /// The pointer entered or left; leaving restarts the countdown and
    /// returns the generation to schedule.
    pub fn hover(&mut self, inside: bool) -> Option<u64> {
        let left = self.hovered && !inside;
        self.hovered = inside;
        if left {
            self.generation += 1;
            return Some(self.generation);
        }
        None
    }

    pub fn expired(&self, generation: u64) -> bool {
        generation == self.generation && !self.hovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_sits_in_the_bottom_right_of_a_left_monitor() {
        let r = notice_rect(PixelRect::new(-1920, 0, 1920, 1040), 1.5, (280.0, 200.0));
        assert_eq!(r, PixelRect::new(-444, 716, 420, 300));
    }

    #[test]
    fn thumbnail_fits_and_never_enlarges() {
        let wide = PixelBuffer::solid(PixelRect::new(0, 0, 3840, 1080), 1, 2, 3);
        assert_eq!(thumbnail(&wide).bounds.size(), PixelSize { width: 320, height: 90 });
        let tall = PixelBuffer::solid(PixelRect::new(0, 0, 400, 1000), 1, 2, 3);
        assert_eq!(thumbnail(&tall).bounds.size(), PixelSize { width: 72, height: 180 });
        let small = PixelBuffer::solid(PixelRect::new(5, 5, 100, 50), 1, 2, 3);
        assert_eq!(thumbnail(&small).bounds.size(), PixelSize { width: 100, height: 50 });
    }

    #[test]
    fn stale_timeouts_are_ignored() {
        let mut t = NoticeTimer::default();
        let first = t.start();
        let second = t.start();
        assert!(!t.expired(first));
        assert!(t.expired(second));
    }

    #[test]
    fn hovering_holds_the_notice_and_leaving_restarts_it() {
        let mut t = NoticeTimer::default();
        let g = t.start();
        assert_eq!(t.hover(true), None);
        assert!(!t.expired(g), "held while the pointer is over it");
        let restarted = t.hover(false).expect("leaving restarts");
        assert!(!t.expired(g) && t.expired(restarted));
        assert_eq!(t.hover(false), None, "moving outside changes nothing");
    }
}
