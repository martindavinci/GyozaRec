//! Runs one capture end to end: hide toolbar, wait, grab, select, save.

use super::{
    AudioStarter, CaptureOutcome, ClipboardService, Cues, CursorLocator, EditorLauncher,
    FileRevealer, MonitorProvider, Notifier, RecordingJob, RecordingUi, ScreenGrabber, Selection,
    SelectionRequest, SelectionUi, SinkFactory, SinkKind, SoundPlayer, ToolbarVisibility,
    WindowSnapshotter, monitor_from_point,
};
use crate::clock::Clock;
use crate::geometry::{CaptureArea, OutputFormat, meets_minimum};
use crate::output::{FileNamer, ensure_folder, save_image};
use crate::pixels::PixelBuffer;
use crate::presentation::{Notice, NoticeKind, thumbnail};
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
    pub editor: Box<dyn EditorLauncher>,
    pub notifier: Box<dyn Notifier>,
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
        let area = settings.area.for_format(settings.format);
        let outcome = self.capture_hidden(&AppSettings { area, ..settings.clone() });
        self.toolbar.show();
        outcome
    }

    fn capture_hidden(&mut self, settings: &AppSettings) -> CaptureOutcome {
        match (settings.format, settings.area) {
            // The bar can switch to any area, so even full screen is selected.
            (OutputFormat::Png | OutputFormat::Jpg, _) if settings.area_switcher => {
                self.capture_region(settings)
            }
            (OutputFormat::Png | OutputFormat::Jpg, CaptureArea::FullScreen) => {
                self.capture_fullscreen(settings)
            }
            (OutputFormat::Png | OutputFormat::Jpg, CaptureArea::AllMonitors) => {
                match self.grabber.grab(self.monitors.virtual_screen(), false) {
                    Ok(img) => self.finish_image(&img, settings),
                    Err(err) => CaptureOutcome::failed(err, false),
                }
            }
            (OutputFormat::Png | OutputFormat::Jpg, _) => self.capture_region(settings),
            (OutputFormat::Mp4 | OutputFormat::Gif, _) => self.capture_recording(settings),
        }
    }

    /// Freezes the desktop and lets the user pick a region on it.
    fn select_on_frozen(
        &mut self,
        settings: &AppSettings,
    ) -> Result<(Arc<PixelBuffer>, Selection), CaptureOutcome> {
        let frozen = match self.grabber.grab(self.monitors.virtual_screen(), false) {
            Ok(frozen) => Arc::new(frozen),
            Err(err) => return Err(CaptureOutcome::failed(err, false)),
        };
        let switcher = settings.area_switcher;
        let windows = if settings.area == CaptureArea::Window || switcher {
            self.windows.snapshot()
        } else {
            Vec::new()
        };
        let request = SelectionRequest {
            area: settings.area,
            switcher,
            lasso: !settings.format.is_recording(),
            frozen: Some(frozen.clone()),
            windows,
            monitors: self.monitors.monitors(),
        };
        match self.selection.select_region(request) {
            Some(picked) => Ok((frozen, picked)),
            None => Err(CaptureOutcome::canceled()),
        }
    }

    fn capture_region(&mut self, settings: &AppSettings) -> CaptureOutcome {
        match self.select_on_frozen(settings) {
            Ok((frozen, picked)) if meets_minimum(settings.format, &picked.rect) => {
                let mut img = frozen.crop(&picked.rect);
                if !picked.outline.is_empty() {
                    // PNG keeps what lies outside the lasso transparent; JPG makes it white.
                    img.cut_out(&picked.outline, settings.format == OutputFormat::Png);
                }
                self.finish_image(&img, settings)
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
        // The bar can switch to any area, so even full screen is selected.
        let at_once = match settings.area {
            _ if settings.area_switcher => None,
            CaptureArea::FullScreen if monitors.len() == 1 => Some(monitors[0].bounds),
            CaptureArea::AllMonitors => Some(self.monitors.virtual_screen()),
            _ => None,
        };
        let (picked, frozen) = match at_once {
            Some(bounds) => (bounds, None),
            None => match self.select_on_frozen(settings) {
                Ok((frozen, picked)) => (picked.rect, Some(frozen)),
                Err(outcome) => return outcome,
            },
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
        // The start sound plays once the audio runs, then the recording waits it
        // out: anything captured before the recording loop starts is discarded.
        let cues = Cues {
            play: settings.play_sound,
            guard: feed.as_ref().is_some_and(|f| !f.excludes_self),
        };
        if cues.play {
            self.sound.play(crate::sounds::Cue::Start);
            if cues.guard {
                self.clock.sleep(crate::sounds::CUE_GUARD);
            }
        }
        let namer = FileNamer::with_style(
            self.clock.clone(),
            settings.file_names,
            settings.file_name_prefix,
        );
        let path = namer.next_path(&settings.save_folder, ext);
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
        // The notification shows the screen as the recording started.
        let first_screen = if settings.show_notification {
            match &frozen {
                Some(f) => Some(thumbnail(&f.crop(&region))),
                None => self.grabber.grab(region, false).ok().map(|img| thumbnail(&img)),
            }
        } else {
            None
        };
        let home = (monitor.work_area, monitor.scale);
        let job = RecordingJob { region, monitor, path, config, sink, audio: feed, cues };
        let result = self.recording_ui.record(job);
        match (result.outcome, result.path) {
            (RecordingOutcome::Saved, Some(path)) => {
                // The recording panel already played the stop sound.
                let copied = settings.copy_to_clipboard && self.clipboard.set_file(&path);
                if settings.open_folder_after_recording {
                    self.revealer.reveal(&path);
                }
                if settings.open_file_after_capture {
                    self.revealer.open(&path);
                }
                if let Some(thumbnail) = first_screen {
                    let (work_area, scale) = home;
                    let kind = NoticeKind::Video;
                    self.notifier.notify(Notice {
                        path: path.clone(),
                        kind,
                        thumbnail,
                        work_area,
                        scale,
                    });
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
                Ok((frozen, picked)) => self.finish_image(&frozen.crop(&picked.rect), settings),
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
        if settings.edit_before_saving {
            return self.copy_and_edit(img, settings);
        }
        let namer = FileNamer::with_style(
            self.clock.clone(),
            settings.file_names,
            settings.file_name_prefix,
        );
        let saved =
            save_image(img, &settings.save_folder, &namer, settings.format, settings.jpeg_quality);
        let copied = settings.copy_to_clipboard && self.clipboard.set_image(img);
        match saved {
            Ok(path) => {
                if settings.play_sound {
                    self.sound.play(crate::sounds::Cue::Saved);
                }
                let monitors = self.monitors.monitors();
                let home = (!monitors.is_empty())
                    .then(|| monitor_from_point(&monitors, img.bounds.center()))
                    .map(|m| (m.work_area, m.scale));
                match home {
                    // The editor takes the place of the default app and the notification.
                    Some((work_area, scale)) if settings.open_editor_after_capture => {
                        let image = Arc::new(img.clone());
                        self.editor.open(image, path.clone(), true, work_area, scale);
                    }
                    _ => {
                        if settings.open_file_after_capture {
                            self.revealer.open(&path);
                        }
                        if let Some((work_area, scale)) =
                            home.filter(|_| settings.show_notification)
                        {
                            self.notifier.notify(Notice {
                                path: path.clone(),
                                kind: NoticeKind::Image,
                                thumbnail: thumbnail(img),
                                work_area,
                                scale,
                            });
                        }
                    }
                }
                CaptureOutcome::saved(path, copied)
            }
            Err(err) => CaptureOutcome::failed(err, copied),
        }
    }

    /// Copies the picture and opens it in the editor without writing a file;
    /// the editor saves it under the name the capture would have had.
    fn copy_and_edit(&mut self, img: &PixelBuffer, settings: &AppSettings) -> CaptureOutcome {
        let copied = self.clipboard.set_image(img);
        if settings.play_sound {
            self.sound.play(crate::sounds::Cue::Saved);
        }
        let monitors = self.monitors.monitors();
        if !monitors.is_empty() {
            let monitor = monitor_from_point(&monitors, img.bounds.center());
            let namer = FileNamer::with_style(
                self.clock.clone(),
                settings.file_names,
                settings.file_name_prefix,
            );
            let ext = if settings.format == OutputFormat::Jpg { "jpg" } else { "png" };
            let path = namer.next_path(&settings.save_folder, ext);
            let (work_area, scale) = (monitor.work_area, monitor.scale);
            self.editor.open(Arc::new(img.clone()), path, false, work_area, scale);
        }
        CaptureOutcome::unsaved(copied)
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
    fn start_cue_plays_before_the_recording_with_a_guard() {
        use crate::sounds::CUE_GUARD;
        let rect = Some(PixelRect::new(0, 0, 200, 100));
        let mut h = CoordinatorHarness::rect(rect).with_audio().recording(RecordingOutcome::Saved);
        h.run(AppSettings { format: OutputFormat::Mp4, ..h.settings() });
        let ev = h.events();
        let at = |name: &str| ev.iter().position(|e| e.starts_with(name)).unwrap();
        assert!(
            at("audio:start") < at("sound:start") && at("sound:start") < at("record:"),
            "{ev:?}"
        );
        assert!(h.clock.sleeps().contains(&CUE_GUARD), "{:?}", h.clock.sleeps());
    }

    #[test]
    fn start_cue_without_audio_does_not_wait() {
        use crate::sounds::CUE_GUARD;
        let rect = Some(PixelRect::new(0, 0, 200, 100));
        let mut h = CoordinatorHarness::rect(rect).recording(RecordingOutcome::Saved);
        h.run(AppSettings { format: OutputFormat::Gif, ..h.settings() });
        assert!(h.events().contains(&"sound:start".to_string()));
        assert!(!h.clock.sleeps().contains(&CUE_GUARD));
    }

    #[test]
    fn no_cues_when_in_app_sounds_off() {
        let rect = Some(PixelRect::new(0, 0, 200, 100));
        let mut h = CoordinatorHarness::rect(rect).with_audio().recording(RecordingOutcome::Saved);
        h.run(AppSettings { format: OutputFormat::Mp4, play_sound: false, ..h.settings() });
        assert!(!h.events().iter().any(|e| e.starts_with("sound")), "{:?}", h.events());
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
        assert!(!opened(png.events(), ".png"), "the editor opens instead: {:?}", png.events());
        let mut png = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)));
        png.run(AppSettings {
            open_file_after_capture: true,
            open_editor_after_capture: false,
            ..png.settings()
        });
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
                "editor:2026-10-24_12-00-00.png:30x40:work=0,0,1920,1080:scale=1",
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
    fn the_area_bar_selects_every_image_area_with_windows_at_hand() {
        let cand = WindowCandidate {
            id: 1,
            pid: 2,
            title: "x".into(),
            maximized: false,
            bounds: PixelRect::new(0, 0, 50, 50),
        };
        let mut h =
            CoordinatorHarness::with_windows(Some(PixelRect::new(0, 0, 50, 50)), vec![cand]);
        let o = h.run(AppSettings {
            area: CaptureArea::FullScreen,
            area_switcher: true,
            ..h.settings()
        });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let ev = h.events();
        assert!(ev.contains(&"select:FullScreen:frozen=true:windows=1:bar".to_string()), "{ev:?}");
    }

    #[test]
    fn recordings_offer_the_area_bar_too() {
        let mut rec = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        rec.run(AppSettings {
            area: CaptureArea::FullScreen,
            format: OutputFormat::Gif,
            area_switcher: true,
            ..rec.settings()
        });
        let ev = rec.events();
        assert!(ev.contains(&"select:FullScreen:frozen=true:windows=0:bar".to_string()), "{ev:?}");
        assert!(ev.contains(&"record:0,0,200,100:gif:fps=10:max=15".to_string()), "{ev:?}");
    }

    #[test]
    fn lasso_cuts_the_picture_out() {
        let outline = vec![
            PixelPoint { x: 10, y: 10 },
            PixelPoint { x: 49, y: 10 },
            PixelPoint { x: 10, y: 49 },
        ];
        let rect = PixelRect::new(10, 10, 40, 40);
        for (format, outside) in [(OutputFormat::Png, 0u8), (OutputFormat::Jpg, 255)] {
            let mut h = CoordinatorHarness::rect(None).lasso(rect, outline.clone());
            let o = h.run(AppSettings { area: CaptureArea::Lasso, format, ..h.settings() });
            assert_eq!(o.kind, CaptureOutcomeKind::Saved, "{format:?}");
            let img = image::open(o.path.unwrap()).unwrap().to_rgba8();
            assert_eq!(img.dimensions(), (40, 40));
            assert_eq!(img.get_pixel(5, 5).0[3], 255, "inside stays opaque");
            let corner = img.get_pixel(38, 38).0;
            match format {
                OutputFormat::Png => assert_eq!(corner[3], outside, "{corner:?}"),
                _ => assert_eq!(corner, [255, 255, 255, 255], "white outside"),
            }
        }
    }

    #[test]
    fn recordings_never_use_the_lasso() {
        let mut rec = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        rec.run(AppSettings {
            area: CaptureArea::Lasso,
            format: OutputFormat::Mp4,
            ..rec.settings()
        });
        let ev = rec.events();
        assert!(ev.contains(&"select:Rectangle:frozen=true:windows=0".to_string()), "{ev:?}");
    }

    #[test]
    fn all_monitors_takes_the_whole_desktop_at_once() {
        let mut h = CoordinatorHarness::two_monitors_cursor_at(PixelPoint { x: 10, y: 10 });
        let o = h.run(AppSettings { area: CaptureArea::AllMonitors, ..h.settings() });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        let ev = h.events();
        assert!(ev.contains(&"grab:-1920,0,3840,1080".to_string()), "{ev:?}");
        assert!(!ev.iter().any(|e| e.starts_with("select:")), "{ev:?}");

        let mut rec = CoordinatorHarness::two_monitors_cursor_at(PixelPoint { x: 10, y: 10 })
            .recording(RecordingOutcome::Saved);
        rec.run(AppSettings {
            area: CaptureArea::AllMonitors,
            format: OutputFormat::Mp4,
            ..rec.settings()
        });
        let ev = rec.events();
        assert!(ev.contains(&"record:-1920,0,3840,1080:mp4:fps=30:max=300".to_string()), "{ev:?}");
        assert!(!ev.iter().any(|e| e.starts_with("select:")), "{ev:?}");
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
        assert!(!ev.iter().any(|e| e.starts_with("select")), "{ev:?}");
        let grabs: Vec<_> = ev.iter().filter(|e| e.starts_with("grab")).collect();
        assert_eq!(grabs, ["grab:0,0,1920,1080"], "one grab, for the notification");
        let mut quiet = CoordinatorHarness::rect(None).recording(RecordingOutcome::Saved);
        quiet.run(AppSettings {
            area: CaptureArea::FullScreen,
            format: OutputFormat::Mp4,
            show_notification: false,
            ..quiet.settings()
        });
        assert!(!quiet.events().iter().any(|e| e.starts_with("grab")));
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
    fn saved_image_opens_the_editor_instead_of_the_file() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(10, 20, 200, 100)));
        h.run(AppSettings { open_file_after_capture: true, ..h.settings() });
        let ev = h.events();
        let editor = ev.iter().find(|e| e.starts_with("editor:")).expect("editor opened");
        assert!(editor.ends_with(".png:200x100:work=0,0,1920,1080:scale=1"), "{editor}");
        assert!(!ev.iter().any(|e| e.starts_with("open:")), "{ev:?}");
        assert!(ev.iter().any(|e| e.starts_with("clip:image")), "still copied: {ev:?}");
    }

    #[test]
    fn edit_before_saving_copies_and_opens_the_editor_without_a_file() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(10, 20, 30, 40)));
        let o = h.run(AppSettings {
            edit_before_saving: true,
            copy_to_clipboard: false,
            open_editor_after_capture: false,
            open_file_after_capture: true,
            ..h.settings()
        });
        assert_eq!(
            (o.kind, o.path, o.copied_to_clipboard),
            (CaptureOutcomeKind::Unsaved, None, true)
        );
        assert_eq!(fs::read_dir(h.dir()).unwrap().count(), 0, "nothing written");
        assert_eq!(
            h.events(),
            [
                "hide",
                "grab:0,0,1920,1080",
                "select:Rectangle:frozen=true:windows=0",
                "clip:image:10,20,30,40",
                "sound",
                "editor:2026-10-24_12-00-00.png:30x40:work=0,0,1920,1080:scale=1:unsaved",
                "show"
            ]
        );
        let mut jpg = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 30, 40)));
        jpg.run(AppSettings {
            edit_before_saving: true,
            format: OutputFormat::Jpg,
            ..jpg.settings()
        });
        assert!(jpg.events().iter().any(|e| e.starts_with("editor:") && e.contains(".jpg:")));
    }

    #[test]
    fn edit_before_saving_leaves_recordings_alone() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        let o = h.run(AppSettings {
            format: OutputFormat::Gif,
            edit_before_saving: true,
            ..h.settings()
        });
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        assert!(!h.events().iter().any(|e| e.starts_with("editor:")));
    }

    #[test]
    fn editor_off_keeps_the_old_behaviour() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)));
        h.run(AppSettings {
            open_file_after_capture: true,
            open_editor_after_capture: false,
            ..h.settings()
        });
        let ev = h.events();
        assert!(ev.iter().any(|e| e.starts_with("open:")), "{ev:?}");
        assert!(!ev.iter().any(|e| e.starts_with("editor:")), "{ev:?}");
    }

    #[test]
    fn recordings_never_open_the_editor() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
            .recording(RecordingOutcome::Saved);
        h.run(AppSettings { format: OutputFormat::Mp4, ..h.settings() });
        assert!(!h.events().iter().any(|e| e.starts_with("editor:")));
    }

    #[test]
    fn failed_save_does_not_open_the_editor() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 10, 10)));
        let blocker = h.dir().join("blocker");
        fs::write(&blocker, b"").unwrap();
        h.run(AppSettings { save_folder: blocker.join("sub"), ..h.settings() });
        assert!(!h.events().iter().any(|e| e.starts_with("editor:")));
    }

    #[test]
    fn image_without_editor_is_notified() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 640, 360)));
        h.run(AppSettings { open_editor_after_capture: false, ..h.settings() });
        let ev = h.events();
        let notice = ev.iter().find(|e| e.starts_with("notice:")).expect("notified");
        assert!(
            notice.starts_with("notice:image:") && notice.ends_with(".png:320x180"),
            "{notice}"
        );
    }

    #[test]
    fn image_with_editor_is_not_notified() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)));
        h.run(h.settings());
        assert!(!h.events().iter().any(|e| e.starts_with("notice:")), "{:?}", h.events());
    }

    #[test]
    fn recording_is_notified_with_its_first_screen() {
        let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 640, 480)))
            .recording(RecordingOutcome::Saved);
        h.run(AppSettings { format: OutputFormat::Mp4, ..h.settings() });
        let ev = h.events();
        let notice = ev.iter().find(|e| e.starts_with("notice:")).expect("notified");
        assert!(
            notice.starts_with("notice:video:") && notice.ends_with(".mp4:240x180"),
            "{notice}"
        );
    }

    #[test]
    fn notification_off_shows_nothing() {
        let off = |format| AppSettings {
            format,
            open_editor_after_capture: false,
            show_notification: false,
            ..AppSettings::default()
        };
        for format in [OutputFormat::Png, OutputFormat::Gif] {
            let mut h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 200, 100)))
                .recording(RecordingOutcome::Saved);
            let settings = AppSettings { save_folder: h.dir().to_path_buf(), ..off(format) };
            h.run(settings);
            assert!(!h.events().iter().any(|e| e.starts_with("notice:")), "{:?}", h.events());
        }
    }

    #[test]
    fn random_file_names_leave_out_the_app_name() {
        let rect = Some(PixelRect::new(0, 0, 200, 100));
        let mut img = CoordinatorHarness::rect(rect);
        let random =
            |s: AppSettings| AppSettings { file_names: crate::output::FileNames::Random, ..s };
        let o = img.run(random(img.settings()));
        let name = o.path.unwrap().file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            !name.starts_with("GyozaRec") && name.ends_with(".png") && name.len() == 14,
            "{name}"
        );
        let mut rec = CoordinatorHarness::rect(rect).recording(RecordingOutcome::Saved);
        let o = rec.run(random(AppSettings { format: OutputFormat::Gif, ..rec.settings() }));
        let name = o.path.unwrap().file_name().unwrap().to_string_lossy().into_owned();
        assert!(!name.starts_with("GyozaRec") && name.ends_with(".gif"), "{name}");
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
