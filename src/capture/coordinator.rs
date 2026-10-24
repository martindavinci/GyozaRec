//! Runs one capture end to end: hide toolbar, wait, grab, select, save.

use super::{
    CaptureOutcome, ClipboardService, CursorLocator, MonitorProvider, ScreenGrabber,
    SelectionRequest, SelectionUi, SoundPlayer, ToolbarVisibility, WindowSnapshotter,
    monitor_from_point,
};
use crate::clock::Clock;
use crate::geometry::{CaptureMode, meets_minimum};
use crate::output::{FileNamer, save_png};
use crate::pixels::PixelBuffer;
use crate::settings::AppSettings;
use std::sync::Arc;
use std::time::Duration;

/// Time for the hidden toolbar to disappear from the screen before grabbing.
pub const HIDE_SETTLE_DELAY: Duration = Duration::from_millis(200);

pub struct CaptureCoordinator {
    pub monitors: Box<dyn MonitorProvider>,
    pub grabber: Arc<dyn ScreenGrabber>,
    pub cursor: Box<dyn CursorLocator>,
    pub windows: Box<dyn WindowSnapshotter>,
    pub selection: Box<dyn SelectionUi>,
    pub clipboard: Box<dyn ClipboardService>,
    pub sound: Box<dyn SoundPlayer>,
    pub toolbar: Box<dyn ToolbarVisibility>,
    pub clock: Arc<dyn Clock>,
}

impl CaptureCoordinator {
    pub fn capture(&mut self, settings: &AppSettings) -> CaptureOutcome {
        self.toolbar.hide();
        let delay = Duration::from_secs(u64::from(settings.delay_seconds));
        self.clock.sleep(HIDE_SETTLE_DELAY + delay);
        let outcome = self.capture_hidden(settings);
        self.toolbar.show();
        outcome
    }

    fn capture_hidden(&mut self, settings: &AppSettings) -> CaptureOutcome {
        match settings.mode {
            CaptureMode::Rectangle | CaptureMode::Window => self.capture_region(settings),
            CaptureMode::FullScreen => self.capture_fullscreen(settings),
            CaptureMode::Video | CaptureMode::Gif => {
                CaptureOutcome::failed("non ancora supportato", false)
            }
        }
    }

    fn capture_region(&mut self, settings: &AppSettings) -> CaptureOutcome {
        let frozen = match self.grabber.grab(self.monitors.virtual_screen(), false) {
            Ok(frozen) => Arc::new(frozen),
            Err(err) => return CaptureOutcome::failed(err, false),
        };
        let windows =
            if settings.mode == CaptureMode::Window { self.windows.snapshot() } else { Vec::new() };
        let request = SelectionRequest {
            mode: settings.mode,
            frozen: Some(frozen.clone()),
            windows,
            monitors: self.monitors.monitors(),
        };
        match self.selection.select_region(request) {
            Some(region) if meets_minimum(settings.mode, &region) => {
                self.finish_image(&frozen.crop(&region), settings)
            }
            _ => CaptureOutcome::canceled(),
        }
    }

    fn capture_fullscreen(&mut self, settings: &AppSettings) -> CaptureOutcome {
        let monitors = self.monitors.monitors();
        if monitors.is_empty() {
            return CaptureOutcome::failed("nessun monitor trovato", false);
        }
        let monitor = monitor_from_point(&monitors, self.cursor.position());
        match self.grabber.grab(monitor.bounds, false) {
            Ok(img) => self.finish_image(&img, settings),
            Err(err) => CaptureOutcome::failed(err, false),
        }
    }

    /// Saves the PNG and copies it; the copy happens even if saving fails.
    fn finish_image(&mut self, img: &PixelBuffer, settings: &AppSettings) -> CaptureOutcome {
        let namer = FileNamer::new(self.clock.clone());
        let saved = save_png(img, &settings.save_folder, &namer);
        let copied = settings.copy_to_clipboard && self.clipboard.set_image(img);
        match saved {
            Ok(path) => {
                if settings.play_sound {
                    self.sound.play_capture();
                }
                CaptureOutcome::saved(path, copied)
            }
            Err(err) => CaptureOutcome::failed(err, copied),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::capture::{CaptureOutcomeKind, WindowCandidate};
    use crate::geometry::{CaptureMode, PixelPoint, PixelRect};
    use crate::settings::AppSettings;
    use crate::testing::CoordinatorHarness;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn canceled_selection_saves_nothing_and_restores_toolbar() {
        let mut h = CoordinatorHarness::rect(None);
        let o = h.run(h.settings());
        assert_eq!(o.kind, CaptureOutcomeKind::Canceled);
        assert_eq!(fs::read_dir(h.dir()).unwrap().count(), 0);
        let ev = h.events();
        assert_eq!((ev.first().unwrap().as_str(), ev.last().unwrap().as_str()), ("hide", "show"));
    }

    #[test]
    fn tiny_region_is_treated_as_cancel() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(5, 5, 1, 1)));
        assert_eq!(h.run(h.settings()).kind, CaptureOutcomeKind::Canceled);
    }

    #[test]
    fn rectangle_saves_png_of_selected_size_and_copies() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(10, 20, 30, 40)));
        let o = h.run(h.settings());
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let img = image::open(o.path.as_ref().unwrap()).unwrap();
        assert_eq!((img.width(), img.height(), o.copied_to_clipboard), (30, 40, true));
        assert_eq!(
            h.events(),
            [
                "hide",
                "grab:0,0,1920,1080",
                "select:Rectangle:frozen=true:windows=0",
                "clip:image:10,20,30,40",
                "sound",
                "show"
            ]
        );
    }

    #[test]
    fn delay_is_added_to_settle_delay() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 10, 10)));
        h.run(AppSettings { delay_seconds: 3, ..h.settings() });
        assert_eq!(h.clock.sleeps(), [Duration::from_millis(3200)]);
    }

    #[test]
    fn window_mode_passes_window_snapshot_to_selection() {
        let cand = WindowCandidate {
            id: 1,
            pid: 2,
            title: "x".into(),
            bounds: PixelRect::new(0, 0, 50, 50),
        };
        let mut h =
            CoordinatorHarness::with_windows(Some(PixelRect::new(0, 0, 50, 50)), vec![cand]);
        h.run(AppSettings { mode: CaptureMode::Window, ..h.settings() });
        assert!(h.events().contains(&"select:Window:frozen=true:windows=1".to_string()));
    }

    #[test]
    fn fullscreen_grabs_monitor_under_cursor() {
        let mut h = CoordinatorHarness::two_monitors_cursor_at(PixelPoint { x: -100, y: 100 });
        h.run(AppSettings { mode: CaptureMode::FullScreen, ..h.settings() });
        assert!(h.events().contains(&"grab:-1920,0,1920,1080".to_string()));
        assert!(!h.events().iter().any(|e| e.starts_with("select")));
    }

    #[test]
    fn save_failure_still_copies_to_clipboard() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 10, 10)));
        let blocker = h.dir().join("blocker");
        fs::write(&blocker, b"").unwrap();
        let o = h.run(AppSettings { save_folder: blocker.join("sub"), ..h.settings() });
        assert_eq!(
            (o.kind, o.error.is_some(), o.copied_to_clipboard),
            (CaptureOutcomeKind::Failed, true, true)
        );
    }
}
