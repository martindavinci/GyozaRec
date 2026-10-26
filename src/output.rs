//! File names and saving of captures.

use crate::clock::Clock;
use crate::pixels::PixelBuffer;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How captures are named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileNames {
    /// `GyozaRec_yyyy-MM-dd_HH-mm-ss`.
    #[default]
    DateTime,
    /// Ten random letters and digits, nothing else.
    Random,
}

pub const FILE_NAMES: [FileNames; 2] = [FileNames::DateTime, FileNames::Random];

/// What a name looks like in this style, for Options.
pub fn example_name(style: FileNames, now: chrono::NaiveDateTime) -> String {
    match style {
        FileNames::DateTime => format!("GyozaRec_{}.png", now.format("%Y-%m-%d_%H-%M-%S")),
        FileNames::Random => "k3F9xQ2bTm.png".to_string(),
    }
}

/// Letters and digits that cannot be mistaken for one another.
pub const RANDOM_ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

pub struct FileNamer {
    clock: Arc<dyn Clock>,
    style: FileNames,
}

impl FileNamer {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self::with_style(clock, FileNames::DateTime)
    }

    pub fn with_style(clock: Arc<dyn Clock>, style: FileNames) -> Self {
        Self { clock, style }
    }

    /// `GyozaRec_yyyy-MM-dd_HH-mm-ss.<ext>`, with `_2`, `_3`… when taken;
    /// or a random name, drawn again while taken.
    pub fn next_path(&self, folder: &Path, ext: &str) -> PathBuf {
        if self.style == FileNames::Random {
            return std::iter::repeat_with(random_stem)
                .map(|stem| folder.join(format!("{stem}.{ext}")))
                .find(|p| !p.exists())
                .expect("unbounded draws");
        }
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

/// Ten characters of `RANDOM_ALPHABET`, uniformly drawn.
fn random_stem() -> String {
    let alphabet: Vec<char> = RANDOM_ALPHABET.chars().collect();
    let n = alphabet.len() as u8;
    // Bytes at or above the largest multiple of `n` are thrown away, so
    // every character is equally likely.
    let limit = 256 / u16::from(n) * u16::from(n);
    let mut out = String::with_capacity(10);
    while out.chars().count() < 10 {
        let mut bytes = [0u8; 16];
        crate::platform::random_bytes(&mut bytes);
        for b in bytes {
            if u16::from(b) < limit && out.chars().count() < 10 {
                out.push(alphabet[usize::from(b % n)]);
            }
        }
    }
    out
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
    fn random_names_are_ten_unambiguous_characters_without_the_app_name() {
        let d = tempdir().unwrap();
        let n = FileNamer::with_style(namer().clock, FileNames::Random);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let path = n.next_path(d.path(), "png");
            assert_eq!(path.extension().unwrap(), "png");
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            assert_eq!(stem.chars().count(), 10, "{stem}");
            assert!(stem.chars().all(|c| RANDOM_ALPHABET.contains(c)), "{stem}");
            assert!(seen.insert(stem), "names repeat");
        }
        for c in ['0', 'O', 'I', 'l'] {
            assert!(!RANDOM_ALPHABET.contains(c), "{c} looks like another character");
        }
    }

    #[test]
    fn examples_show_each_style() {
        let at = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_hms_opt(14, 30, 5).unwrap();
        assert_eq!(example_name(FileNames::DateTime, at), "GyozaRec_2026-10-26_14-30-05.png");
        let random = example_name(FileNames::Random, at);
        assert!(random.trim_end_matches(".png").chars().all(|c| RANDOM_ALPHABET.contains(c)));
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
