//! What the toolbar shows: labels, current settings, status line.

use crate::capture::{CaptureOutcome, CaptureOutcomeKind};
use crate::error::{Failure, UserError};
use crate::geometry::CaptureMode;
use crate::i18n::{Language, Texts};
use crate::settings::AppSettings;
use std::path::PathBuf;

/// Modes in menu order; `Texts::modes` uses the same order.
pub const MODES: [CaptureMode; 5] = [
    CaptureMode::Rectangle,
    CaptureMode::Window,
    CaptureMode::FullScreen,
    CaptureMode::Video,
    CaptureMode::Gif,
];

/// Delay choices in seconds; `Texts::delays` uses the same order.
pub const DELAYS: [u32; 6] = [0, 1, 2, 3, 4, 5];

pub fn mode_label(mode: CaptureMode, texts: &Texts) -> &'static str {
    let index = MODES.iter().position(|m| *m == mode).unwrap_or(0);
    texts.modes[index]
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
    }

    pub fn set_error(&mut self, msg: &str) {
        self.status = format!("{}{msg}", self.texts().error_prefix);
        self.status_is_error = true;
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
    fn mode_and_delay_labels_follow_the_language() {
        let it = Language::It.texts();
        let labels: Vec<_> = MODES.iter().map(|m| mode_label(*m, it)).collect();
        assert_eq!(
            labels,
            ["Rettangolo", "Finestra", "Schermo intero", "Video MP4", "GIF animata"]
        );
        assert_eq!(mode_label(CaptureMode::Gif, Language::En.texts()), "Animated GIF");
        assert_eq!(mode_label(CaptureMode::Window, Language::Ja.texts()), "ウィンドウ");
        assert_eq!(DELAYS, [0, 1, 2, 3, 4, 5]);
        assert_eq!(delay_label(5, Language::En.texts()), "5 seconds");
        assert_eq!(delay_label(0, it), "Nessun ritardo");
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
        let o = failed(Failure::Other("disco pieno".into()));
        assert_eq!(
            format_status(&o, "x", Language::It.texts()),
            ("Errore: disco pieno".to_string(), true)
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
