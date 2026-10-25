//! BGRA pixel buffers positioned on the virtual screen.

use crate::geometry::{PixelPoint, PixelRect, PixelSize};
use image::{ImageBuffer, Rgba, imageops};

/// Top-down BGRA pixels (alpha 255), stride = width × 4. `bounds` places the
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
    fn resized_changes_dimensions_keeps_origin() {
        let r = PixelBuffer::solid(PixelRect::new(4, 4, 160, 90), 0, 0, 0)
            .resized(PixelSize { width: 80, height: 45 });
        assert_eq!(r.bounds, PixelRect::new(4, 4, 80, 45));
    }
}
