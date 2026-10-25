//! Draws annotations, on the canvas or into the final picture, and saves it.

use super::{Annotation, View};
use crate::geometry::{PixelPoint, PixelRect};
use crate::pixels::PixelBuffer;
use crate::uikit::painter::{Fonts, Painter, RectF};
use std::path::Path;

/// Highlighter strokes: three times wider, 40 % opaque.
const HIGHLIGHT_WIDTH: f32 = 3.0;
const HIGHLIGHT_ALPHA: u8 = 102;

/// Draws `a` where `view` puts it; sizes scale with the view.
pub fn draw(p: &mut Painter, a: &Annotation, view: &View, fonts: &Fonts) {
    let k = view.scale;
    let rect = |a: (f32, f32), b: (f32, f32)| {
        let (a, b) = (view.to_canvas(a), view.to_canvas(b));
        RectF { x: a.0.min(b.0), y: a.1.min(b.1), w: (a.0 - b.0).abs(), h: (a.1 - b.1).abs() }
    };
    match a {
        Annotation::Arrow { from, to, color, width } => {
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-3 {
                return;
            }
            let (ux, uy) = (dx / len, dy / len);
            let head = (3.0 * width + 8.0).min(len);
            let half = 1.5 * width + 4.0;
            let base = (to.0 - ux * head, to.1 - uy * head);
            // The shaft stops at the head, so its round end never shows past the tip.
            let shaft = [view.to_canvas(*from), view.to_canvas(base)];
            p.stroke_polyline(&shaft, width * k, color.color(), 255);
            let corner =
                |side: f32| view.to_canvas((base.0 - uy * half * side, base.1 + ux * half * side));
            p.fill_triangle([view.to_canvas(*to), corner(1.0), corner(-1.0)], color.color());
        }
        Annotation::Rectangle { a, b, color, width } => {
            p.stroke_rect(rect(*a, *b), width * k, color.color());
        }
        Annotation::Stroke { points, color, width, highlight } => {
            let points: Vec<(f32, f32)> = points.iter().map(|q| view.to_canvas(*q)).collect();
            let (w, alpha) =
                if *highlight { (width * HIGHLIGHT_WIDTH, HIGHLIGHT_ALPHA) } else { (*width, 255) };
            p.stroke_polyline(&points, w * k, color.color(), alpha);
        }
        Annotation::Text { at, text, color, px } => {
            let (x, y) = view.to_canvas(*at);
            p.text(fonts, text, x, y, px * k, color.color(), true);
        }
        Annotation::Pixelate { a, b, block } => p.pixelate(rect(*a, *b), block * k),
    }
}

/// `base` with `annotations` drawn in order, cropped to `visible`.
pub(super) fn flatten(
    base: &PixelBuffer,
    annotations: &[Annotation],
    visible: PixelRect,
    fonts: &Fonts,
) -> PixelBuffer {
    let mut p = Painter::from_buffer(base);
    for a in annotations {
        draw(&mut p, a, &View::IDENTITY, fonts);
    }
    p.into_buffer(PixelPoint { x: 0, y: 0 }).crop(&visible).with_origin(PixelPoint { x: 0, y: 0 })
}

/// Writes PNG, or JPEG for `.jpg`/`.jpeg`, through a temporary file so a
/// failed write never damages the old picture.
pub fn save(img: &PixelBuffer, path: &Path, jpeg_quality: u8) -> crate::Result<()> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let bytes = if ext == "jpg" || ext == "jpeg" {
        img.encode_jpeg(jpeg_quality)?
    } else {
        img.encode_png()?
    };
    let temp = temp_path_for(path);
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(&bytes).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })?;
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })?;
    Ok(())
}

/// A temporary file next to `path`, unique to this save.
fn temp_path_for(path: &Path) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.{}-{n}.gyozarec-tmp", std::process::id()))
}

/// Reads a PNG or JPEG back into an opaque buffer at (0, 0).
pub fn load(path: &Path) -> crate::Result<PixelBuffer> {
    let rgba = image::open(path)?.to_rgba8();
    let (w, h) = (rgba.width() as i32, rgba.height() as i32);
    let mut bgra = rgba.into_raw();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
        px[3] = 255;
    }
    Ok(PixelBuffer::new(PixelRect::new(0, 0, w, h), bgra))
}

#[cfg(test)]
mod tests {
    use super::super::{Document, PALETTE, pixel_block};
    use super::*;
    use std::sync::Arc;

    fn white(w: i32, h: i32) -> Document {
        Document::new(Arc::new(PixelBuffer::solid(PixelRect::new(0, 0, w, h), 255, 255, 255)))
    }

    fn at(img: &PixelBuffer, x: i32, y: i32) -> [u8; 4] {
        img.pixel(PixelPoint { x, y })
    }

    #[test]
    fn rectangle_and_arrow_paint_their_color() {
        let mut d = white(200, 100);
        d.add(Annotation::Rectangle {
            a: (10.0, 10.0),
            b: (90.0, 90.0),
            color: PALETTE[0],
            width: 6.0,
        });
        d.add(Annotation::Arrow {
            from: (120.0, 50.0),
            to: (190.0, 50.0),
            color: PALETTE[3],
            width: 6.0,
        });
        let img = d.flatten(&Fonts::embedded());
        assert_eq!(at(&img, 10, 50), [0x35, 0x39, 0xE5, 255], "red edge (BGRA)");
        assert_eq!(at(&img, 50, 50), [255, 255, 255, 255], "inside stays white");
        assert_eq!(at(&img, 184, 50), [0xE5, 0x88, 0x1E, 255], "blue head");
    }

    #[test]
    fn highlighter_lets_the_image_show_through() {
        let mut d =
            Document::new(Arc::new(PixelBuffer::solid(PixelRect::new(0, 0, 60, 60), 0, 0, 0)));
        let stroke = vec![(5.0, 30.0), (55.0, 30.0)];
        d.add(Annotation::Stroke {
            points: stroke,
            color: PALETTE[1],
            width: 6.0,
            highlight: true,
        });
        let px = at(&d.flatten(&Fonts::embedded()), 30, 30);
        assert!(px[2] > 60 && px[2] < 200, "red between black and yellow: {px:?}");
    }

    #[test]
    fn pixelate_makes_blocks_uniform() {
        let mut noise = PixelBuffer::solid(PixelRect::new(0, 0, 64, 64), 0, 0, 0);
        for y in 0..64 {
            for x in 0..64 {
                let v = ((x * 37 + y * 91) % 256) as u8;
                noise.put_pixel(PixelPoint { x, y }, [v, 255 - v, v / 2, 255]);
            }
        }
        let mut d = Document::new(Arc::new(noise));
        assert_eq!(pixel_block(64, 64), 8.0);
        d.add(Annotation::Pixelate { a: (0.0, 0.0), b: (64.0, 64.0), block: 8.0 });
        let img = d.flatten(&Fonts::embedded());
        for by in 0..8 {
            for bx in 0..8 {
                let first = at(&img, bx * 8, by * 8);
                for y in 0..8 {
                    for x in 0..8 {
                        assert_eq!(at(&img, bx * 8 + x, by * 8 + y), first, "block {bx},{by}");
                    }
                }
            }
        }
    }

    #[test]
    fn text_is_drawn() {
        let mut d = white(200, 60);
        d.add(Annotation::Text {
            at: (10.0, 10.0),
            text: "Hi".into(),
            color: PALETTE[4],
            px: 28.0,
        });
        let img = d.flatten(&Fonts::embedded());
        let dark = img.bgra.as_chunks::<4>().0.iter().filter(|p| p[0] < 100).count();
        assert!(dark > 50, "{dark} dark pixels");
    }

    #[test]
    fn painter_round_trip_keeps_colors() {
        let mut img = PixelBuffer::solid(PixelRect::new(5, 7, 3, 1), 10, 20, 30);
        img.put_pixel(PixelPoint { x: 6, y: 7 }, [200, 100, 50, 255]);
        let back = Painter::from_buffer(&img).into_buffer(PixelPoint { x: 5, y: 7 });
        assert_eq!(back, img);
    }

    #[test]
    fn save_writes_png_or_jpg_by_extension() {
        let d = tempfile::tempdir().unwrap();
        let img = PixelBuffer::solid(PixelRect::new(0, 0, 30, 20), 10, 20, 200);
        for name in ["a.png", "a.jpg"] {
            let path = d.path().join(name);
            save(&img, &path, 90).unwrap();
            let format = image::ImageFormat::from_path(&path).unwrap();
            let read =
                image::load(std::io::BufReader::new(std::fs::File::open(&path).unwrap()), format)
                    .unwrap();
            assert_eq!((read.width(), read.height()), (30, 20));
        }
        let names: Vec<_> =
            std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "no temporary files left: {names:?}");
    }

    #[test]
    fn saved_png_loads_back() {
        let d = tempfile::tempdir().unwrap();
        let mut img = PixelBuffer::solid(PixelRect::new(0, 0, 3, 2), 10, 20, 200);
        img.put_pixel(PixelPoint { x: 2, y: 1 }, [1, 2, 3, 255]);
        let path = d.path().join("a.png");
        save(&img, &path, 90).unwrap();
        assert_eq!(load(&path).unwrap(), img);
    }

    #[test]
    fn every_save_writes_its_own_temporary_file() {
        let path = Path::new("C:/x/a.png");
        assert_ne!(temp_path_for(path), temp_path_for(path));
        assert_eq!(temp_path_for(path).parent(), path.parent());
    }

    #[test]
    fn save_to_a_missing_folder_fails() {
        let d = tempfile::tempdir().unwrap();
        let img = PixelBuffer::solid(PixelRect::new(0, 0, 4, 4), 0, 0, 0);
        assert!(save(&img, &d.path().join("nope").join("a.png"), 90).is_err());
        assert!(!d.path().join("nope").exists());
    }
}
