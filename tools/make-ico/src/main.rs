//! Builds GyozaRec.ico from a large PNG, and extracts PNGs back from it.
//!
//! cargo run -p make-ico -- build <input.png> <out.ico> [--key-out-background]
//! cargo run -p make-ico -- extract <in.ico> <out.png>

use image::{Rgba, RgbaImage, imageops};
use std::collections::VecDeque;
use std::process::ExitCode;

const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
/// Max per-channel distance from the corner color still treated as background.
const KEY_TOLERANCE: u8 = 24;

/// Pads to a centered square with transparent pixels.
fn square(src: &RgbaImage) -> RgbaImage {
    let side = src.width().max(src.height());
    let mut out = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
    let x = (side - src.width()) / 2;
    let y = (side - src.height()) / 2;
    imageops::overlay(&mut out, src, i64::from(x), i64::from(y));
    out
}

/// Makes the background connected to the four corners transparent.
fn key_out_background(src: &RgbaImage) -> RgbaImage {
    let mut img = src.clone();
    let (w, h) = img.dimensions();
    let mut seen = vec![false; (w * h) as usize];
    for (cx, cy) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        let key = src.get_pixel(cx, cy).0;
        let near = |p: [u8; 4]| (0..3).all(|i| p[i].abs_diff(key[i]) <= KEY_TOLERANCE);
        let mut queue = VecDeque::from([(cx, cy)]);
        while let Some((x, y)) = queue.pop_front() {
            let i = (y * w + x) as usize;
            if seen[i] || !near(src.get_pixel(x, y).0) {
                continue;
            }
            seen[i] = true;
            img.get_pixel_mut(x, y).0[3] = 0;
            if x > 0 {
                queue.push_back((x - 1, y));
            }
            if y > 0 {
                queue.push_back((x, y - 1));
            }
            if x + 1 < w {
                queue.push_back((x + 1, y));
            }
            if y + 1 < h {
                queue.push_back((x, y + 1));
            }
        }
    }
    img
}

/// ICO with PNG entries at every size in `SIZES`.
fn build_ico(src: &RgbaImage, key_out: bool) -> Vec<u8> {
    let base = if key_out { key_out_background(src) } else { src.clone() };
    let base = square(&base);
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in SIZES {
        let scaled = imageops::resize(&base, size, size, imageops::FilterType::Lanczos3);
        let image = ico::IconImage::from_rgba_data(size, size, scaled.into_raw());
        dir.add_entry(ico::IconDirEntry::encode_as_png(&image).expect("encode icon entry"));
    }
    let mut out = Vec::new();
    dir.write(&mut out).expect("write icon");
    out
}

/// The largest entry of an ICO file.
fn extract_largest(ico_bytes: &[u8]) -> RgbaImage {
    let dir = ico::IconDir::read(std::io::Cursor::new(ico_bytes)).expect("read icon");
    let entry = dir.entries().iter().max_by_key(|e| e.width()).expect("icon has entries");
    let image = entry.decode().expect("decode icon entry");
    RgbaImage::from_raw(image.width(), image.height(), image.rgba_data().to_vec())
        .expect("rgba size")
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, input, output, rest @ ..] if cmd == "build" => {
            let key_out = rest.iter().any(|a| a == "--key-out-background");
            let src = image::open(input).map_err(|e| format!("{input}: {e}"))?.to_rgba8();
            std::fs::write(output, build_ico(&src, key_out)).map_err(|e| format!("{output}: {e}"))
        }
        [cmd, input, output] if cmd == "extract" => {
            let bytes = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
            extract_largest(&bytes).save(output).map_err(|e| format!("{output}: {e}"))
        }
        _ => Err("uso: make-ico build <in.png> <out.ico> [--key-out-background] | make-ico extract <in.ico> <out.png>".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> image::RgbaImage {
        image::RgbaImage::from_pixel(300, 200, image::Rgba([250, 230, 180, 255]))
    }

    #[test]
    fn ico_has_seven_png_entries() {
        let dir = ico::IconDir::read(std::io::Cursor::new(build_ico(&sample(), false))).unwrap();
        let sizes: Vec<_> = dir.entries().iter().map(|e| e.width()).collect();
        assert_eq!(sizes, [16, 24, 32, 48, 64, 128, 256]);
        assert!(dir.entries().iter().all(|e| e.is_png()));
    }

    #[test]
    fn extract_returns_256_entry() {
        let img = extract_largest(&build_ico(&sample(), false));
        assert_eq!((img.width(), img.height()), (256, 256));
    }

    #[test]
    fn key_out_makes_corner_background_transparent() {
        let mut src = image::RgbaImage::from_pixel(100, 100, image::Rgba([255, 255, 255, 255]));
        for y in 30..70 {
            for x in 30..70 {
                src.put_pixel(x, y, image::Rgba([240, 200, 120, 255]));
            }
        }
        let img = extract_largest(&build_ico(&src, true));
        assert_eq!(img.get_pixel(0, 0).0[3], 0);
        assert_eq!(img.get_pixel(128, 128).0[3], 255);
    }
}
