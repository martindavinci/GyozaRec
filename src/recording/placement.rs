//! Where the recording panel and the region frame go on screen.

use crate::geometry::{PixelPoint, PixelRect, PixelSize};

const GAP: i32 = 8;

/// Right-aligned with the region: below it, else above, else inside at the
/// bottom; always kept within the monitor horizontally.
pub fn place_panel(region: PixelRect, panel: PixelSize, monitor: PixelRect) -> PixelPoint {
    let max_x = (monitor.right() - panel.width).max(monitor.x);
    let x = (region.right() - panel.width).clamp(monitor.x, max_x);
    let below = region.bottom() + GAP;
    let above = region.y - GAP - panel.height;
    let y = if below + panel.height <= monitor.bottom() {
        below
    } else if above >= monitor.y {
        above
    } else {
        // Inside the region, but never over the taskbar.
        region.bottom().min(monitor.bottom()) - GAP - panel.height
    };
    PixelPoint { x, y }
}

/// Window that frames `region` with a border of `thickness` just outside it.
pub fn frame_bounds(region: PixelRect, thickness: i32) -> PixelRect {
    let t = thickness;
    PixelRect::new(region.x - t, region.y - t, region.width + 2 * t, region.height + 2 * t)
}

/// Border pixels in `border`, everything inside in the transparent `key` color.
pub fn render_frame(size: PixelSize, thickness: i32, border: u32, key: u32, out: &mut [u32]) {
    let (w, h, t) = (size.width, size.height, thickness);
    for y in 0..h {
        for x in 0..w {
            let edge = x < t || y < t || x >= w - t || y >= h - t;
            out[(y * w + x) as usize] = if edge { border } else { key };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PixelPoint, PixelRect, PixelSize};

    const MON: PixelRect = PixelRect { x: 0, y: 0, width: 1920, height: 1080 };
    const PANEL: PixelSize = PixelSize { width: 220, height: 40 };

    #[test]
    fn below_and_right_aligned_when_room() {
        assert_eq!(
            place_panel(PixelRect::new(100, 100, 400, 300), PANEL, MON),
            PixelPoint { x: 280, y: 408 }
        );
    }

    #[test]
    fn above_when_no_room_below() {
        assert_eq!(
            place_panel(PixelRect::new(100, 900, 400, 170), PANEL, MON),
            PixelPoint { x: 280, y: 852 }
        );
    }

    #[test]
    fn inside_bottom_when_region_fills_monitor() {
        assert_eq!(place_panel(MON, PANEL, MON), PixelPoint { x: 1700, y: 1032 });
    }

    #[test]
    fn full_screen_region_keeps_the_panel_above_the_taskbar() {
        let work = PixelRect::new(0, 0, 1920, 1040);
        assert_eq!(place_panel(MON, PANEL, work), PixelPoint { x: 1700, y: 992 });
    }

    #[test]
    fn x_is_clamped_to_monitor() {
        assert_eq!(place_panel(PixelRect::new(0, 100, 100, 100), PANEL, MON).x, 0);
    }

    #[test]
    fn frame_bounds_surround_region_outside() {
        assert_eq!(
            frame_bounds(PixelRect::new(100, 100, 50, 40), 2),
            PixelRect::new(98, 98, 54, 44)
        );
    }

    #[test]
    fn frame_has_border_and_keyed_interior() {
        let size = PixelSize { width: 8, height: 6 };
        let mut out = vec![0u32; 48];
        render_frame(size, 2, 0x00E0533C, 0x00FF00FF, &mut out);
        let at = |x: usize, y: usize| out[y * 8 + x];
        assert_eq!(
            (at(0, 0), at(1, 1), at(7, 5), at(6, 3)),
            (0xE0533C, 0xE0533C, 0xE0533C, 0xE0533C)
        );
        assert_eq!((at(2, 2), at(5, 3)), (0xFF00FF, 0xFF00FF));
    }
}
