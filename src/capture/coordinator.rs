//! Runs one capture end to end: hide toolbar, wait, grab, select, save.

use super::{
    AudioStarter, CaptureOutcome, ClipboardService, CursorLocator, FileRevealer, MonitorProvider,
    RecordingJob, RecordingUi, ScreenGrabber, SelectionRequest, SelectionUi, SinkFactory, SinkKind,
    SoundPlayer, ToolbarVisibility, WindowSnapshotter, monitor_from_point,
};
use crate::clock::Clock;
use crate::geometry::{CaptureArea, OutputFormat, PixelRect, meets_minimum};
use crate::output::{FileNamer, ensure_folder, save_image};
use crate::pixels::PixelBuffer;
use crate::recording::{LoopConfig, RecordingOutcome};
use crate::settings::AppSettings;
use std::sync::Arc;
use std::time::Duration;

/// Time for the hidden toolbar to disappear from the screen before grabbing.
pub const HIDE_SETTLE_DELAY: Duration = Duration::from_millis(200);
/// Time for the selection overlays to disappear before recording starts.
pub const OVERLAY_CLOSE_SETTLE: Duration = Duration::from_millis(150);

pub struct CaptureCoordinator {
    pub monitors: Box<dyn MonitorProvider>,
    pub grabber: Arc<dyn ScreenGrabber>,
    pub cursor: Box<dyn CursorLocator>,
    pub windows: Box<dyn WindowSnapshotter>,
    pub selection: Box<dyn SelectionUi>,
    pub clipboard: Box<dyn ClipboardService>,
    pub sound: Box<dyn SoundPlayer>,
    pub revealer: Box<dyn FileRevealer>,
    pub toolbar: Box<dyn ToolbarVisibility>,
    pub recording_ui: Box<dyn RecordingUi>,
    pub sinks: Box<dyn SinkFactory>,
    pub audio: Box<dyn AudioStarter>,
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
        match (settings.format, settings.area) {
            (OutputFormat::Png | OutputFormat::Jpg, CaptureArea::FullScreen) => {
                self.capture_fullscreen(settings)
            }
            (OutputFormat::Png | OutputFormat::Jpg, _) => self.capture_region(settings),
            (OutputFormat::Mp4 | OutputFormat::Gif, _) => self.capture_recording(settings),
        }
    }

    /// Freezes the desktop and lets the user pick a region on it.
    fn select_on_frozen(
        &mut self,
        settings: &AppSettings,
    ) -> Result<(Arc<PixelBuffer>, PixelRect), CaptureOutcome> {
        let frozen = match self.grabber.grab(self.monitors.virtual_screen(), false) {
            Ok(frozen) => Arc::new(frozen),
            Err(err) => return Err(CaptureOutcome::failed(err, false)),
        };
        let windows =
            if settings.area == CaptureArea::Window { self.windows.snapshot() } else { Vec::new() };
        let request = SelectionRequest {
            area: settings.area,
            frozen: Some(frozen.clone()),
            windows,
            monitors: self.monitors.monitors(),
        };
        match self.selection.select_region(request) {
            Some(region) => Ok((frozen, region)),
            None => Err(CaptureOutcome::canceled()),
        }
    }

    fn capture_region(&mut self, settings: &AppSettings) -> CaptureOutcome {
        match self.select_on_frozen(settings) {
            Ok((frozen, region)) if meets_minimum(settings.format, &region) => {
                self.finish_image(&frozen.crop(&region), settings)
            }
            Ok(_) => CaptureOutcome::canceled(),
            Err(outcome) => outcome,
        }
    }

    fn capture_recording(&mut self, settings: &AppSettings) -> CaptureOutcome {
        let video = settings.format == OutputFormat::Mp4;
        let monitors = self.monitors.monitors();
        if monitors.is_empty() {
            return CaptureOutcome::failed(crate::UserError::NoMonitor, false);
        }
        let picked = if settings.area == CaptureArea::FullScreen && monitors.len() == 1 {
            monitors[0].bounds
        } else {
            match self.select_on_frozen(settings) {
                Ok((_, region)) => region,
                Err(outcome) => return outcome,
            }
        };
        // A maximized window reaches past the screen edges; record only what is visible.
        let region = picked.clamp_to(&self.monitors.virtual_screen());
        let region = if video { region.even_floor() } else { region };
        if !meets_minimum(settings.format, &region) {
            return CaptureOutcome::canceled();
        }
        if let Err(err) = ensure_folder(&settings.save_folder) {
            return CaptureOutcome::failed(err, false);
        }
        let monitor = monitor_from_point(&monitors, region.center()).clone();
        let (ext, fps, max_seconds, kind) = if video {
            ("mp4", settings.video_fps, settings.video_max_seconds, SinkKind::Mp4 { audio: false })
        } else {
            let kind = SinkKind::Gif { max_long_side: settings.gif_max_long_side };
            ("gif", settings.gif_fps, settings.gif_max_seconds, kind)
        };
        // Audio starts before the sink so the file knows whether it has a track;
        // without a working device the recording is video-only.
        let feed = if video && settings.record_system_audio {
            self.audio.start(self.clock.clone())
        } else {
            None
        };
        let kind = match kind {
            SinkKind::Mp4 { .. } => SinkKind::Mp4 { audio: feed.is_some() },
            other => other,
        };
        let path = FileNamer::new(self.clock.clone()).next_path(&settings.save_folder, ext);
        let sink = match self.sinks.create(&kind, &path, region.size(), fps) {
            Ok(sink) => sink,
            Err(err) => return CaptureOutcome::failed(err, false),
        };
        let config = LoopConfig {
            region,
            fps,
            include_cursor: settings.include_cursor,
            max_duration: Duration::from_secs(u64::from(max_seconds)),
        };
        self.clock.sleep(OVERLAY_CLOSE_SETTLE);
        let job = RecordingJob { region, monitor, path, config, sink, audio: feed };
        let result = self.recording_ui.record(job);
        match (result.outcome, result.path) {
            (RecordingOutcome::Saved, Some(path)) => {
                let copied = settings.copy_to_clipboard && self.clipboard.set_file(&path);
                if settings.play_sound {
                    self.sound.play_capture();
                }
                if settings.open_folder_after_recording {
                    self.revealer.reveal(&path);
                }
                if settings.open_file_after_capture {
                    self.revealer.open(&path);
                }
                CaptureOutcome::saved(path, copied)
            }
            (RecordingOutcome::Canceled, _) => CaptureOutcome::canceled(),
            _ => CaptureOutcome::failed(
                result.error.unwrap_or(crate::Failure::User(crate::UserError::RecordingFailed)),
                false,
            ),
        }
    }

    fn capture_fullscreen(&mut self, settings: &AppSettings) -> CaptureOutcome {
        let monitors = self.monitors.monitors();
        if monitors.is_empty() {
            return CaptureOutcome::failed(crate::UserError::NoMonitor, false);
        }
        if monitors.len() > 1 {
            // Several screens: the user clicks the one to capture.
            return match self.select_on_frozen(settings) {
                Ok((frozen, region)) => self.finish_image(&frozen.crop(&region), settings),
                Err(outcome) => outcome,
            };
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
        let saved =
            save_image(img, &settings.save_folder, &namer, settings.format, settings.jpeg_quality);
        let copied = settings.copy_to_clipboard && self.clipboard.set_image(img);
        match saved {
            Ok(path) => {
                if settings.play_sound {
                    self.sound.play_capture();
                }
                if settings.open_file_after_capture {
                    self.revealer.open(&path);
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
    use crate::geometry::{CaptureArea, OutputFormat, PixelPoint, PixelRect};
    use crate::recording::RecordingOutcome;
    use crate::settings::AppSettings;
    use crate::testing::CoordinatorHarness;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn video_region_is_floored_to_even_and_saved_as_mp4() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 101, 65)))
            .recording(RecordingOutcome::Saved);
        let o = h.run(AppSettings { format: OutputFormat::Mp4, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let ev = h.events();
        assert!(ev.contains(&"select:Rectangle:frozen=true:windows=0".to_string()));
        assert!(ev.contains(&"sink:Mp4 { audio: false }:100x64".to_string()));
        assert!(ev.contains(&"record:0,0,100,64:mp4:fps=30:max=300".to_string()));
        assert!(ev.iter().any(|e| e.starts_with("clip:file:") && e.ends_with(".mp4")));
        assert_eq!(h.clock.sleeps(), [Duration::from_millis(200), Duration::from_millis(150)]);
    }

    #[test]
    fn coordinator_starts_audio_only_for_mp4_with_the_option() {
        let rect = Some(PixelRect::new(0, 0, 200, 100));
        let mut mp4 =
            CoordinatorHarness::rect(rect).with_audio().recording(RecordingOutcome::Saved);
        mp4.run(AppSettings { format: OutputFormat::Mp4, ..mp4.settings() });
        let ev = mp4.events();
        assert!(ev.contains(&"audio:start".to_string()), "{ev:?}");
        assert!(ev.contains(&"sink:Mp4 { audio: true }:200x100".to_string()), "{ev:?}");

        let mut gif =
            CoordinatorHarness::rect(rect).with_audio().recording(RecordingOutcome::Saved);
        gif.run(AppSettings { format: OutputFormat::Gif, ..gif.settings() });
        assert!(!gif.events().contains(&"audio:start".to_string()));

        let mut off =
            CoordinatorHarness::rect(rect).with_audio().recording(RecordingOutcome::Saved);
        off.run(AppSettings {
            format: OutputFormat::Mp4,
            record_system_audio: false,
            ..off.settings()
        });
        assert!(!off.events().contains(&"audio:start".to_string()));
        assert!(off.events().contains(&"sink:Mp4 { audio: false }:200x100".to_string()));
    }

    #[test]
    fn missing_audio_device_records_video_only() {
        let rect = Some(PixelRect::new(0, 0, 200, 100));
        let mut h = CoordinatorHarness::rect(rect).recording(RecordingOutcome::Saved);
        let o = h.run(AppSettings { format: OutputFormat::Mp4, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let ev = h.events();
        assert!(ev.contains(&"audio:start".to_string()), "it was tried: {ev:?}");
        assert!(ev.contains(&"sink:Mp4 { audio: false }:200x100".to_string()), "{ev:?}");
    }

    #[test]
    fn gif_below_minimum_cancels_without_sink() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 10, 10)))
            .recording(RecordingOutcome::Saved);
        let o = h.run(AppSettings { format: OutputFormat::Gif, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Canceled);
        assert!(!h.events().iter().any(|e| e.starts_with("sink")));
    }

    #[test]
    fn gif_uses_gif_settings() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        h.run(AppSettings {
            format: OutputFormat::Gif,
            gif_fps: 15,
            gif_max_seconds: 7,
            gif_max_long_side: 640,
            ..h.settings()
        });
        assert!(h.events().contains(&"sink:Gif { max_long_side: 640 }:200x100".to_string()));
        assert!(h.events().contains(&"record:0,0,200,100:gif:fps=15:max=7".to_string()));
    }

    #[test]
    fn saved_recording_reveals_the_file_when_enabled() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        h.run(AppSettings {
            format: OutputFormat::Gif,
            open_folder_after_recording: true,
            ..h.settings()
        });
        let ev = h.events();
        assert!(ev.iter().any(|e| e.starts_with("reveal:") && e.ends_with(".gif")), "{ev:?}");
    }

    #[test]
    fn recording_does_not_reveal_by_default() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        h.run(AppSettings { format: OutputFormat::Mp4, ..h.settings() });
        assert!(!h.events().iter().any(|e| e.starts_with("reveal:")));
    }

    #[test]
    fn png_and_canceled_recordings_never_reveal() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)));
        h.run(AppSettings { open_folder_after_recording: true, ..h.settings() });
        let mut c = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Canceled);
        c.run(AppSettings {
            format: OutputFormat::Gif,
            open_folder_after_recording: true,
            ..c.settings()
        });
        let ev = [h.events(), c.events()].concat();
        assert!(!ev.iter().any(|e| e.starts_with("reveal:")), "{ev:?}");
    }

    #[test]
    fn saved_capture_opens_the_file_when_enabled() {
        let mut png = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)));
        png.run(AppSettings { open_file_after_capture: true, ..png.settings() });
        let mut gif = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        gif.run(AppSettings {
            format: OutputFormat::Gif,
            open_file_after_capture: true,
            ..gif.settings()
        });
        let opened = |ev: Vec<String>, ext: &str| {
            ev.iter().any(|e| e.starts_with("open:") && e.ends_with(ext))
        };
        assert!(opened(png.events(), ".png"), "{:?}", png.events());
        assert!(opened(gif.events(), ".gif"), "{:?}", gif.events());
        let mut off = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)));
        off.run(off.settings());
        assert!(!off.events().iter().any(|e| e.starts_with("open:")));
    }

    #[test]
    fn canceled_recording_leaves_clipboard_untouched() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Canceled);
        let o = h.run(AppSettings { format: OutputFormat::Gif, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Canceled);
        assert!(!h.events().iter().any(|e| e.starts_with("clip:")));
    }

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
    fn jpg_capture_saves_a_jpeg() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(10, 20, 30, 40)));
        let o = h.run(AppSettings { format: OutputFormat::Jpg, jpeg_quality: 80, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let path = o.path.unwrap();
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("jpg"));
        let img = image::open(&path).unwrap();
        assert_eq!((img.width(), img.height()), (30, 40));
        assert!(h.events().contains(&"clip:image:10,20,30,40".to_string()));
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
            maximized: false,
            bounds: PixelRect::new(0, 0, 50, 50),
        };
        let mut h =
            CoordinatorHarness::with_windows(Some(PixelRect::new(0, 0, 50, 50)), vec![cand]);
        h.run(AppSettings { area: CaptureArea::Window, ..h.settings() });
        assert!(h.events().contains(&"select:Window:frozen=true:windows=1".to_string()));
    }

    #[test]
    fn window_recording_selects_a_window() {
        let cand = WindowCandidate {
            id: 1,
            pid: 2,
            title: "x".into(),
            maximized: false,
            bounds: PixelRect::new(0, 0, 200, 100),
        };
        let mut h =
            CoordinatorHarness::with_windows(Some(PixelRect::new(0, 0, 200, 100)), vec![cand])
                .recording(RecordingOutcome::Saved);
        let o = h.run(AppSettings {
            area: CaptureArea::Window,
            format: OutputFormat::Gif,
            ..h.settings()
        });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let ev = h.events();
        assert!(ev.contains(&"select:Window:frozen=true:windows=1".to_string()), "{ev:?}");
        assert!(ev.contains(&"record:0,0,200,100:gif:fps=10:max=15".to_string()), "{ev:?}");
    }

    #[test]
    fn fullscreen_recording_with_two_monitors_asks_which() {
        let left = PixelRect::new(-1920, 0, 1920, 1080);
        let mut h = CoordinatorHarness::two_monitors_with_selection(
            PixelPoint { x: 10, y: 10 },
            Some(left),
        )
        .recording(RecordingOutcome::Saved);
        let o = h.run(AppSettings {
            area: CaptureArea::FullScreen,
            format: OutputFormat::Mp4,
            ..h.settings()
        });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let ev = h.events();
        assert!(ev.contains(&"select:FullScreen:frozen=true:windows=0".to_string()), "{ev:?}");
        assert!(ev.contains(&"record:-1920,0,1920,1080:mp4:fps=30:max=300".to_string()), "{ev:?}");
    }

    #[test]
    fn fullscreen_recording_with_one_monitor_uses_it() {
        let mut h = CoordinatorHarness::rect(None).recording(RecordingOutcome::Saved);
        h.run(AppSettings {
            area: CaptureArea::FullScreen,
            format: OutputFormat::Mp4,
            ..h.settings()
        });
        let ev = h.events();
        assert!(ev.contains(&"record:0,0,1920,1080:mp4:fps=30:max=300".to_string()), "{ev:?}");
        assert!(!ev.iter().any(|e| e.starts_with("select") || e.starts_with("grab")), "{ev:?}");
    }

    #[test]
    fn window_recording_is_clamped_to_screen() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(-8, -8, 1937, 1097)))
            .recording(RecordingOutcome::Saved);
        h.run(AppSettings { area: CaptureArea::Window, format: OutputFormat::Mp4, ..h.settings() });
        let ev = h.events();
        assert!(ev.contains(&"record:0,0,1920,1080:mp4:fps=30:max=300".to_string()), "{ev:?}");
    }

    #[test]
    fn fullscreen_with_one_monitor_does_not_ask() {
        let mut h = CoordinatorHarness::rect(None);
        let o = h.run(AppSettings { area: CaptureArea::FullScreen, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        assert!(h.events().contains(&"grab:0,0,1920,1080".to_string()));
        assert!(!h.events().iter().any(|e| e.starts_with("select")));
    }

    #[test]
    fn fullscreen_with_two_monitors_asks_which() {
        let left = PixelRect::new(-1920, 0, 1920, 1080);
        let mut h = CoordinatorHarness::two_monitors_with_selection(
            PixelPoint { x: 10, y: 10 },
            Some(left),
        );
        let o = h.run(AppSettings { area: CaptureArea::FullScreen, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let img = image::open(o.path.as_ref().unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (1920, 1080));
        let ev = h.events();
        assert!(ev.contains(&"grab:-1920,0,3840,1080".to_string()), "{ev:?}");
        assert!(ev.contains(&"select:FullScreen:frozen=true:windows=0".to_string()), "{ev:?}");
        assert!(ev.contains(&"clip:image:-1920,0,1920,1080".to_string()), "{ev:?}");
    }

    #[test]
    fn cancelled_monitor_pick_saves_nothing() {
        let mut h = CoordinatorHarness::two_monitors_with_selection(PixelPoint::default(), None);
        let o = h.run(AppSettings { area: CaptureArea::FullScreen, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Canceled);
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
