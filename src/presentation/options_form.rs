//! Editable copy of the settings shown by the Options window.

use crate::settings::AppSettings;
use std::path::{Path, PathBuf};

pub const GIF_FPS_CHOICES: [u32; 4] = [5, 10, 15, 20];
pub const GIF_MAX_SECONDS_CHOICES: [u32; 5] = [5, 10, 15, 30, 60];
pub const GIF_LONG_SIDE_CHOICES: [u32; 6] = [480, 640, 800, 1024, 1280, 1920];
pub const VIDEO_FPS_CHOICES: [u32; 3] = [15, 30, 60];
pub const VIDEO_MAX_SECONDS_CHOICES: [u32; 7] = [30, 60, 120, 300, 600, 1800, 3600];

/// Shortens `text` to `max` characters by replacing its middle with "…".
pub fn shorten_middle(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max || max < 3 {
        return text.to_string();
    }
    let keep = max - 1;
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// `base` plus `current` if missing, sorted.
pub fn choices_with(base: &[u32], current: u32) -> Vec<u32> {
    let mut out = base.to_vec();
    if !out.contains(&current) {
        out.push(current);
        out.sort_unstable();
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptionsForm {
    pub save_folder: PathBuf,
    pub copy_to_clipboard: bool,
    pub play_sound: bool,
    pub include_cursor: bool,
    pub open_folder_after_recording: bool,
    pub open_file_after_capture: bool,
    pub gif_fps: u32,
    pub gif_max_seconds: u32,
    pub gif_max_long_side: u32,
    pub video_fps: u32,
    pub video_max_seconds: u32,
    /// `None` = follow Windows.
    pub language: Option<crate::i18n::Language>,
}

impl OptionsForm {
    pub fn from_settings(s: &AppSettings) -> OptionsForm {
        OptionsForm {
            save_folder: s.save_folder.clone(),
            copy_to_clipboard: s.copy_to_clipboard,
            play_sound: s.play_sound,
            include_cursor: s.include_cursor,
            open_folder_after_recording: s.open_folder_after_recording,
            open_file_after_capture: s.open_file_after_capture,
            gif_fps: s.gif_fps,
            gif_max_seconds: s.gif_max_seconds,
            gif_max_long_side: s.gif_max_long_side,
            video_fps: s.video_fps,
            video_max_seconds: s.video_max_seconds,
            language: s.language(),
        }
    }

    /// `base` with the edited fields applied; area, format and delay stay as in `base`.
    pub fn to_settings(&self, base: &AppSettings, capture_dir: &Path) -> AppSettings {
        AppSettings {
            save_folder: self.save_folder.clone(),
            copy_to_clipboard: self.copy_to_clipboard,
            play_sound: self.play_sound,
            include_cursor: self.include_cursor,
            open_folder_after_recording: self.open_folder_after_recording,
            open_file_after_capture: self.open_file_after_capture,
            gif_fps: self.gif_fps,
            gif_max_seconds: self.gif_max_seconds,
            gif_max_long_side: self.gif_max_long_side,
            video_fps: self.video_fps,
            video_max_seconds: self.video_max_seconds,
            language: self.language.map(|l| l.code().to_string()),
            ..base.clone()
        }
        .normalized(capture_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{CaptureArea, OutputFormat};
    use crate::settings::AppSettings;
    use std::path::PathBuf;

    fn cap() -> PathBuf {
        PathBuf::from(r"C:\caps")
    }

    #[test]
    fn shorten_middle_keeps_ends_within_limit() {
        let s = shorten_middle(r"C:\Users\someone\Pictures\Screenshots\GyozaRec", 24);
        assert_eq!(s.chars().count(), 24);
        assert!(s.starts_with(r"C:\Users") && s.ends_with("GyozaRec") && s.contains('…'));
        assert_eq!(shorten_middle("short", 24), "short");
    }

    #[test]
    fn choices_with_inserts_current_sorted() {
        assert_eq!(choices_with(&[5, 10, 15, 20], 12), vec![5, 10, 12, 15, 20]);
    }

    #[test]
    fn choices_with_does_not_duplicate() {
        assert_eq!(choices_with(&[5, 10, 15, 20], 10), vec![5, 10, 15, 20]);
    }

    #[test]
    fn to_settings_applies_edits_and_keeps_area_format_and_delay() {
        let base = AppSettings {
            area: CaptureArea::Window,
            format: OutputFormat::Gif,
            delay_seconds: 2,
            ..AppSettings::defaults(&cap())
        };
        let mut f = OptionsForm::from_settings(&base);
        f.gif_fps = 20;
        f.copy_to_clipboard = false;
        f.save_folder = PathBuf::from(r"D:\Shots");
        let s = f.to_settings(&base, &cap());
        assert_eq!(
            (s.gif_fps, s.copy_to_clipboard, s.save_folder.clone(), s.delay_seconds),
            (20, false, PathBuf::from(r"D:\Shots"), 2)
        );
        assert_eq!((s.area, s.format), (CaptureArea::Window, OutputFormat::Gif));
    }

    #[test]
    fn to_settings_normalizes_blank_folder() {
        let mut f = OptionsForm::from_settings(&AppSettings::defaults(&cap()));
        f.save_folder = PathBuf::new();
        assert_eq!(f.to_settings(&AppSettings::defaults(&cap()), &cap()).save_folder, cap());
    }

    #[test]
    fn open_file_option_defaults_off_and_roundtrips() {
        let base = AppSettings::defaults(&cap());
        let mut f = OptionsForm::from_settings(&base);
        assert!(!f.open_file_after_capture);
        f.open_file_after_capture = true;
        assert!(f.to_settings(&base, &cap()).open_file_after_capture);
    }

    #[test]
    fn open_folder_option_defaults_off_and_roundtrips() {
        let base = AppSettings::defaults(&cap());
        let mut f = OptionsForm::from_settings(&base);
        assert!(!f.open_folder_after_recording);
        f.open_folder_after_recording = true;
        assert!(f.to_settings(&base, &cap()).open_folder_after_recording);
    }

    #[test]
    fn language_choice_roundtrips_through_the_form() {
        use crate::i18n::Language;
        let base = AppSettings { language: Some("ko".into()), ..AppSettings::defaults(&cap()) };
        let mut f = OptionsForm::from_settings(&base);
        assert_eq!(f.language, Some(Language::Ko));
        f.language = Some(Language::PtBr);
        assert_eq!(f.to_settings(&base, &cap()).language(), Some(Language::PtBr));
        f.language = None;
        assert_eq!(f.to_settings(&base, &cap()).language, None);
    }
}
