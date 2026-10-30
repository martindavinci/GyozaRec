//! Editable copy of the settings shown by the Options window.

use crate::settings::AppSettings;
use std::path::{Path, PathBuf};

pub const JPEG_QUALITY_CHOICES: [u32; 5] = [70, 80, 90, 95, 100];
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
    pub record_system_audio: bool,
    pub open_folder_after_recording: bool,
    pub open_file_after_capture: bool,
    pub open_editor_after_capture: bool,
    pub edit_before_saving: bool,
    pub editor_asks_to_save: bool,
    pub theme: crate::settings::ThemeChoice,
    pub interface_size: u16,
    pub show_notification: bool,
    pub check_for_updates: bool,
    pub file_names: crate::output::FileNames,
    pub file_name_prefix: bool,
    pub keep_in_tray: bool,
    pub start_with_windows: bool,
    pub shortcuts_enabled: bool,
    pub print_screen_captures: bool,
    pub shortcut_keys: std::collections::BTreeMap<String, String>,
    pub shortcuts_disabled: std::collections::BTreeSet<String>,
    pub jpeg_quality: u8,
    pub gif_fps: u32,
    pub gif_max_seconds: u32,
    pub gif_max_long_side: u32,
    pub video_fps: u32,
    pub video_max_seconds: u32,
    /// `None` = follow Windows.
    pub language: Option<crate::i18n::Language>,
}

impl OptionsForm {
    /// What an image capture does: 0 saves and opens the editor, 1 only
    /// saves, 2 copies and opens the editor without saving.
    pub fn image_action(&self) -> usize {
        match (self.edit_before_saving, self.open_editor_after_capture) {
            (true, _) => 2,
            (false, true) => 0,
            (false, false) => 1,
        }
    }

    pub fn set_image_action(&mut self, action: usize) {
        self.edit_before_saving = action == 2;
        if action < 2 {
            self.open_editor_after_capture = action == 0;
        }
    }

    pub fn from_settings(s: &AppSettings) -> OptionsForm {
        OptionsForm {
            save_folder: s.save_folder.clone(),
            copy_to_clipboard: s.copy_to_clipboard,
            play_sound: s.play_sound,
            include_cursor: s.include_cursor,
            record_system_audio: s.record_system_audio,
            open_folder_after_recording: s.open_folder_after_recording,
            open_file_after_capture: s.open_file_after_capture,
            open_editor_after_capture: s.open_editor_after_capture,
            edit_before_saving: s.edit_before_saving,
            editor_asks_to_save: s.editor_asks_to_save,
            theme: s.theme,
            interface_size: s.interface_size,
            show_notification: s.show_notification,
            check_for_updates: s.check_for_updates,
            file_names: s.file_names,
            file_name_prefix: s.file_name_prefix,
            keep_in_tray: s.keep_in_tray,
            start_with_windows: s.start_with_windows,
            shortcuts_enabled: s.shortcuts_enabled,
            print_screen_captures: s.print_screen_captures,
            shortcut_keys: s.shortcut_keys.clone(),
            shortcuts_disabled: s.shortcuts_disabled.clone(),
            jpeg_quality: s.jpeg_quality,
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
            record_system_audio: self.record_system_audio,
            open_folder_after_recording: self.open_folder_after_recording,
            open_file_after_capture: self.open_file_after_capture,
            open_editor_after_capture: self.open_editor_after_capture,
            edit_before_saving: self.edit_before_saving,
            editor_asks_to_save: self.editor_asks_to_save,
            theme: self.theme,
            interface_size: self.interface_size,
            show_notification: self.show_notification,
            check_for_updates: self.check_for_updates,
            file_names: self.file_names,
            file_name_prefix: self.file_name_prefix,
            keep_in_tray: self.keep_in_tray,
            start_with_windows: self.start_with_windows,
            shortcuts_enabled: self.shortcuts_enabled,
            print_screen_captures: self.print_screen_captures,
            shortcut_keys: self.shortcut_keys.clone(),
            shortcuts_disabled: self.shortcuts_disabled.clone(),
            jpeg_quality: self.jpeg_quality,
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
    fn tray_and_shortcut_options_roundtrip() {
        let base = AppSettings::defaults(&cap());
        let mut f = OptionsForm::from_settings(&base);
        assert_eq!(
            (f.keep_in_tray, f.shortcuts_enabled, f.print_screen_captures),
            (true, false, false)
        );
        (f.keep_in_tray, f.shortcuts_enabled, f.print_screen_captures) = (false, true, true);
        let s = f.to_settings(&base, &cap());
        assert_eq!(
            (s.keep_in_tray, s.shortcuts_enabled, s.print_screen_captures),
            (false, true, true)
        );
    }

    #[test]
    fn system_audio_defaults_on_and_roundtrips() {
        let base = AppSettings::defaults(&cap());
        let mut f = OptionsForm::from_settings(&base);
        assert!(f.record_system_audio);
        f.record_system_audio = false;
        assert!(!f.to_settings(&base, &cap()).record_system_audio);
    }

    #[test]
    fn start_with_windows_defaults_off_and_roundtrips() {
        let base = AppSettings::defaults(&cap());
        let mut f = OptionsForm::from_settings(&base);
        assert!(!f.start_with_windows);
        f.start_with_windows = true;
        assert!(f.to_settings(&base, &cap()).start_with_windows);
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
    fn edit_before_saving_defaults_off_and_asking_on() {
        let base = AppSettings::defaults(&cap());
        let mut f = OptionsForm::from_settings(&base);
        assert!(!f.edit_before_saving && f.editor_asks_to_save);
        assert_eq!(f.image_action(), 0);
        f.set_image_action(2);
        f.editor_asks_to_save = false;
        let s = f.to_settings(&base, &cap());
        assert!(s.edit_before_saving && !s.editor_asks_to_save);
    }

    #[test]
    fn image_action_maps_to_the_editor_settings() {
        let mut f = OptionsForm::from_settings(&AppSettings::defaults(&cap()));
        for i in [1, 2, 0, 2, 1] {
            f.set_image_action(i);
            assert_eq!(f.image_action(), i);
        }
        f.set_image_action(1);
        assert!(!f.open_editor_after_capture && !f.edit_before_saving);
        f.set_image_action(0);
        assert!(f.open_editor_after_capture && !f.edit_before_saving);
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
