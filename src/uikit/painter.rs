//! Antialiased drawing and text on a pixmap, in logical units × scale.

use super::theme::Color;
use crate::geometry::{PixelPoint, PixelSize};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont, point};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, PremultipliedColorU8, Transform};

const NUNITO: &[u8] = include_bytes!("../../assets/fonts/Nunito-Variable.ttf");

/// Rectangle in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

pub struct Fonts {
    pub regular: FontVec,
    pub bold: FontVec,
}

impl Fonts {
    /// Nunito (variable font) at weights 400 and 700.
    pub fn embedded() -> Fonts {
        let instance = |weight: f32| {
            let mut font = FontVec::try_from_vec(NUNITO.to_vec()).expect("embedded font");
            font.set_variation(b"wght", weight);
            font
        };
        Fonts { regular: instance(400.0), bold: instance(700.0) }
    }

    fn pick(&self, bold: bool) -> &FontVec {
        if bold { &self.bold } else { &self.regular }
    }
}

/// Width and line height of `text` in logical units.
pub fn measure_text(fonts: &Fonts, text: &str, px: f32, bold: bool) -> (f32, f32) {
    let font = fonts.pick(bold).as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = font.glyph_id(c);
        if let Some(p) = prev {
            width += font.kern(p, id);
        }
        width += font.h_advance(id);
        prev = Some(id);
    }
    (width, font.ascent() - font.descent())
}

pub struct Painter {
    pixmap: Pixmap,
    scale: f32,
}

impl Painter {
    pub fn new(size: PixelSize, scale: f32) -> Painter {
        let pixmap = Pixmap::new(size.width.max(1) as u32, size.height.max(1) as u32)
            .expect("non-empty pixmap");
        Painter { pixmap, scale }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn clear(&mut self, color: Color) {
        let (r, g, b) = color.rgb();
        self.pixmap.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
    }

    fn fill_path(&mut self, path: Option<Path>, color: Color) {
        let Some(path) = path else { return };
        let (r, g, b) = color.rgb();
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, 255);
        paint.anti_alias = true;
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    pub fn fill_rounded_rect(&mut self, rect: RectF, radius: f32, color: Color) {
        let s = self.scale;
        let (x, y, w, h) = (rect.x * s, rect.y * s, rect.w * s, rect.h * s);
        let r = (radius * s).min(w / 2.0).min(h / 2.0).max(0.0);
        // Cubic Bézier approximation of quarter circles.
        let k = r * 0.552_284_8;
        let mut pb = PathBuilder::new();
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
        pb.line_to(x, y + r);
        pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
        pb.close();
        self.fill_path(pb.finish(), color);
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let s = self.scale;
        self.fill_path(PathBuilder::from_circle(cx * s, cy * s, r * s), color);
    }

    pub fn fill_triangle(&mut self, points: [(f32, f32); 3], color: Color) {
        let s = self.scale;
        let mut pb = PathBuilder::new();
        pb.move_to(points[0].0 * s, points[0].1 * s);
        pb.line_to(points[1].0 * s, points[1].1 * s);
        pb.line_to(points[2].0 * s, points[2].1 * s);
        pb.close();
        self.fill_path(pb.finish(), color);
    }

    /// Draws `text` with its top-left corner at (`x`, `y`).
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        fonts: &Fonts,
        text: &str,
        x: f32,
        y: f32,
        px: f32,
        color: Color,
        bold: bool,
    ) {
        let s = self.scale;
        let font = fonts.pick(bold);
        let scaled = font.as_scaled(PxScale::from(px * s));
        let baseline = y * s + scaled.ascent();
        let mut caret = x * s;
        let mut prev = None;
        let (cr, cg, cb) = color.rgb();
        let (width, height) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                caret += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(px * s, point(caret, baseline));
            caret += scaled.h_advance(id);
            prev = Some(id);
            let Some(outline) = font.outline_glyph(glyph) else { continue };
            let bounds = outline.px_bounds();
            let pixels = self.pixmap.pixels_mut();
            outline.draw(|gx, gy, coverage| {
                let px_x = bounds.min.x as i32 + gx as i32;
                let px_y = bounds.min.y as i32 + gy as i32;
                if px_x < 0 || px_y < 0 || px_x >= width || px_y >= height {
                    return;
                }
                let dst = &mut pixels[(px_y * width + px_x) as usize];
                let a = coverage.clamp(0.0, 1.0);
                let mix = |src: u8, d: u8| (f32::from(src) * a + f32::from(d) * (1.0 - a)) as u8;
                if let Some(c) = PremultipliedColorU8::from_rgba(
                    mix(cr, dst.red()),
                    mix(cg, dst.green()),
                    mix(cb, dst.blue()),
                    255,
                ) {
                    *dst = c;
                }
            });
        }
    }

    /// Copies the pixmap into `dst` (row width `dst_width`) at `at`, clipped.
    pub fn blit_into(&self, dst: &mut [u32], dst_width: i32, at: PixelPoint) {
        let dst_height = dst.len() as i32 / dst_width.max(1);
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        for y in 0..h {
            let ty = at.y + y;
            if ty < 0 || ty >= dst_height {
                continue;
            }
            for x in 0..w {
                let tx = at.x + x;
                if tx < 0 || tx >= dst_width {
                    continue;
                }
                let p = self.pixmap.pixels()[(y * w + x) as usize];
                dst[(ty * dst_width + tx) as usize] = pack(p);
            }
        }
    }

    pub fn to_softbuffer(&self, out: &mut [u32]) {
        for (dst, p) in out.iter_mut().zip(self.pixmap.pixels()) {
            *dst = pack(*p);
        }
    }
}

fn pack(p: PremultipliedColorU8) -> u32 {
    u32::from(p.red()) << 16 | u32::from(p.green()) << 8 | u32::from(p.blue())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelSize;
    use crate::uikit::theme::GYOZA;

    #[test]
    fn measure_text_is_positive_and_bold_not_narrower() {
        let f = Fonts::embedded();
        let (w, h) = measure_text(&f, "Cattura", 14.0, false);
        assert!(w > 20.0 && h > 10.0, "{w}x{h}");
        assert!(measure_text(&f, "Cattura", 14.0, true).0 >= w);
    }

    #[test]
    fn clear_and_rounded_rect_paint_pixels() {
        let mut p = Painter::new(PixelSize { width: 40, height: 20 }, 2.0);
        p.clear(GYOZA.bg);
        p.fill_rounded_rect(RectF { x: 2.0, y: 2.0, w: 16.0, h: 6.0 }, 2.0, GYOZA.accent);
        let mut out = vec![0; 800];
        p.to_softbuffer(&mut out);
        assert_eq!(out[0], GYOZA.bg.0);
        assert_eq!(out[10 * 40 + 20], GYOZA.accent.0); // logical (10,5) × scale 2
    }

    #[test]
    fn text_draws_non_background_pixels() {
        let f = Fonts::embedded();
        let mut p = Painter::new(PixelSize { width: 100, height: 30 }, 1.0);
        p.clear(GYOZA.bg);
        p.text(&f, "Cattura", 4.0, 4.0, 14.0, GYOZA.text, false);
        let mut out = vec![0; 3000];
        p.to_softbuffer(&mut out);
        assert!(out.iter().any(|&c| c != GYOZA.bg.0));
    }

    #[test]
    fn blit_copies_into_destination_with_clipping() {
        let mut p = Painter::new(PixelSize { width: 4, height: 4 }, 1.0);
        p.clear(GYOZA.accent);
        let mut dst = vec![0u32; 6 * 6];
        p.blit_into(&mut dst, 6, crate::geometry::PixelPoint { x: 4, y: 4 });
        assert_eq!(dst[4 * 6 + 4], GYOZA.accent.0);
        assert_eq!(dst[5 * 6 + 5], GYOZA.accent.0);
        assert_eq!(dst[3 * 6 + 3], 0);
    }
}
