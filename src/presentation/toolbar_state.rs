//! What the toolbar shows: labels, current settings, status line.

use crate::capture::{CaptureOutcome, CaptureOutcomeKind};
use crate::geometry::CaptureMode;
use crate::settings::AppSettings;
use std::path::PathBuf;

pub const INITIAL_STATUS: &str = "Scegli una modalità con il pulsante Modalità, poi premi Cattura.";

pub const MODE_OPTIONS: [(CaptureMode, &str); 5] = [
    (CaptureMode::Rectangle, "Rettangolo"),
    (CaptureMode::Window, "Finestra"),
    (CaptureMode::FullScreen, "Schermo intero"),
    (CaptureMode::Video, "Video MP4"),
    (CaptureMode::Gif, "GIF animata"),
];

pub const DELAY_OPTIONS: [(u32, &str); 6] = [
    (0, "Nessun ritardo"),
    (1, "1 secondo"),
    (2, "2 secondi"),
    (3, "3 secondi"),
    (4, "4 secondi"),
    (5, "5 secondi"),
];

pub fn mode_label(mode: CaptureMode) -> &'static str {
    MODE_OPTIONS.iter().find(|(m, _)| *m == mode).map(|(_, l)| *l).unwrap_or("Rettangolo")
}

const COPIED_SUFFIX: &str = " · copiato negli appunti";

/// Status line text after a capture, and whether it is an error.
pub fn format_status(o: &CaptureOutcome, current: &str) -> (String, bool) {
    let copied = if o.copied_to_clipboard { COPIED_SUFFIX } else { "" };
    match o.kind {
        CaptureOutcomeKind::Saved => {
            let name = o
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (format!("Salvato: {name}{copied}"), false)
        }
        CaptureOutcomeKind::Failed => {
            (format!("Errore: {}{copied}", o.error.as_deref().unwrap_or("sconosciuto")), true)
        }
        CaptureOutcomeKind::Canceled => (current.to_string(), false),
    }
}

pub struct ToolbarState {
    pub settings: AppSettings,
    pub status: String,
    pub status_is_error: bool,
    pub last_saved: Option<PathBuf>,
    pub busy: bool,
}

impl ToolbarState {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            status: INITIAL_STATUS.to_string(),
            status_is_error: false,
            last_saved: None,
            busy: false,
        }
    }

    pub fn apply_outcome(&mut self, o: &CaptureOutcome) {
        let (text, is_error) = format_status(o, &self.status);
        self.status = text;
        self.status_is_error = is_error;
        if o.kind == CaptureOutcomeKind::Saved {
            self.last_saved = o.path.clone();
        }
    }

    pub fn set_error(&mut self, msg: &str) {
        self.status = format!("Errore: {msg}");
        self.status_is_error = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureOutcome, CaptureOutcomeKind};
    use crate::settings::AppSettings;
    use std::path::PathBuf;

    #[test]
    fn mode_and_delay_labels_match_spec() {
        let labels: Vec<_> = MODE_OPTIONS.iter().map(|(_, l)| *l).collect();
        assert_eq!(
            labels,
            ["Rettangolo", "Finestra", "Schermo intero", "Video MP4", "GIF animata"]
        );
        assert_eq!(
            (DELAY_OPTIONS[0].1, DELAY_OPTIONS[1].1, DELAY_OPTIONS[5].1),
            ("Nessun ritardo", "1 secondo", "5 secondi")
        );
    }

    #[test]
    fn saved_and_copied_status() {
        let o = CaptureOutcome {
            kind: CaptureOutcomeKind::Saved,
            path: Some(PathBuf::from(r"C:\x\GyozaRec_1.png")),
            error: None,
            copied_to_clipboard: true,
        };
        assert_eq!(
            format_status(&o, "x"),
            ("Salvato: GyozaRec_1.png · copiato negli appunti".to_string(), false)
        );
    }

    #[test]
    fn failed_status_is_error() {
        let o = CaptureOutcome {
            kind: CaptureOutcomeKind::Failed,
            path: None,
            error: Some("disco pieno".into()),
            copied_to_clipboard: false,
        };
        assert_eq!(format_status(&o, "x"), ("Errore: disco pieno".to_string(), true));
    }

    #[test]
    fn canceled_keeps_current_text() {
        let o = CaptureOutcome {
            kind: CaptureOutcomeKind::Canceled,
            path: None,
            error: None,
            copied_to_clipboard: false,
        };
        assert_eq!(format_status(&o, "prima"), ("prima".to_string(), false));
    }

    #[test]
    fn apply_outcome_remembers_last_saved() {
        let mut s = ToolbarState::new(AppSettings::default());
        assert_eq!(s.status, INITIAL_STATUS);
        s.apply_outcome(&CaptureOutcome {
            kind: CaptureOutcomeKind::Saved,
            path: Some(PathBuf::from(r"C:\x\a.png")),
            error: None,
            copied_to_clipboard: false,
        });
        assert_eq!(s.last_saved, Some(PathBuf::from(r"C:\x\a.png")));
    }
}
