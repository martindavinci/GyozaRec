//! Physical-pixel geometry shared by capture, rendering and recording.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelSize {
    pub width: i32,
    pub height: i32,
}

/// Rectangle in physical pixels of the virtual screen (or of a local surface).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// Half-open: the right and bottom edges are outside.
    pub fn contains(&self, p: PixelPoint) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn intersect(&self, other: &PixelRect) -> PixelRect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return PixelRect::default();
        }
        PixelRect::new(x, y, right - x, bottom - y)
    }

    pub fn center(&self) -> PixelPoint {
        PixelPoint { x: self.x + self.width / 2, y: self.y + self.height / 2 }
    }

    pub fn offset(&self, dx: i32, dy: i32) -> PixelRect {
        PixelRect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    pub fn size(&self) -> PixelSize {
        PixelSize { width: self.width, height: self.height }
    }

    /// Rectangle spanned by two drag points, in any order.
    pub fn from_points(a: PixelPoint, b: PixelPoint) -> PixelRect {
        PixelRect::new(a.x.min(b.x), a.y.min(b.y), (b.x - a.x).abs(), (b.y - a.y).abs())
    }

    pub fn clamp_to(&self, bounds: &PixelRect) -> PixelRect {
        self.intersect(bounds)
    }

    /// Rounds width and height down to even numbers (H.264 requirement).
    pub fn even_floor(&self) -> PixelRect {
        PixelRect::new(self.x, self.y, self.width & !1, self.height & !1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    pub device_name: String,
    pub bounds: PixelRect,
    /// `bounds` without the taskbar and docked toolbars.
    pub work_area: PixelRect,
    /// DPI / 96.
    pub scale: f64,
}

/// `screen` relative to the top-left corner of `monitor`.
pub fn local_point(screen: PixelPoint, monitor: PixelRect) -> PixelPoint {
    PixelPoint { x: screen.x - monitor.x, y: screen.y - monitor.y }
}

/// What part of the screen a capture takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureArea {
    #[default]
    Rectangle,
    /// A free-hand outline; pictures only.
    Lasso,
    Window,
    FullScreen,
    /// Every monitor at once, as one picture or video.
    AllMonitors,
}

impl CaptureArea {
    /// The lasso is for pictures: recordings take a rectangle instead.
    pub fn for_format(self, format: OutputFormat) -> CaptureArea {
        if self == CaptureArea::Lasso && format.is_recording() {
            CaptureArea::Rectangle
        } else {
            self
        }
    }

    /// Whether menus offer this area for `format`.
    pub fn offered_for(self, format: OutputFormat) -> bool {
        self.for_format(format) == self
    }
}

/// What a capture produces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    #[default]
    Png,
    Jpg,
    Mp4,
    Gif,
}

impl OutputFormat {
    pub fn is_recording(self) -> bool {
        matches!(self, OutputFormat::Mp4 | OutputFormat::Gif)
    }
}

pub fn minimum_size(format: OutputFormat) -> PixelSize {
    let (width, height) = match format {
        OutputFormat::Png | OutputFormat::Jpg => (2, 2),
        OutputFormat::Mp4 => (64, 64),
        OutputFormat::Gif => (16, 16),
    };
    PixelSize { width, height }
}

pub fn meets_minimum(format: OutputFormat, rect: &PixelRect) -> bool {
    let min = minimum_size(format);
    rect.width >= min.width && rect.height >= min.height
}

/// Largest frame the H.264 encoder takes (level 5.1).
const ENCODER_MAX: PixelSize = PixelSize { width: 4096, height: 2304 };

/// The video size for a `region`: scaled down to fit the encoder's limit,
/// aspect kept, both sides even. Never upscales.
pub fn encoder_size(region: PixelSize) -> PixelSize {
    let ratio = (f64::from(ENCODER_MAX.width) / f64::from(region.width.max(1)))
        .min(f64::from(ENCODER_MAX.height) / f64::from(region.height.max(1)))
        .min(1.0);
    let side = |v: i32| (((f64::from(v) * ratio) as i32) & !1).max(2);
    PixelSize { width: side(region.width), height: side(region.height) }
}

/// Scales `size` down so its long side is at most `max_long_side`, keeping the
/// aspect ratio. Never upscales; each side is at least 1.
pub fn fit(size: PixelSize, max_long_side: i32) -> PixelSize {
    let long = size.width.max(size.height);
    if long <= max_long_side || long <= 0 {
        return size;
    }
    let ratio = f64::from(max_long_side) / f64::from(long);
    let scale = |v: i32| ((f64::from(v) * ratio).round() as i32).max(1);
    PixelSize { width: scale(size.width), height: scale(size.height) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: i32, y: i32, w: i32, h: i32) -> PixelRect {
        PixelRect::new(x, y, w, h)
    }
    fn p(x: i32, y: i32) -> PixelPoint {
        PixelPoint { x, y }
    }
    fn s(width: i32, height: i32) -> PixelSize {
        PixelSize { width, height }
    }

    #[test]
    fn from_points_normalizes_reversed_drag() {
        assert_eq!(PixelRect::from_points(p(40, 60), p(10, 20)), r(10, 20, 30, 40));
    }
    #[test]
    fn intersect_returns_empty_when_disjoint() {
        assert!(r(0, 0, 10, 10).intersect(&r(20, 20, 5, 5)).is_empty());
    }
    #[test]
    fn contains_is_half_open() {
        assert!(r(0, 0, 10, 10).contains(p(9, 9)));
        assert!(!r(0, 0, 10, 10).contains(p(10, 0)));
    }
    #[test]
    fn clamp_keeps_selection_inside_monitor() {
        assert_eq!(r(-10, -10, 60, 60).clamp_to(&r(0, 0, 100, 100)), r(0, 0, 50, 50));
    }
    #[test]
    fn local_point_subtracts_monitor_origin() {
        let p = local_point(PixelPoint { x: -1500, y: 300 }, r(-1920, 0, 1920, 1080));
        assert_eq!(p, PixelPoint { x: 420, y: 300 });
    }

    #[test]
    fn even_floor_rounds_size_down() {
        assert_eq!(r(5, 5, 101, 65).even_floor(), r(5, 5, 100, 64));
    }
    #[test]
    fn offset_moves_rect() {
        assert_eq!(r(-1500, 100, 640, 480).offset(1920, 0), r(420, 100, 640, 480));
    }

    #[test]
    fn meets_minimum_per_format() {
        let cases = [
            (OutputFormat::Png, 1, 1, false),
            (OutputFormat::Png, 2, 2, true),
            (OutputFormat::Mp4, 63, 200, false),
            (OutputFormat::Mp4, 64, 64, true),
            (OutputFormat::Gif, 16, 15, false),
            (OutputFormat::Gif, 16, 16, true),
        ];
        for (format, w, h, ok) in cases {
            assert_eq!(meets_minimum(format, &r(0, 0, w, h)), ok, "{format:?} {w}x{h}");
        }
    }

    #[test]
    fn encoder_size_fits_h264_limits() {
        assert_eq!(encoder_size(s(5120, 1440)), s(4096, 1152));
        assert_eq!(encoder_size(s(1920, 1080)), s(1920, 1080));
        assert_eq!(encoder_size(s(4096, 2304)), s(4096, 2304));
        assert_eq!(encoder_size(s(3000, 5000)), s(1382, 2304));
    }

    #[test]
    fn fit_keeps_aspect_and_never_upscales() {
        assert_eq!(fit(s(1600, 900), 800), s(800, 450));
        assert_eq!(fit(s(900, 1600), 800), s(450, 800));
        assert_eq!(fit(s(400, 300), 800), s(400, 300));
        assert_eq!(fit(s(5000, 3), 800), s(800, 1));
    }
}
