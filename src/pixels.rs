//! BGRA pixel buffers positioned on the virtual screen.

use crate::geometry::{PixelPoint, PixelRect, PixelSize};
use image::{ImageBuffer, Rgba, imageops};

/// Top-down BGRA pixels, stride = width × 4, not premultiplied. Alpha is 255
/// except where a lasso left the picture transparent. `bounds` places the
/// buffer on the virtual screen, so crops use screen coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelBuffer {
    pub bounds: PixelRect,
    pub bgra: Vec<u8>,
}

impl PixelBuffer {
    pub fn new(bounds: PixelRect, bgra: Vec<u8>) -> Self {
        assert_eq!(
            bgra.len(),
            bounds.width.max(0) as usize * bounds.height.max(0) as usize * 4,
            "pixel data does not match {bounds:?}"
        );
        Self { bounds, bgra }
    }

    pub fn solid(bounds: PixelRect, b: u8, g: u8, r: u8) -> Self {
        let count = bounds.width.max(0) as usize * bounds.height.max(0) as usize;
        Self::new(bounds, [b, g, r, 255].repeat(count))
    }

    pub fn width(&self) -> i32 {
        self.bounds.width
    }

    pub fn height(&self) -> i32 {
        self.bounds.height
    }

    fn offset_of(&self, p: PixelPoint) -> usize {
        assert!(self.bounds.contains(p), "{p:?} outside {:?}", self.bounds);
        let x = (p.x - self.bounds.x) as usize;
        let y = (p.y - self.bounds.y) as usize;
        (y * self.width() as usize + x) * 4
    }

    pub fn pixel(&self, p: PixelPoint) -> [u8; 4] {
        let i = self.offset_of(p);
        self.bgra[i..i + 4].try_into().unwrap()
    }

    pub fn put_pixel(&mut self, p: PixelPoint, bgra: [u8; 4]) {
        let i = self.offset_of(p);
        self.bgra[i..i + 4].copy_from_slice(&bgra);
    }

    /// Copy of the part of this buffer inside `screen_rect`.
    pub fn crop(&self, screen_rect: &PixelRect) -> PixelBuffer {
        let area = self.bounds.intersect(screen_rect);
        let row_len = area.width.max(0) as usize * 4;
        let mut out = Vec::with_capacity(row_len * area.height.max(0) as usize);
        for y in area.y..area.bottom() {
            let start = self.offset_of(PixelPoint { x: area.x, y });
            out.extend_from_slice(&self.bgra[start..start + row_len]);
        }
        PixelBuffer::new(area, out)
    }

    pub fn with_origin(self, origin: PixelPoint) -> PixelBuffer {
        let bounds = PixelRect::new(origin.x, origin.y, self.bounds.width, self.bounds.height);
        PixelBuffer { bounds, bgra: self.bgra }
    }

    pub fn resized(&self, size: PixelSize) -> PixelBuffer {
        // Resampling treats the four channels alike, so BGRA can go through
        // an RGBA buffer unchanged.
        let src: ImageBuffer<Rgba<u8>, &[u8]> =
            ImageBuffer::from_raw(self.width() as u32, self.height() as u32, &self.bgra[..])
                .expect("buffer size checked in new");
        let out = imageops::resize(
            &src,
            size.width as u32,
            size.height as u32,
            imageops::FilterType::Triangle,
        );
        let bounds = PixelRect::new(self.bounds.x, self.bounds.y, size.width, size.height);
        PixelBuffer::new(bounds, out.into_raw())
    }

    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = self.bgra.clone();
        for px in out.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        out
    }

    pub fn is_opaque(&self) -> bool {
        self.bgra.as_chunks::<4>().0.iter().all(|p| p[3] == 255)
    }

    /// Keeps what lies inside `outline` (screen pixels, closed by itself), with
    /// smooth edges. Outside becomes transparent, or white when `transparent`
    /// is false.
    pub fn cut_out(&mut self, outline: &[PixelPoint], transparent: bool) {
        const SUB: usize = 4;
        let (w, h) = (self.width().max(0) as usize, self.height().max(0) as usize);
        let (ox, oy) = (self.bounds.x, self.bounds.y);
        // Each outline point is the centre of its pixel.
        let points: Vec<(f32, f32)> =
            outline.iter().map(|p| ((p.x - ox) as f32 + 0.5, (p.y - oy) as f32 + 0.5)).collect();
        let mut cover = vec![0f32; w];
        let mut crossings = Vec::new();
        for y in 0..h {
            cover.fill(0.0);
            for sub in 0..SUB {
                let sy = y as f32 + (sub as f32 + 0.5) / SUB as f32;
                crossings.clear();
                for (i, &a) in points.iter().enumerate() {
                    let b = points[(i + 1) % points.len()];
                    if (a.1 <= sy) != (b.1 <= sy) {
                        crossings.push(a.0 + (sy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                    }
                }
                crossings.sort_by(f32::total_cmp);
                for [a, b] in crossings.as_chunks::<2>().0 {
                    let (x0, x1) = (a.clamp(0.0, w as f32), b.clamp(0.0, w as f32));
                    let first = x0.floor() as usize;
                    let last = (x1.ceil() as usize).min(w);
                    for (x, c) in cover.iter_mut().enumerate().take(last).skip(first) {
                        let part = x1.min(x as f32 + 1.0) - x0.max(x as f32);
                        *c += part.max(0.0) / SUB as f32;
                    }
                }
            }
            for (x, c) in cover.iter().enumerate() {
                let c = c.min(1.0);
                let i = (y * w + x) * 4;
                let px = &mut self.bgra[i..i + 4];
                if !transparent {
                    for ch in &mut px[..3] {
                        *ch = (f32::from(*ch) * c + 255.0 * (1.0 - c)).round() as u8;
                    }
                    px[3] = 255;
                } else if c <= 0.0 {
                    // White, for programs that ignore transparency.
                    px.copy_from_slice(&[255, 255, 255, 0]);
                } else {
                    px[3] = (c * 255.0).round() as u8;
                }
            }
        }
    }

    /// The picture laid over `back(x, y)` (BGR, local pixels), fully opaque.
    fn over(&self, back: impl Fn(usize, usize) -> [u8; 3]) -> PixelBuffer {
        let w = self.width().max(1) as usize;
        let mut out = self.bgra.clone();
        for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let a = u32::from(px[3]);
            if a < 255 {
                let bg = back(i % w, i / w);
                for (ch, b) in px[..3].iter_mut().zip(bg) {
                    *ch = ((u32::from(*ch) * a + u32::from(b) * (255 - a) + 127) / 255) as u8;
                }
                px[3] = 255;
            }
        }
        PixelBuffer { bounds: self.bounds, bgra: out }
    }

    /// Transparent parts on white.
    pub fn on_white(&self) -> PixelBuffer {
        self.over(|_, _| [255; 3])
    }

    /// Transparent parts on a grey and white checkerboard of `cell` pixels.
    pub fn on_checkerboard(&self, cell: usize) -> PixelBuffer {
        let cell = cell.max(1);
        self.over(|x, y| if (x / cell + y / cell).is_multiple_of(2) { [255; 3] } else { [0xDD; 3] })
    }

    /// JPEG has no transparency: the image is flattened onto white first.
    pub fn encode_jpeg(&self, quality: u8) -> crate::Result<Vec<u8>> {
        let flat;
        let img = if self.is_opaque() {
            self
        } else {
            flat = self.on_white();
            &flat
        };
        let rgb: Vec<u8> =
            img.bgra.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0]]).collect();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality).encode(
            &rgb,
            self.width() as u32,
            self.height() as u32,
            image::ExtendedColorType::Rgb8,
        )?;
        Ok(jpeg)
    }

    pub fn encode_png(&self) -> crate::Result<Vec<u8>> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(self.width() as u32, self.height() as u32, self.to_rgba())
                .expect("buffer size checked in new");
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        Ok(png)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PixelPoint, PixelRect, PixelSize};

    #[test]
    fn crop_uses_screen_coordinates_with_negative_origin() {
        let mut buf = PixelBuffer::solid(PixelRect::new(-100, 0, 200, 100), 0, 0, 0);
        buf.put_pixel(PixelPoint { x: -50, y: 10 }, [0, 0, 255, 255]);
        let crop = buf.crop(&PixelRect::new(-60, 0, 20, 20));
        assert_eq!(crop.bounds, PixelRect::new(-60, 0, 20, 20));
        assert_eq!(crop.pixel(PixelPoint { x: -50, y: 10 }), [0, 0, 255, 255]);
    }

    #[test]
    fn crop_is_intersected_with_bounds() {
        let c = PixelBuffer::solid(PixelRect::new(0, 0, 10, 10), 0, 0, 0)
            .crop(&PixelRect::new(-5, -5, 50, 50));
        assert_eq!(c.bounds, PixelRect::new(0, 0, 10, 10));
    }

    #[test]
    fn with_origin_moves_bounds_only() {
        let b = PixelBuffer::solid(PixelRect::new(-1920, 0, 4, 4), 1, 2, 3)
            .with_origin(PixelPoint { x: 0, y: 0 });
        assert_eq!(
            (b.bounds, b.pixel(PixelPoint { x: 0, y: 0 })),
            (PixelRect::new(0, 0, 4, 4), [1, 2, 3, 255])
        );
    }

    #[test]
    fn encode_png_roundtrips_size_and_color() {
        let png = PixelBuffer::solid(PixelRect::new(0, 0, 7, 5), 10, 20, 30).encode_png().unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!((img.width(), img.height(), img.get_pixel(3, 2).0), (7, 5, [30, 20, 10, 255]));
    }

    #[test]
    fn cut_out_keeps_the_inside_and_clears_the_rest() {
        let square = [
            PixelPoint { x: 12, y: 12 },
            PixelPoint { x: 27, y: 12 },
            PixelPoint { x: 27, y: 27 },
            PixelPoint { x: 12, y: 27 },
        ];
        let mut img = PixelBuffer::solid(PixelRect::new(10, 10, 20, 20), 1, 2, 3);
        img.cut_out(&square, true);
        assert_eq!(img.pixel(PixelPoint { x: 20, y: 20 }), [1, 2, 3, 255]);
        assert_eq!(img.pixel(PixelPoint { x: 10, y: 10 }), [255, 255, 255, 0]);
        let edge = img.pixel(PixelPoint { x: 12, y: 20 })[3];
        assert!(edge > 0 && edge < 255, "the edge is half covered: {edge}");
        assert!(!img.is_opaque());
        assert_eq!(img.on_white().pixel(PixelPoint { x: 10, y: 10 }), [255, 255, 255, 255]);

        let mut jpg = PixelBuffer::solid(PixelRect::new(10, 10, 20, 20), 1, 2, 3);
        jpg.cut_out(&square, false);
        assert!(jpg.is_opaque());
        assert_eq!(jpg.pixel(PixelPoint { x: 29, y: 29 }), [255, 255, 255, 255]);
        assert_eq!(jpg.pixel(PixelPoint { x: 20, y: 20 }), [1, 2, 3, 255]);
    }

    #[test]
    fn transparent_png_keeps_alpha_and_jpg_goes_white() {
        let mut img = PixelBuffer::solid(PixelRect::new(0, 0, 4, 4), 0, 0, 0);
        img.put_pixel(PixelPoint { x: 0, y: 0 }, [255, 255, 255, 0]);
        let png = image::load_from_memory(&img.encode_png().unwrap()).unwrap().to_rgba8();
        assert_eq!(png.get_pixel(0, 0).0[3], 0);
        let jpg = image::load_from_memory(&img.encode_jpeg(95).unwrap()).unwrap().to_rgb8();
        assert!(jpg.get_pixel(0, 0).0.iter().all(|c| *c > 200), "{:?}", jpg.get_pixel(0, 0));
    }

    #[test]
    fn resized_changes_dimensions_keeps_origin() {
        let r = PixelBuffer::solid(PixelRect::new(4, 4, 160, 90), 0, 0, 0)
            .resized(PixelSize { width: 80, height: 45 });
        assert_eq!(r.bounds, PixelRect::new(4, 4, 80, 45));
    }
}
