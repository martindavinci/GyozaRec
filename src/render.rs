//! Selection overlay rendering into softbuffer pixels (`0x00RRGGBB`).

use crate::geometry::{PixelRect, PixelSize};
use crate::pixels::PixelBuffer;

const RING_INNER: u32 = 0x00FF_FFFF;
const RING_OUTER: u32 = 0x0020_2020;

/// Frozen screenshot of one monitor, pre-converted so each redraw is a few
/// row copies: the dimmed image, the selection from the original, two rings.
pub struct OverlayCache {
    size: PixelSize,
    original: Vec<u32>,
    dimmed: Vec<u32>,
}

impl OverlayCache {
    /// `frozen` must already be cropped to the monitor, with origin 0,0.
    pub fn new(frozen: &PixelBuffer) -> Self {
        let original: Vec<u32> = frozen
            .bgra
            .chunks_exact(4)
            .map(|px| u32::from(px[2]) << 16 | u32::from(px[1]) << 8 | u32::from(px[0]))
            .collect();
        let dim = |c: u32| c * 153 / 255;
        let dimmed = original
            .iter()
            .map(|&p| dim(p >> 16 & 0xFF) << 16 | dim(p >> 8 & 0xFF) << 8 | dim(p & 0xFF))
            .collect();
        Self { size: frozen.bounds.size(), original, dimmed }
    }

    pub fn size(&self) -> PixelSize {
        self.size
    }

    /// `out` holds `width × height` pixels; `selection` is in local pixels.
    pub fn render(&self, selection: Option<PixelRect>, out: &mut [u32]) {
        out.copy_from_slice(&self.dimmed);
        let Some(sel) = selection else { return };
        let full = PixelRect::new(0, 0, self.size.width, self.size.height);
        let inside = sel.intersect(&full);
        let w = self.size.width as usize;
        for y in inside.y..inside.bottom() {
            let start = y as usize * w + inside.x as usize;
            let end = start + inside.width as usize;
            out[start..end].copy_from_slice(&self.original[start..end]);
        }
        self.ring(
            out,
            &PixelRect::new(sel.x - 1, sel.y - 1, sel.width + 2, sel.height + 2),
            RING_INNER,
        );
        self.ring(
            out,
            &PixelRect::new(sel.x - 2, sel.y - 2, sel.width + 4, sel.height + 4),
            RING_OUTER,
        );
    }

    /// Draws the 1-pixel border of `r`, clipped to the surface.
    fn ring(&self, out: &mut [u32], r: &PixelRect, color: u32) {
        let (w, h) = (self.size.width, self.size.height);
        let mut put = |x: i32, y: i32| {
            if x >= 0 && x < w && y >= 0 && y < h {
                out[(y * w + x) as usize] = color;
            }
        };
        for x in r.x..r.right() {
            put(x, r.y);
            put(x, r.bottom() - 1);
        }
        for y in r.y..r.bottom() {
            put(r.x, y);
            put(r.right() - 1, y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelRect;
    use crate::pixels::PixelBuffer;

    fn at(out: &[u32], w: i32, x: i32, y: i32) -> u32 {
        out[(y * w + x) as usize]
    }

    // b,g,r = 200,150,100 → rgb(100,150,200)
    fn cache() -> OverlayCache {
        OverlayCache::new(&PixelBuffer::solid(PixelRect::new(0, 0, 8, 8), 200, 150, 100))
    }

    #[test]
    fn dims_outside_and_keeps_inside() {
        let mut out = vec![0; 64];
        cache().render(Some(PixelRect::new(3, 3, 2, 2)), &mut out);
        assert_eq!(at(&out, 8, 0, 0), 0x003C5A78);
        assert_eq!(at(&out, 8, 3, 3), 0x006496C8);
        assert_eq!(at(&out, 8, 2, 2), 0x00FFFFFF);
        assert_eq!(at(&out, 8, 1, 1), 0x00202020);
    }

    #[test]
    fn no_selection_dims_everything() {
        let mut out = vec![0; 64];
        cache().render(None, &mut out);
        assert!(out.iter().all(|&p| p == 0x003C5A78));
    }

    #[test]
    fn selection_at_edge_does_not_panic() {
        let mut out = vec![0; 64];
        cache().render(Some(PixelRect::new(0, 0, 8, 8)), &mut out);
        assert_eq!(at(&out, 8, 0, 0), 0x006496C8);
    }
}
