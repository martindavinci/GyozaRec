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
pub const FORMATS: [OutputFormat; 4] =
    [OutputFormat::Png, OutputFormat::Jpg, OutputFormat::Mp4, OutputFormat::Gif];

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

pub fn delay_label(seconds: u32, texts: &Texts) -> &'static str {
    texts.delays[(seconds as usize).min(DELAYS.len() - 1)]
}

/// What a request to bring GyozaRec forward should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// A capture is running: showing the toolbar would put it in the picture.
    Nothing,
    FocusOptions,
    ShowToolbar,
}

pub fn activation(busy: bool, options_open: bool) -> Activation {
    match (busy, options_open) {
        (true, _) => Activation::Nothing,
        (false, true) => Activation::FocusOptions,
        (false, false) => Activation::ShowToolbar,
    }
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
        Failure::User(UserError::CaptureFailed) => texts.err_capture_failed.into(),
        Failure::User(UserError::VideoUnreadable) => texts.err_video_unreadable.into(),
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
        CaptureOutcomeKind::Unsaved => (format!("{}{copied}", texts.not_saved_status), false),
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

/// Where GitHub shows GyozaRec's releases; nothing else is ever opened.
pub const RELEASES_PAGE_PREFIX: &str = "https://github.com/martindavinci/GyozaRec/releases/";

/// A newer release on GitHub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateNote {
    pub version: String,
    pub page: String,
}

impl UpdateNote {
    /// The page to open, if it really is one of GyozaRec's releases.
    pub fn safe_page(&self) -> Option<&str> {
        let rest = self.page.strip_prefix(RELEASES_PAGE_PREFIX)?;
        let plain = rest.chars().all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c));
        (plain && !rest.contains("..")).then_some(self.page.as_str())
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
    /// The status line still shows the starting hint.
    showing_hint: bool,
    /// Progress of the running video conversion, in percent.
    pub converting: Option<u8>,
    pub update: Option<UpdateNote>,
    /// The status line announces `update`.
    update_shown: bool,
}

impl ToolbarState {
    pub fn new(settings: AppSettings, language: Language) -> Self {
        let mut state = Self {
            settings,
            language,
            status: String::new(),
            status_is_error: false,
            last_saved: None,
            busy: false,
            shown_outcome: None,
            showing_hint: true,
            converting: None,
            update: None,
            update_shown: false,
        };
        state.status = state.hint();
        state
    }

    fn hint(&self) -> String {
        let texts = self.texts();
        texts.initial_status.replace("{button}", capture_label(&self.settings, texts))
    }

    /// Call after changing `settings`: the hint names the capture button.
    pub fn settings_changed(&mut self) {
        if self.showing_hint {
            self.status = self.hint();
        }
    }

    /// Switches language; the status follows unless it is free text.
    pub fn set_language(&mut self, language: Language) {
        self.language = language;
        if self.update_shown {
            self.show_update();
        } else if let Some(percent) = self.converting {
            self.conversion_progress(percent);
        } else if let Some(o) = &self.shown_outcome {
            let (text, _) = format_status(o, &self.status, self.texts());
            self.status = text;
        } else if self.showing_hint {
            self.status = self.hint();
        }
    }

    pub fn texts(&self) -> &'static Texts {
        self.language.texts()
    }

    /// Announces a newer release on the status line (and keeps it for Options).
    pub fn update_available(&mut self, version: &str, page: String) {
        self.update = Some(UpdateNote { version: version.to_string(), page });
        self.show_update();
    }

    fn show_update(&mut self) {
        let Some(u) = &self.update else { return };
        self.status = self.texts().update_available.replace("{version}", &u.version);
        self.status_is_error = false;
        self.shown_outcome = None;
        self.showing_hint = false;
        self.update_shown = true;
    }

    /// The status line shows the update, with its Download link.
    pub fn shows_update(&self) -> bool {
        self.update_shown
    }

    pub fn apply_outcome(&mut self, o: &CaptureOutcome) {
        if o.kind != CaptureOutcomeKind::Canceled {
            self.update_shown = false;
        }
        let (text, is_error) = format_status(o, &self.status, self.texts());
        self.status = text;
        self.status_is_error = is_error;
        if o.kind == CaptureOutcomeKind::Saved {
            self.last_saved = o.path.clone();
        }
        if o.kind != CaptureOutcomeKind::Canceled {
            self.shown_outcome = Some(o.clone());
            self.showing_hint = false;
        }
    }

    /// Only one video is converted at a time.
    pub fn can_convert(&self) -> bool {
        self.converting.is_none()
    }

    pub fn conversion_progress(&mut self, percent: u8) {
        self.update_shown = false;
        self.converting = Some(percent.min(100));
        self.status = self.texts().converting.replace("{percent}", &percent.min(100).to_string());
        self.status_is_error = false;
        self.shown_outcome = None;
        self.showing_hint = false;
    }

    pub fn conversion_done(&mut self, path: PathBuf) {
        self.converting = None;
        self.apply_outcome(&CaptureOutcome::saved(path, false));
    }

    pub fn conversion_failed(&mut self, failure: &Failure) {
        self.converting = None;
        self.apply_outcome(&CaptureOutcome::failed(failure.clone(), false));
    }

    pub fn conversion_canceled(&mut self) {
        self.converting = None;
        self.status = self.texts().conversion_canceled.to_string();
        self.status_is_error = false;
    }

    pub fn set_error(&mut self, msg: &str) {
        self.update_shown = false;
        self.status = format!("{}{msg}", self.texts().error_prefix);
        self.status_is_error = true;
        self.shown_outcome = None;
        self.showing_hint = false;
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
        assert_eq!(formats, ["Immagine PNG", "Immagine JPG", "Video MP4", "GIF animata"]);
        assert_eq!(format_label(OutputFormat::Gif, Language::En.texts()), "Animated GIF");
        assert_eq!(format_label(OutputFormat::Mp4, Language::Ja.texts()), "MP4 動画");
        assert_eq!(area_label(CaptureArea::Window, Language::Ja.texts()), "ウィンドウ");
        assert_eq!(DELAYS, [0, 1, 2, 3, 4, 5]);
        assert_eq!(delay_label(5, Language::En.texts()), "5 seconds");
        assert_eq!(delay_label(0, it), "Nessun ritardo");
    }

    #[test]
    fn converting_shows_the_percent_and_blocks_a_second_conversion() {
        let mut s = ToolbarState::new(AppSettings::default(), Language::En);
        assert!(s.can_convert());
        s.conversion_progress(42);
        assert_eq!(s.status, "Converting to GIF… 42%");
        assert!(!s.can_convert());
        s.set_language(Language::It);
        assert_eq!(s.status, "Conversione in GIF… 42%");
        s.conversion_canceled();
        assert_eq!(s.status, "Conversione annullata");
        assert!(s.can_convert());
    }

    #[test]
    fn conversion_done_offers_the_folder() {
        let mut s = ToolbarState::new(AppSettings::default(), Language::En);
        s.conversion_progress(99);
        s.conversion_done(PathBuf::from("C:/v/clip.gif"));
        assert_eq!(s.status, "Saved: clip.gif");
        assert_eq!(s.last_saved, Some(PathBuf::from("C:/v/clip.gif")));
        assert!(s.can_convert());
        s.conversion_progress(10);
        s.conversion_failed(&Failure::User(UserError::VideoUnreadable));
        assert_eq!(s.status, "Error: this video can't be read");
        assert!(s.status_is_error && s.can_convert());
    }

    #[test]
    fn update_shows_a_download_link() {
        let mut s = ToolbarState::new(AppSettings::default(), Language::En);
        let page = "https://github.com/martindavinci/GyozaRec/releases/tag/v0.5.0";
        s.update_available("0.5.0", page.into());
        assert_eq!(s.status, "GyozaRec 0.5.0 is available");
        assert!(s.shows_update());
        s.set_language(Language::It);
        assert_eq!(s.status, "GyozaRec 0.5.0 è disponibile");
        s.apply_outcome(&CaptureOutcome::saved(PathBuf::from("a.png"), false));
        assert!(!s.shows_update(), "a capture takes the status line");
        assert_eq!(s.update.as_ref().map(|u| u.page.as_str()), Some(page), "Options still knows");
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
            assert_eq!(t.initial_status.matches("{button}").count(), 1, "{lang:?}");
            for old in ["Mode", "Modus", "Modalità", "Modo", "モード", "모드", "模式"] {
                assert!(!t.initial_status.contains(old), "{lang:?}: {}", t.initial_status);
            }
        }
    }

    #[test]
    fn activation_policy() {
        assert_eq!(activation(true, false), Activation::Nothing);
        assert_eq!(activation(true, true), Activation::Nothing);
        assert_eq!(activation(false, true), Activation::FocusOptions);
        assert_eq!(activation(false, false), Activation::ShowToolbar);
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
    fn unsaved_capture_says_so() {
        assert_eq!(
            format_status(&CaptureOutcome::unsaved(true), "x", Language::En.texts()),
            ("Not saved yet · copied to clipboard".to_string(), false)
        );
        let mut s = ToolbarState::new(AppSettings::default(), Language::En);
        s.apply_outcome(&saved(false));
        s.apply_outcome(&CaptureOutcome::unsaved(true));
        assert_eq!(s.last_saved, Some(PathBuf::from(r"C:\x\GyozaRec_1.png")), "kept");
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
        assert_eq!(s.status, "Scegli cosa catturare e il formato, poi premi Cattura.");
        s.apply_outcome(&saved(true));
        s.set_language(Language::En);
        assert_eq!(s.status, "Saved: GyozaRec_1.png · copied to clipboard");
        s.set_error("disk full");
        s.set_language(Language::It);
        assert_eq!(s.status, "Error: disk full", "free text has no translation");
    }

    #[test]
    fn hint_names_the_button_as_it_is_labelled() {
        let mut s = ToolbarState::new(AppSettings::default(), Language::En);
        assert_eq!(s.status, "Choose what to capture and the format, then press Capture.");
        s.settings.format = OutputFormat::Gif;
        s.settings_changed();
        assert_eq!(s.status, "Choose what to capture and the format, then press Record.");
        s.apply_outcome(&saved(false));
        s.settings.format = OutputFormat::Png;
        s.settings_changed();
        assert_eq!(s.status, "Saved: GyozaRec_1.png", "a result stays until the next one");
    }

    #[test]
    fn apply_outcome_remembers_last_saved() {
        let mut s = ToolbarState::new(AppSettings::default(), Language::It);
        s.apply_outcome(&CaptureOutcome {
            kind: CaptureOutcomeKind::Saved,
            path: Some(PathBuf::from(r"C:\x\a.png")),
            error: None,
            copied_to_clipboard: false,
        });
        assert_eq!(s.last_saved, Some(PathBuf::from(r"C:\x\a.png")));
    }
}
