//! User settings persisted as JSON.

use crate::geometry::{CaptureArea, OutputFormat};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub area: CaptureArea,
    pub format: OutputFormat,
    pub delay_seconds: u32,
    pub save_folder: PathBuf,
    pub copy_to_clipboard: bool,
    pub play_sound: bool,
    pub include_cursor: bool,
    pub gif_fps: u32,
    pub gif_max_seconds: u32,
    pub gif_max_long_side: u32,
    pub video_fps: u32,
    pub video_max_seconds: u32,
    /// Interface language code (`it`, `pt-BR`…); absent or unknown means
    /// "follow Windows". Kept as text so an unknown code never resets the
    /// other settings.
    pub language: Option<String>,
}

/// Defaults with an empty save folder; `normalized` fills it in.
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            area: CaptureArea::Rectangle,
            format: OutputFormat::Png,
            delay_seconds: 0,
            save_folder: PathBuf::new(),
            copy_to_clipboard: true,
            play_sound: true,
            include_cursor: true,
            gif_fps: 10,
            gif_max_seconds: 15,
            gif_max_long_side: 800,
            video_fps: 30,
            video_max_seconds: 300,
            language: None,
        }
    }
}

const VIDEO_FPS_STEPS: [u32; 3] = [15, 30, 60];

impl AppSettings {
    pub fn defaults(capture_dir: &Path) -> AppSettings {
        AppSettings { save_folder: capture_dir.to_path_buf(), ..AppSettings::default() }
    }

    /// The chosen interface language, or `None` for automatic.
    pub fn language(&self) -> Option<crate::i18n::Language> {
        self.language.as_deref().and_then(crate::i18n::Language::from_code)
    }

    /// Brings every value back into its allowed range.
    pub fn normalized(self, capture_dir: &Path) -> AppSettings {
        let save_folder = if self.save_folder.as_os_str().to_string_lossy().trim().is_empty() {
            capture_dir.to_path_buf()
        } else {
            self.save_folder
        };
        AppSettings {
            delay_seconds: self.delay_seconds.min(5),
            save_folder,
            gif_fps: self.gif_fps.clamp(5, 20),
            gif_max_seconds: self.gif_max_seconds.clamp(1, 60),
            gif_max_long_side: self.gif_max_long_side.clamp(240, 1920),
            video_fps: nearest_video_fps(self.video_fps),
            video_max_seconds: self.video_max_seconds.clamp(5, 3600),
            ..self
        }
    }
}

/// Nearest allowed frame rate; on a tie the higher one wins.
fn nearest_video_fps(fps: u32) -> u32 {
    VIDEO_FPS_STEPS
        .into_iter()
        .min_by_key(|&step| (step.abs_diff(fps), std::cmp::Reverse(step)))
        .unwrap_or(30)
}

pub struct SettingsStore {
    path: PathBuf,
    capture_dir: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf, capture_dir: PathBuf) -> Self {
        Self { path, capture_dir }
    }

    pub fn from_paths(paths: &AppPaths) -> Self {
        Self::new(paths.settings_file.clone(), paths.capture_dir.clone())
    }

    /// Never fails: a missing or unreadable file yields the defaults.
    pub fn load(&self) -> AppSettings {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|mut value| {
                migrate_legacy_mode(&mut value);
                serde_json::from_value::<AppSettings>(value).ok()
            })
            .unwrap_or_default()
            .normalized(&self.capture_dir)
    }

    pub fn save(&self, settings: &AppSettings) -> crate::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(settings)?)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Files written before area and format were separate have a single `mode`
/// (`Rectangle`, `Window`, `FullScreen`, `Video` or `Gif`).
fn migrate_legacy_mode(value: &mut serde_json::Value) {
    let Some(object) = value.as_object_mut() else { return };
    let Some(mode) = object.remove("mode") else { return };
    if object.contains_key("area") || object.contains_key("format") {
        return;
    }
    let (area, format) = match mode.as_str() {
        Some("Window") => ("Window", "Png"),
        Some("FullScreen") => ("FullScreen", "Png"),
        Some("Video") => ("Rectangle", "Mp4"),
        Some("Gif") => ("Rectangle", "Gif"),
        _ => ("Rectangle", "Png"),
    };
    object.insert("area".into(), area.into());
    object.insert("format".into(), format.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{CaptureArea, OutputFormat};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn cap() -> PathBuf {
        PathBuf::from(r"C:\caps")
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let d = tempdir().unwrap();
        assert_eq!(
            SettingsStore::new(d.path().join("s.json"), cap()).load(),
            AppSettings::defaults(&cap())
        );
    }

    #[test]
    fn load_corrupt_json_returns_defaults() {
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        fs::write(&p, "{ not json").unwrap();
        assert_eq!(SettingsStore::new(p, cap()).load(), AppSettings::defaults(&cap()));
    }

    #[test]
    fn save_then_load_roundtrips() {
        let d = tempdir().unwrap();
        let st = SettingsStore::new(d.path().join("sub/s.json"), cap());
        let s = AppSettings {
            area: CaptureArea::Window,
            format: OutputFormat::Gif,
            delay_seconds: 3,
            gif_fps: 15,
            ..AppSettings::defaults(&cap())
        };
        st.save(&s).unwrap();
        assert_eq!(st.load(), s);
    }

    #[test]
    fn load_partial_json_fills_missing_with_defaults() {
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        fs::write(&p, r#"{ "area": "Window" }"#).unwrap();
        let s = SettingsStore::new(p, cap()).load();
        assert_eq!(
            (s.area, s.format, s.copy_to_clipboard, s.gif_fps, s.save_folder),
            (CaptureArea::Window, OutputFormat::Png, true, 10, cap())
        );
    }

    #[test]
    fn load_clamps_out_of_range_values() {
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        fs::write(
            &p,
            r#"{ "delay_seconds": 99, "gif_fps": 1, "video_fps": 25, "gif_max_long_side": 99999, "video_max_seconds": 1, "save_folder": "  " }"#,
        )
        .unwrap();
        let s = SettingsStore::new(p, cap()).load();
        assert_eq!(
            (s.delay_seconds, s.gif_fps, s.video_fps, s.gif_max_long_side, s.video_max_seconds),
            (5, 5, 30, 1920, 5)
        );
        assert_eq!(s.save_folder, cap());
    }

    #[test]
    fn area_and_format_are_serialized_as_strings_without_mode() {
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        let s = AppSettings {
            area: CaptureArea::FullScreen,
            format: OutputFormat::Gif,
            ..AppSettings::defaults(&cap())
        };
        SettingsStore::new(p.clone(), cap()).save(&s).unwrap();
        let text = fs::read_to_string(p).unwrap();
        assert!(text.contains(r#""area": "FullScreen""#) && text.contains(r#""format": "Gif""#));
        assert!(!text.contains(r#""mode""#), "{text}");
    }

    #[test]
    fn legacy_mode_is_migrated_to_area_and_format() {
        let cases = [
            ("Rectangle", CaptureArea::Rectangle, OutputFormat::Png),
            ("Window", CaptureArea::Window, OutputFormat::Png),
            ("FullScreen", CaptureArea::FullScreen, OutputFormat::Png),
            ("Video", CaptureArea::Rectangle, OutputFormat::Mp4),
            ("Gif", CaptureArea::Rectangle, OutputFormat::Gif),
        ];
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        for (mode, area, format) in cases {
            fs::write(&p, format!(r#"{{ "mode": "{mode}", "gif_fps": 15 }}"#)).unwrap();
            let s = SettingsStore::new(p.clone(), cap()).load();
            assert_eq!((s.area, s.format, s.gif_fps), (area, format, 15), "{mode}");
        }
    }

    #[test]
    fn new_fields_win_over_a_leftover_mode() {
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        fs::write(&p, r#"{ "mode": "Video", "area": "Window", "format": "Png" }"#).unwrap();
        let s = SettingsStore::new(p, cap()).load();
        assert_eq!((s.area, s.format), (CaptureArea::Window, OutputFormat::Png));
    }

    #[test]
    fn language_defaults_to_automatic_and_roundtrips() {
        use crate::i18n::Language;
        let d = tempdir().unwrap();
        let st = SettingsStore::new(d.path().join("s.json"), cap());
        assert_eq!(st.load().language(), None);
        let s = AppSettings { language: Some("ja".into()), ..AppSettings::defaults(&cap()) };
        st.save(&s).unwrap();
        assert_eq!(st.load().language(), Some(Language::Ja));
    }

    #[test]
    fn unknown_language_code_means_automatic_and_keeps_other_settings() {
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        fs::write(&p, r#"{ "language": "xx", "gif_fps": 15 }"#).unwrap();
        let s = SettingsStore::new(p, cap()).load();
        assert_eq!((s.language(), s.gif_fps), (None, 15));
    }

    #[test]
    fn save_reports_unwritable_location() {
        let d = tempdir().unwrap();
        let blocker = d.path().join("blocker");
        fs::write(&blocker, b"").unwrap();
        assert!(
            SettingsStore::new(blocker.join("s.json"), cap())
                .save(&AppSettings::defaults(&cap()))
                .is_err()
        );
    }
}
