//! File names and saving of captures.

use crate::clock::Clock;
use crate::pixels::PixelBuffer;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct FileNamer {
    clock: Arc<dyn Clock>,
}

impl FileNamer {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }

    /// `GyozaRec_yyyy-MM-dd_HH-mm-ss.<ext>`, with `_2`, `_3`… when taken.
    pub fn next_path(&self, folder: &Path, ext: &str) -> PathBuf {
        let stem = format!("GyozaRec_{}", self.clock.local_now().format("%Y-%m-%d_%H-%M-%S"));
        let first = folder.join(format!("{stem}.{ext}"));
        if !first.exists() {
            return first;
        }
        (2..)
            .map(|n| folder.join(format!("{stem}_{n}.{ext}")))
            .find(|p| !p.exists())
            .expect("unbounded counter")
    }
}

pub fn ensure_folder(folder: &Path) -> crate::Result<()> {
    fs::create_dir_all(folder)?;
    Ok(())
}

pub fn save_png(img: &PixelBuffer, folder: &Path, namer: &FileNamer) -> crate::Result<PathBuf> {
    save_image(img, folder, namer, crate::geometry::OutputFormat::Png, 0)
}

/// Saves `img` as JPG for `OutputFormat::Jpg`, as PNG otherwise.
pub fn save_image(
    img: &PixelBuffer,
    folder: &Path,
    namer: &FileNamer,
    format: crate::geometry::OutputFormat,
    jpeg_quality: u8,
) -> crate::Result<PathBuf> {
    ensure_folder(folder)?;
    let (ext, bytes) = match format {
        crate::geometry::OutputFormat::Jpg => ("jpg", img.encode_jpeg(jpeg_quality)?),
        _ => ("png", img.encode_png()?),
    };
    let path = namer.next_path(folder, ext);
    fs::write(&path, bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelRect;
    use crate::pixels::PixelBuffer;
    use crate::testing::FakeClock;
    use chrono::NaiveDate;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn namer() -> FileNamer {
        FileNamer::new(Arc::new(FakeClock::new(
            NaiveDate::from_ymd_opt(2026, 10, 24).unwrap().and_hms_opt(15, 3, 7).unwrap(),
        )))
    }

    #[test]
    fn next_path_uses_local_timestamp_pattern() {
        let d = tempdir().unwrap();
        assert_eq!(
            namer().next_path(d.path(), "png"),
            d.path().join("GyozaRec_2026-10-24_15-03-07.png")
        );
    }

    #[test]
    fn next_path_appends_counter_on_collision() {
        let d = tempdir().unwrap();
        let n = namer();
        fs::write(n.next_path(d.path(), "png"), b"").unwrap();
        assert!(n.next_path(d.path(), "png").ends_with("GyozaRec_2026-10-24_15-03-07_2.png"));
        fs::write(n.next_path(d.path(), "png"), b"").unwrap();
        assert!(n.next_path(d.path(), "png").ends_with("GyozaRec_2026-10-24_15-03-07_3.png"));
    }

    #[test]
    fn save_png_creates_missing_folder() {
        let d = tempdir().unwrap();
        let img = PixelBuffer::solid(PixelRect::new(0, 0, 4, 4), 0, 0, 0);
        assert!(save_png(&img, &d.path().join("a/b"), &namer()).unwrap().exists());
    }

    #[test]
    fn save_png_reports_unwritable_folder() {
        let d = tempdir().unwrap();
        let blocker = d.path().join("blocker");
        fs::write(&blocker, b"").unwrap();
        let img = PixelBuffer::solid(PixelRect::new(0, 0, 4, 4), 0, 0, 0);
        assert!(save_png(&img, &blocker.join("sub"), &namer()).is_err());
    }
}
