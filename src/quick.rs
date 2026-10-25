//! One-off captures requested from outside the toolbar: command line, a
//! second launch, the notification area or a global shortcut.

use crate::geometry::{CaptureArea, OutputFormat};
use crate::settings::AppSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickAction {
    /// The toolbar's current area, format and delay.
    Current,
    Image,
    ImageFullScreen,
    Record,
    RecordFullScreen,
}

impl QuickAction {
    pub const ALL: [QuickAction; 5] = [
        QuickAction::Current,
        QuickAction::Image,
        QuickAction::ImageFullScreen,
        QuickAction::Record,
        QuickAction::RecordFullScreen,
    ];

    /// Stable name for the command line and the single-instance protocol.
    pub fn name(self) -> &'static str {
        match self {
            QuickAction::Current => "capture",
            QuickAction::Image => "capture-image",
            QuickAction::ImageFullScreen => "capture-fullscreen",
            QuickAction::Record => "record",
            QuickAction::RecordFullScreen => "record-fullscreen",
        }
    }

    pub fn from_name(name: &str) -> Option<QuickAction> {
        QuickAction::ALL.into_iter().find(|a| a.name().eq_ignore_ascii_case(name))
    }
}

/// The settings for one capture; the saved settings stay untouched.
pub fn apply(action: QuickAction, s: &AppSettings) -> AppSettings {
    let recording = if s.format.is_recording() { s.format } else { OutputFormat::Mp4 };
    let (area, format) = match action {
        QuickAction::Current => (s.area, s.format),
        QuickAction::Image => (s.area, OutputFormat::Png),
        QuickAction::ImageFullScreen => (CaptureArea::FullScreen, OutputFormat::Png),
        QuickAction::Record => (s.area, recording),
        QuickAction::RecordFullScreen => (CaptureArea::FullScreen, recording),
    };
    AppSettings { area, format, ..s.clone() }
}

/// A capture is already running, or Options is open: the request is dropped.
pub fn can_start(busy: bool, options_open: bool) -> bool {
    !busy && !options_open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{CaptureArea, OutputFormat};
    use crate::settings::AppSettings;

    fn s(area: CaptureArea, format: OutputFormat) -> AppSettings {
        AppSettings { area, format, delay_seconds: 3, ..AppSettings::default() }
    }

    #[test]
    fn apply_overrides_area_and_format() {
        use CaptureArea::*;
        use OutputFormat::*;
        let cases = [
            (QuickAction::Current, s(Window, Gif), (Window, Gif)),
            (QuickAction::Image, s(Window, Gif), (Window, Png)),
            (QuickAction::ImageFullScreen, s(Window, Mp4), (FullScreen, Png)),
            (QuickAction::Record, s(Rectangle, Png), (Rectangle, Mp4)),
            (QuickAction::Record, s(Window, Gif), (Window, Gif)),
            (QuickAction::RecordFullScreen, s(Window, Png), (FullScreen, Mp4)),
            (QuickAction::RecordFullScreen, s(Rectangle, Gif), (FullScreen, Gif)),
        ];
        for (action, settings, expected) in cases {
            let out = apply(action, &settings);
            assert_eq!((out.area, out.format), expected, "{action:?}");
            assert_eq!(out.delay_seconds, 3, "other settings are kept");
        }
    }

    #[test]
    fn names_roundtrip() {
        for a in QuickAction::ALL {
            assert_eq!(QuickAction::from_name(a.name()), Some(a));
        }
        assert_eq!(QuickAction::from_name("nope"), None);
    }

    #[test]
    fn quick_action_is_ignored_while_busy() {
        assert!(can_start(false, false));
        assert!(!can_start(true, false));
        assert!(!can_start(false, true));
    }
}
