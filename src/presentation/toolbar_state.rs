//! What the toolbar shows: labels, current settings, status line.

use crate::capture::{CaptureOutcome, CaptureOutcomeKind};
use crate::error::{Failure, UserError};
use crate::geometry::{CaptureArea, OutputFormat};
use crate::i18n::{Language, Texts};
use crate::settings::AppSettings;
use std::path::PathBuf;

/// Areas in menu order; `Texts::areas` uses the same order.
pub const AREAS: [CaptureArea; 3] =
    [CaptureArea::Rectangle, CaptureArea::Window, CaptureArea::FullScreen];

/// Formats in menu order; `Texts::formats` uses the same order.
pub const FORMATS: [OutputFormat; 3] = [OutputFormat::Png, OutputFormat::Mp4, OutputFormat::Gif];

/// Delay choices in seconds; `Texts::delays` uses the same order.
pub const DELAYS: [u32; 6] = [0, 1, 2, 3, 4, 5];

pub fn area_label(area: CaptureArea, texts: &Texts) -> &'static str {
    texts.areas[AREAS.iter().position(|a| *a == area).unwrap_or(0)]
}

pub fn format_label(format: OutputFormat, texts: &Texts) -> &'static str {
    texts.formats[FORMATS.iter().position(|f| *f == format).unwrap_or(0)]
}

/// "Capture" for images, "Record" for videos and GIFs.
pub fn capture_label(settings: &AppSettings, texts: &Texts) -> &'static str {
    if settings.format.is_recording() { texts.record } else { texts.capture }
}

// Transitional: the single menu of the previous toolbar, removed with it.
pub const MODES: [(CaptureArea, OutputFormat); 5] = [
    (CaptureArea::Rectangle, OutputFormat::Png),
    (CaptureArea::Window, OutputFormat::Png),
    (CaptureArea::FullScreen, OutputFormat::Png),
    (CaptureArea::Rectangle, OutputFormat::Mp4),
    (CaptureArea::Rectangle, OutputFormat::Gif),
];

pub fn mode_label(mode: (CaptureArea, OutputFormat), texts: &Texts) -> &'static str {
    match mode {
        (area, OutputFormat::Png) => area_label(area, texts),
        (_, format) => format_label(format, texts),
    }
}

pub fn delay_label(seconds: u32, texts: &Texts) -> &'static str {
    texts.delays[(seconds as usize).min(DELAYS.len() - 1)]
}

/// Recording timer, e.g. `00:07 / 05:00`.
pub fn format_elapsed(elapsed: std::time::Duration, max: std::time::Duration) -> String {
    let mmss = |d: std::time::Duration| format!("{:02}:{:02}", d.as_secs() / 60, d.as_secs() % 60);
    format!("{} / {}", mmss(elapsed), mmss(max))
}

/// A failure in the interface language; OS messages are shown as Windows wrote them.
pub fn failure_text(failure: &Failure, texts: &Texts) -> String {
    match failure {
        Failure::User(UserError::EmptyArea) => texts.err_empty_area.into(),
        Failure::User(UserError::NoMonitor) => texts.err_no_monitor.into(),
        Failure::User(UserError::NoFrames) => texts.err_no_frames.into(),
        Failure::User(UserError::RecordingFailed) => texts.err_recording_failed.into(),
        Failure::User(UserError::InvalidVideoSize { width, height }) => texts
            .err_video_size
            .replace("{w}", &width.to_string())
            .replace("{h}", &height.to_string()),
        Failure::Other(message) => message.clone(),
    }
}

/// Status line text after a capture, and whether it is an error.
pub fn format_status(o: &CaptureOutcome, current: &str, texts: &Texts) -> (String, bool) {
    let copied = if o.copied_to_clipboard { texts.copied_suffix } else { "" };
    match o.kind {
        CaptureOutcomeKind::Saved => {
            let name = o
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (format!("{}{name}{copied}", texts.saved_prefix), false)
        }
        CaptureOutcomeKind::Failed => {
            let reason = o
                .error
                .as_ref()
                .map(|f| failure_text(f, texts))
                .unwrap_or_else(|| texts.err_recording_failed.into());
            (format!("{}{reason}{copied}", texts.error_prefix), true)
        }
        CaptureOutcomeKind::Canceled => (current.to_string(), false),
    }
}

pub struct ToolbarState {
    pub settings: AppSettings,
    pub language: Language,
    pub status: String,
    pub status_is_error: bool,
    pub last_saved: Option<PathBuf>,
    pub busy: bool,
    /// The outcome the status line shows, so it can be re-translated.
    shown_outcome: Option<CaptureOutcome>,
}

impl ToolbarState {
    pub fn new(settings: AppSettings, language: Language) -> Self {
        Self {
            settings,
            language,
            status: language.texts().initial_status.to_string(),
            status_is_error: false,
            last_saved: None,
            busy: false,
            shown_outcome: None,
        }
    }

    /// Switches language; the status follows unless it is free text.
    pub fn set_language(&mut self, language: Language) {
        let old_initial = self.texts().initial_status;
        self.language = language;
        if let Some(o) = &self.shown_outcome {
            let (text, _) = format_status(o, &self.status, self.texts());
            self.status = text;
        } else if self.status == old_initial {
            self.status = self.texts().initial_status.to_string();
        }
    }

    pub fn texts(&self) -> &'static Texts {
        self.language.texts()
    }

    pub fn apply_outcome(&mut self, o: &CaptureOutcome) {
        let (text, is_error) = format_status(o, &self.status, self.texts());
        self.status = text;
        self.status_is_error = is_error;
        if o.kind == CaptureOutcomeKind::Saved {
            self.last_saved = o.path.clone();
        }
        if o.kind != CaptureOutcomeKind::Canceled {
            self.shown_outcome = Some(o.clone());
        }
    }

    pub fn set_error(&mut self, msg: &str) {
        self.status = format!("{}{msg}", self.texts().error_prefix);
        self.status_is_error = true;
        self.shown_outcome = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureOutcome, CaptureOutcomeKind};
    use crate::error::{Failure, UserError};
    use crate::i18n::Language;
    use crate::settings::AppSettings;
    use std::path::PathBuf;

    #[test]
    fn area_format_and_delay_labels_follow_the_language() {
        let it = Language::It.texts();
        let areas: Vec<_> = AREAS.iter().map(|a| area_label(*a, it)).collect();
        assert_eq!(areas, ["Rettangolo", "Finestra", "Schermo intero"]);
        let formats: Vec<_> = FORMATS.iter().map(|f| format_label(*f, it)).collect();
        assert_eq!(formats, ["Immagine PNG", "Video MP4", "GIF animata"]);
        assert_eq!(format_label(OutputFormat::Gif, Language::En.texts()), "Animated GIF");
        assert_eq!(format_label(OutputFormat::Mp4, Language::Ja.texts()), "MP4 動画");
        assert_eq!(area_label(CaptureArea::Window, Language::Ja.texts()), "ウィンドウ");
        assert_eq!(DELAYS, [0, 1, 2, 3, 4, 5]);
        assert_eq!(delay_label(5, Language::En.texts()), "5 seconds");
        assert_eq!(delay_label(0, it), "Nessun ritardo");
    }

    #[test]
    fn capture_button_says_record_for_recordings() {
        let with = |format| AppSettings { format, ..AppSettings::default() };
        let en = Language::En.texts();
        assert_eq!(capture_label(&with(OutputFormat::Png), en), "Capture");
        assert_eq!(capture_label(&with(OutputFormat::Mp4), en), "Record");
        assert_eq!(capture_label(&with(OutputFormat::Gif), Language::It.texts()), "Registra");
    }

    #[test]
    fn initial_status_names_the_capture_button_and_no_mode_button() {
        for lang in Language::ALL {
            let t = lang.texts();
            assert!(t.initial_status.contains(t.capture), "{lang:?}: {}", t.initial_status);
            for old in ["Mode", "Modus", "Modalità", "Modo", "モード", "모드", "模式"] {
                assert!(!t.initial_status.contains(old), "{lang:?}: {}", t.initial_status);
            }
        }
    }

    #[test]
    fn elapsed_is_formatted_as_minutes_and_seconds() {
        use std::time::Duration;
        assert_eq!(
            format_elapsed(Duration::from_millis(7_900), Duration::from_secs(300)),
            "00:07 / 05:00"
        );
    }

    fn saved(copied: bool) -> CaptureOutcome {
        CaptureOutcome {
            kind: CaptureOutcomeKind::Saved,
            path: Some(PathBuf::from(r"C:\x\GyozaRec_1.png")),
            error: None,
            copied_to_clipboard: copied,
        }
    }

    fn failed(failure: Failure) -> CaptureOutcome {
        CaptureOutcome {
            kind: CaptureOutcomeKind::Failed,
            path: None,
            error: Some(failure),
            copied_to_clipboard: false,
        }
    }

    #[test]
    fn saved_and_copied_status_is_translated() {
        assert_eq!(
            format_status(&saved(true), "x", Language::It.texts()),
            ("Salvato: GyozaRec_1.png · copiato negli appunti".to_string(), false)
        );
        assert_eq!(
            format_status(&saved(true), "x", Language::En.texts()),
            ("Saved: GyozaRec_1.png · copied to clipboard".to_string(), false)
        );
    }

    #[test]
    fn os_failure_keeps_the_system_message() {
        let o = failed(Failure::Other("disk full".into()));
        assert_eq!(
            format_status(&o, "x", Language::It.texts()),
            ("Errore: disk full".to_string(), true)
        );
    }

    #[test]
    fn known_failures_are_translated() {
        let size = Failure::User(UserError::InvalidVideoSize { width: 101, height: 64 });
        let it = failure_text(&size, Language::It.texts());
        assert!(it.contains("101×64") && it.starts_with("dimensioni video"), "{it}");
        assert_eq!(
            failure_text(&Failure::User(UserError::NoMonitor), Language::De.texts()),
            "kein Monitor gefunden"
        );
    }

    #[test]
    fn canceled_keeps_current_text() {
        let o = CaptureOutcome {
            kind: CaptureOutcomeKind::Canceled,
            path: None,
            error: None,
            copied_to_clipboard: false,
        };
        assert_eq!(
            format_status(&o, "before", Language::It.texts()),
            ("before".to_string(), false)
        );
    }

    #[test]
    fn language_switch_retranslates_the_status() {
        let mut s = ToolbarState::new(AppSettings::default(), Language::En);
        s.set_language(Language::It);
        assert_eq!(s.status, Language::It.texts().initial_status);
        s.apply_outcome(&saved(true));
        s.set_language(Language::En);
        assert_eq!(s.status, "Saved: GyozaRec_1.png · copied to clipboard");
        s.set_error("disk full");
        s.set_language(Language::It);
        assert_eq!(s.status, "Error: disk full", "free text has no translation");
    }

    #[test]
    fn apply_outcome_remembers_last_saved() {
        let mut s = ToolbarState::new(AppSettings::default(), Language::It);
        assert_eq!(s.status, Language::It.texts().initial_status);
        s.apply_outcome(&CaptureOutcome {
            kind: CaptureOutcomeKind::Saved,
            path: Some(PathBuf::from(r"C:\x\a.png")),
            error: None,
            copied_to_clipboard: false,
        });
        assert_eq!(s.last_saved, Some(PathBuf::from(r"C:\x\a.png")));
    }
}
