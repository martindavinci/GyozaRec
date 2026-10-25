//! Interface languages and their texts. English is the reference language.

mod de;
mod en;
mod es;
mod fr;
mod it;
mod ja;
mod ko;
mod pt_br;
mod zh_cn;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    En,
    Es,
    Fr,
    De,
    It,
    PtBr,
    Ja,
    Ko,
    ZhCn,
}

impl Language {
    pub const ALL: [Language; 9] = [
        Language::En,
        Language::Es,
        Language::Fr,
        Language::De,
        Language::It,
        Language::PtBr,
        Language::Ja,
        Language::Ko,
        Language::ZhCn,
    ];

    /// Code stored in settings.
    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Es => "es",
            Language::Fr => "fr",
            Language::De => "de",
            Language::It => "it",
            Language::PtBr => "pt-BR",
            Language::Ja => "ja",
            Language::Ko => "ko",
            Language::ZhCn => "zh-CN",
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|l| l.code().eq_ignore_ascii_case(code))
    }

    /// Maps a Windows/BCP 47 tag (`it-IT`, `zh-Hans-CN`) by its language
    /// part: every Portuguese variant gets Brazilian Portuguese, every Chinese
    /// variant Simplified Chinese.
    pub fn from_locale(tag: &str) -> Option<Language> {
        let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        Some(match primary.as_str() {
            "en" => Language::En,
            "es" => Language::Es,
            "fr" => Language::Fr,
            "de" => Language::De,
            "it" => Language::It,
            "pt" => Language::PtBr,
            "ja" => Language::Ja,
            "ko" => Language::Ko,
            "zh" => Language::ZhCn,
            _ => return None,
        })
    }

    /// The language's name in itself, as shown in the language menu.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Es => "Español",
            Language::Fr => "Français",
            Language::De => "Deutsch",
            Language::It => "Italiano",
            Language::PtBr => "Português (Brasil)",
            Language::Ja => "日本語",
            Language::Ko => "한국어",
            Language::ZhCn => "简体中文",
        }
    }

    pub fn texts(self) -> &'static Texts {
        match self {
            Language::En => &en::TEXTS,
            Language::Es => &es::TEXTS,
            Language::Fr => &fr::TEXTS,
            Language::De => &de::TEXTS,
            Language::It => &it::TEXTS,
            Language::PtBr => &pt_br::TEXTS,
            Language::Ja => &ja::TEXTS,
            Language::Ko => &ko::TEXTS,
            Language::ZhCn => &zh_cn::TEXTS,
        }
    }

    /// Needs a font with CJK glyphs (the embedded font is Latin only).
    pub fn uses_cjk(self) -> bool {
        matches!(self, Language::Ja | Language::Ko | Language::ZhCn)
    }
}

/// The user's choice if any, else the first supported system language,
/// else English.
pub fn resolve(setting: Option<Language>, system: &[String]) -> Language {
    setting
        .or_else(|| system.iter().find_map(|tag| Language::from_locale(tag)))
        .unwrap_or(Language::En)
}

/// Every text the interface shows. `err_video_size` contains `{w}` and `{h}`.
pub struct Texts {
    pub capture: &'static str,
    pub options: &'static str,
    /// In `AREAS` order.
    pub areas: [&'static str; 3],
    /// In `FORMATS` order.
    pub formats: [&'static str; 4],
    /// Replaces `capture` on the button when the format is a recording.
    pub record: &'static str,
    /// No delay, then 1 to 5 seconds.
    pub delays: [&'static str; 6],
    /// `{button}` is replaced with the capture button's current label.
    pub initial_status: &'static str,
    pub saved_prefix: &'static str,
    pub copied_suffix: &'static str,
    pub error_prefix: &'static str,
    pub open_folder: &'static str,
    pub save_folder: &'static str,
    pub browse: &'static str,
    pub copy_to_clipboard: &'static str,
    pub play_sound: &'static str,
    pub include_cursor: &'static str,
    pub record_system_audio: &'static str,
    pub open_folder_after_recording: &'static str,
    pub open_file_after_capture: &'static str,
    pub keep_in_tray: &'static str,
    pub start_with_windows: &'static str,
    pub shortcuts_enabled: &'static str,
    pub print_screen_captures: &'static str,
    pub print_screen_hint: &'static str,
    pub shortcuts_title: &'static str,
    pub shortcut_unavailable: &'static str,
    pub press_new_keys: &'static str,
    pub reset_shortcuts: &'static str,
    pub shortcut_invalid: &'static str,
    pub shortcut_duplicate: &'static str,
    pub shortcut_unsupported: &'static str,
    pub tray_show: &'static str,
    pub tray_exit: &'static str,
    /// In `shortcuts::SHORTCUTS` order: image, full-screen image, record,
    /// full-screen recording.
    pub quick_actions: [&'static str; 4],
    pub discard_question: &'static str,
    pub pick_monitor_hint: &'static str,
    pub jpeg_quality: &'static str,
    pub gif_fps: &'static str,
    pub gif_max_seconds: &'static str,
    pub gif_max_long_side: &'static str,
    pub video_fps: &'static str,
    pub video_max_seconds: &'static str,
    pub language: &'static str,
    pub automatic: &'static str,
    pub ok: &'static str,
    pub cancel: &'static str,
    pub err_empty_area: &'static str,
    pub err_no_monitor: &'static str,
    pub err_no_frames: &'static str,
    pub err_video_size: &'static str,
    pub err_recording_failed: &'static str,
    pub err_capture_thread: &'static str,
    pub err_capture_failed: &'static str,
    pub err_video_unreadable: &'static str,
    pub convert_video: &'static str,
    /// `{percent}` is the progress, 0 to 100.
    pub converting: &'static str,
    pub conversion_canceled: &'static str,
    /// Shown when the app ends on an error; `{path}` is the crash log.
    pub crash_message: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags_map_to_supported_languages() {
        let cases = [
            ("it-IT", Some(Language::It)),
            ("en-GB", Some(Language::En)),
            ("es-MX", Some(Language::Es)),
            ("fr-CA", Some(Language::Fr)),
            ("de-AT", Some(Language::De)),
            ("pt-PT", Some(Language::PtBr)),
            ("pt-BR", Some(Language::PtBr)),
            ("ja-JP", Some(Language::Ja)),
            ("ko-KR", Some(Language::Ko)),
            ("zh-TW", Some(Language::ZhCn)),
            ("zh-Hans-CN", Some(Language::ZhCn)),
            ("ru-RU", None),
            ("", None),
        ];
        for (tag, expected) in cases {
            assert_eq!(Language::from_locale(tag), expected, "{tag}");
        }
    }

    #[test]
    fn codes_roundtrip() {
        for lang in Language::ALL {
            assert_eq!(Language::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Language::from_code("PT-br"), Some(Language::PtBr));
        assert_eq!(Language::from_code("xx"), None);
    }

    #[test]
    fn resolve_prefers_setting_then_first_supported_system_language() {
        let system = vec!["ru-RU".to_string(), "de-DE".to_string(), "en-US".to_string()];
        assert_eq!(resolve(Some(Language::Ja), &system), Language::Ja);
        assert_eq!(resolve(None, &system), Language::De);
        assert_eq!(resolve(None, &["ru-RU".to_string()]), Language::En);
        assert_eq!(resolve(None, &[]), Language::En);
    }

    #[test]
    fn every_language_has_complete_texts() {
        for lang in Language::ALL {
            let t = lang.texts();
            let all = [
                t.capture,
                t.record,
                t.options,
                t.initial_status,
                t.saved_prefix,
                t.copied_suffix,
                t.error_prefix,
                t.open_folder,
                t.save_folder,
                t.browse,
                t.copy_to_clipboard,
                t.play_sound,
                t.include_cursor,
                t.record_system_audio,
                t.open_folder_after_recording,
                t.open_file_after_capture,
                t.discard_question,
                t.pick_monitor_hint,
                t.jpeg_quality,
                t.gif_fps,
                t.gif_max_seconds,
                t.gif_max_long_side,
                t.video_fps,
                t.video_max_seconds,
                t.language,
                t.automatic,
                t.ok,
                t.cancel,
                t.err_empty_area,
                t.err_no_monitor,
                t.err_no_frames,
                t.err_video_size,
                t.err_recording_failed,
                t.err_capture_thread,
                t.err_capture_failed,
                t.crash_message,
                t.err_video_unreadable,
                t.convert_video,
                t.converting,
                t.conversion_canceled,
            ];
            assert!(
                all.iter()
                    .chain(&t.areas)
                    .chain(&t.formats)
                    .chain(&t.delays)
                    .all(|s| !s.trim().is_empty()),
                "{lang:?}"
            );
            assert!(
                t.crash_message.contains("{path}") && !t.err_capture_failed.is_empty(),
                "{lang:?}"
            );
            let shortcut_texts = [
                t.keep_in_tray,
                t.start_with_windows,
                t.shortcuts_enabled,
                t.print_screen_captures,
                t.print_screen_hint,
                t.shortcuts_title,
                t.shortcut_unavailable,
                t.press_new_keys,
                t.reset_shortcuts,
                t.shortcut_invalid,
                t.shortcut_duplicate,
                t.shortcut_unsupported,
                t.tray_show,
                t.tray_exit,
            ];
            assert!(
                shortcut_texts.iter().chain(&t.quick_actions).all(|s| !s.trim().is_empty()),
                "{lang:?}"
            );
            assert!(
                t.err_video_size.contains("{w}") && t.err_video_size.contains("{h}"),
                "{lang:?}"
            );
            assert!(!lang.native_name().is_empty());
        }
    }

    #[test]
    fn only_cjk_languages_need_the_fallback_font() {
        let cjk: Vec<_> = Language::ALL.into_iter().filter(|l| l.uses_cjk()).collect();
        assert_eq!(cjk, [Language::Ja, Language::Ko, Language::ZhCn]);
    }
}
