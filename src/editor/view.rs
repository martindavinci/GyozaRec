//! Where the image sits on the editor's canvas.

use super::Point;
use crate::geometry::PixelRect;
use crate::uikit::painter::RectF;

/// Maps image pixels to canvas units: `canvas = origin + image × scale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub scale: f32,
    pub origin: Point,
}

impl View {
    /// One image pixel per canvas unit, image at (0, 0): for the final picture.
    pub const IDENTITY: View = View { scale: 1.0, origin: (0.0, 0.0) };

    /// `visible` fitted and centered in `canvas`, never enlarged.
    pub fn fit(visible: PixelRect, canvas: RectF) -> View {
        let (w, h) = (visible.width.max(1) as f32, visible.height.max(1) as f32);
        let scale = (canvas.w / w).min(canvas.h / h).clamp(f32::MIN_POSITIVE, 1.0);
        let left = canvas.x + (canvas.w - w * scale) / 2.0;
        let top = canvas.y + (canvas.h - h * scale) / 2.0;
        View { scale, origin: (left - visible.x as f32 * scale, top - visible.y as f32 * scale) }
    }

    pub fn to_canvas(&self, p: Point) -> Point {
        (self.origin.0 + p.0 * self.scale, self.origin.1 + p.1 * self.scale)
    }

    pub fn to_image(&self, p: Point) -> Point {
        ((p.0 - self.origin.0) / self.scale, (p.1 - self.origin.1) / self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_of_a_tiny_image_is_centered_at_full_size() {
        let v =
            View::fit(PixelRect::new(0, 0, 12, 12), RectF { x: 0.0, y: 0.0, w: 400.0, h: 300.0 });
        assert_eq!(v, View { scale: 1.0, origin: (194.0, 144.0) });
    }

    #[test]
    fn view_fits_a_large_image_and_maps_back() {
        let canvas = RectF { x: 0.0, y: 40.0, w: 800.0, h: 600.0 };
        let v = View::fit(PixelRect::new(0, 0, 4000, 2000), canvas);
        assert!((v.scale - 0.2).abs() < 1e-6);
        assert_eq!(v.to_canvas((0.0, 0.0)), (0.0, 140.0));
        let p = (1234.5, 678.25);
        let back = v.to_image(v.to_canvas(p));
        assert!((back.0 - p.0).abs() < 0.01 && (back.1 - p.1).abs() < 0.01, "{back:?}");
    }

    #[test]
    fn cropped_view_shows_the_crop_at_the_canvas_corner() {
        let v = View::fit(
            PixelRect::new(100, 50, 400, 300),
            RectF { x: 0.0, y: 0.0, w: 400.0, h: 300.0 },
        );
        assert_eq!(v.to_canvas((100.0, 50.0)), (0.0, 0.0));
    }

    #[test]
    fn empty_canvas_does_not_divide_by_zero() {
        let v = View::fit(PixelRect::new(0, 0, 10, 10), RectF { x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        assert!(v.scale > 0.0 && v.to_image((1.0, 1.0)).0.is_finite());
    }
}
