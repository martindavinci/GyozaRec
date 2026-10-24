//! User settings persisted as JSON.

use crate::geometry::CaptureMode;
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub mode: CaptureMode,
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
}

/// Defaults with an empty save folder; `normalized` fills it in.
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Rectangle,
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
        }
    }
}

const VIDEO_FPS_STEPS: [u32; 3] = [15, 30, 60];

impl AppSettings {
    pub fn defaults(capture_dir: &Path) -> AppSettings {
        AppSettings { save_folder: capture_dir.to_path_buf(), ..AppSettings::default() }
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
            .and_then(|text| serde_json::from_str::<AppSettings>(&text).ok())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::CaptureMode;
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
            mode: CaptureMode::Gif,
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
        fs::write(&p, r#"{ "mode": "Window" }"#).unwrap();
        let s = SettingsStore::new(p, cap()).load();
        assert_eq!(
            (s.mode, s.copy_to_clipboard, s.gif_fps, s.save_folder),
            (CaptureMode::Window, true, 10, cap())
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
    fn mode_is_serialized_as_string() {
        let d = tempdir().unwrap();
        let p = d.path().join("s.json");
        SettingsStore::new(p.clone(), cap())
            .save(&AppSettings { mode: CaptureMode::Gif, ..AppSettings::defaults(&cap()) })
            .unwrap();
        assert!(fs::read_to_string(p).unwrap().contains(r#""mode": "Gif""#));
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
