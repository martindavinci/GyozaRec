//! Capture abstractions implemented by each platform backend.

mod coordinator;

pub use coordinator::{CaptureCoordinator, HIDE_SETTLE_DELAY, OVERLAY_CLOSE_SETTLE};

use crate::geometry::{CaptureArea, MonitorInfo, PixelPoint, PixelRect, PixelSize};
use crate::pixels::PixelBuffer;
use crate::recording::{FrameSink, LoopConfig, RecordingResult};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum SinkKind {
    /// `audio`: the file gets an audio track.
    Mp4 {
        audio: bool,
    },
    Gif {
        max_long_side: u32,
    },
}

/// Starts the sound sources of a recording; `None` when no device works.
pub trait AudioStarter: Send {
    fn start(
        &self,
        clock: std::sync::Arc<dyn crate::clock::Clock>,
    ) -> Option<crate::audio::AudioFeed>;
}

pub trait SinkFactory: Send {
    fn create(
        &self,
        kind: &SinkKind,
        path: &Path,
        size: PixelSize,
        fps: u32,
    ) -> crate::Result<Box<dyn FrameSink>>;
}

/// Everything the recording UI needs to run one recording.
pub struct RecordingJob {
    pub region: PixelRect,
    pub monitor: MonitorInfo,
    pub path: PathBuf,
    pub config: LoopConfig,
    pub sink: Box<dyn FrameSink>,
    /// Running audio sources, when the recording has sound.
    pub audio: Option<crate::audio::AudioFeed>,
    pub cues: Cues,
}

pub trait RecordingUi: Send {
    /// Shows the recording controls and blocks until the recording ends.
    fn record(&mut self, job: RecordingJob) -> RecordingResult;
}

/// What the selection UI needs; owned so it can cross to the UI thread.
#[derive(Clone, Debug)]
pub struct SelectionRequest {
    pub area: CaptureArea,
    /// Show the bar that switches between rectangle, window and full screen.
    pub switcher: bool,
    pub frozen: Option<Arc<PixelBuffer>>,
    pub windows: Vec<WindowCandidate>,
    pub monitors: Vec<MonitorInfo>,
}

pub trait SelectionUi: Send {
    /// `None` means the user canceled.
    fn select_region(&mut self, req: SelectionRequest) -> Option<PixelRect>;
}

pub trait ClipboardService: Send {
    fn set_image(&self, img: &PixelBuffer) -> bool;
    fn set_file(&self, path: &Path) -> bool;
}

pub trait FileRevealer: Send {
    /// Shows `path` in the file manager.
    fn reveal(&self, path: &Path);
    /// Opens `path` in its default app.
    fn open(&self, path: &Path);
}

/// Opens the editor on a saved image, on the monitor it came from.
pub trait EditorLauncher: Send {
    /// `on_disk` is false when `path` is only where the editor would save.
    fn open(
        &self,
        image: Arc<PixelBuffer>,
        path: PathBuf,
        on_disk: bool,
        work_area: PixelRect,
        scale: f64,
    );
}

/// Shows the notification of a saved capture.
pub trait Notifier: Send {
    fn notify(&self, notice: crate::presentation::Notice);
}

pub trait SoundPlayer: Send {
    fn play(&self, cue: crate::sounds::Cue);
}

/// Recording sounds: whether to play them, and whether audio being recorded
/// means waiting around them so they stay out of the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cues {
    pub play: bool,
    pub guard: bool,
}

pub trait ToolbarVisibility: Send {
    fn hide(&mut self);
    fn show(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOutcomeKind {
    Saved,
    /// Copied and handed to the editor, not written to a file.
    Unsaved,
    Canceled,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureOutcome {
    pub kind: CaptureOutcomeKind,
    pub path: Option<PathBuf>,
    pub error: Option<crate::Failure>,
    pub copied_to_clipboard: bool,
}

impl CaptureOutcome {
    pub fn canceled() -> Self {
        Self {
            kind: CaptureOutcomeKind::Canceled,
            path: None,
            error: None,
            copied_to_clipboard: false,
        }
    }

    pub fn failed(error: impl Into<crate::Failure>, copied_to_clipboard: bool) -> Self {
        Self {
            kind: CaptureOutcomeKind::Failed,
            path: None,
            error: Some(error.into()),
            copied_to_clipboard,
        }
    }

    pub fn saved(path: PathBuf, copied_to_clipboard: bool) -> Self {
        Self { kind: CaptureOutcomeKind::Saved, path: Some(path), error: None, copied_to_clipboard }
    }

    pub fn unsaved(copied_to_clipboard: bool) -> Self {
        Self { kind: CaptureOutcomeKind::Unsaved, path: None, error: None, copied_to_clipboard }
    }
}

pub trait MonitorProvider: Send {
    fn monitors(&self) -> Vec<MonitorInfo>;
    /// Union of all monitor bounds.
    fn virtual_screen(&self) -> PixelRect;
}

pub trait ScreenGrabber: Send + Sync {
    fn grab(&self, rect: PixelRect, include_cursor: bool) -> crate::Result<PixelBuffer>;
}

pub trait CursorLocator: Send {
    fn position(&self) -> PixelPoint;
}

/// A top-level window that Window mode can pick.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowCandidate {
    pub id: u64,
    pub pid: u32,
    pub title: String,
    pub bounds: PixelRect,
    /// Maximized windows can report a frame that spills onto the next monitor.
    pub maximized: bool,
}

pub trait WindowSnapshotter: Send {
    /// Visible top-level windows, topmost first.
    fn snapshot(&self) -> Vec<WindowCandidate>;
}

/// The monitor containing `p`, or the one whose center is nearest.
pub fn monitor_from_point(monitors: &[MonitorInfo], p: PixelPoint) -> &MonitorInfo {
    monitors.iter().find(|m| m.bounds.contains(p)).unwrap_or_else(|| {
        monitors
            .iter()
            .min_by_key(|m| {
                let c = m.bounds.center();
                let (dx, dy) = (i64::from(c.x - p.x), i64::from(c.y - p.y));
                dx * dx + dy * dy
            })
            .expect("at least one monitor")
    })
}

/// Topmost window containing `p`.
pub fn window_at(z_ordered: &[WindowCandidate], p: PixelPoint) -> Option<&WindowCandidate> {
    z_ordered.iter().find(|w| w.bounds.contains(p))
}

/// The monitor containing `p`, if any (monitors may leave gaps).
pub fn monitor_at(monitors: &[PixelRect], p: PixelPoint) -> Option<PixelRect> {
    monitors.iter().find(|m| m.contains(p)).copied()
}

/// Click-through (`WS_EX_TRANSPARENT`) and cloaked windows are invisible to
/// the user, so they are never offered as a pick.
pub fn is_capturable(ex_style: u32, cloaked: bool) -> bool {
    const WS_EX_TRANSPARENT: u32 = 0x20;
    !cloaked && ex_style & WS_EX_TRANSPARENT == 0
}

/// The topmost window under `at`, as much of it as the monitors show: the
/// whole window when it spans several monitors, without the parts no monitor
/// covers. A maximized window keeps to the monitor it is maximized on.
pub fn hovered_window(
    z_ordered: &[WindowCandidate],
    monitors: &[PixelRect],
    at: PixelPoint,
) -> Option<PixelRect> {
    let w = window_at(z_ordered, at)?;
    let visible = monitors.iter().map(|m| w.bounds.intersect(m)).filter(|r| !r.is_empty());
    let area = if w.maximized {
        let center = w.bounds.center();
        monitors.iter().find(|m| m.contains(center)).map(|m| w.bounds.intersect(m))?
    } else {
        visible.reduce(|a, b| {
            let (x, y) = (a.x.min(b.x), a.y.min(b.y));
            PixelRect::new(x, y, a.right().max(b.right()) - x, a.bottom().max(b.bottom()) - y)
        })?
    };
    Some(area).filter(|r| !r.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};

    fn w(id: u64, x: i32, y: i32, wd: i32, h: i32) -> WindowCandidate {
        WindowCandidate {
            id,
            pid: 1,
            title: format!("w{id}"),
            bounds: PixelRect::new(x, y, wd, h),
            maximized: false,
        }
    }

    #[test]
    fn topmost_window_wins_on_overlap() {
        let ws = [w(1, 0, 0, 100, 100), w(2, 50, 50, 100, 100)];
        assert_eq!(window_at(&ws, PixelPoint { x: 60, y: 60 }).unwrap().id, 1);
    }

    #[test]
    fn hovered_window_returns_the_whole_window_across_monitors() {
        let monitors = [PixelRect::new(-1920, 0, 1920, 1080), PixelRect::new(0, 0, 1920, 1080)];
        let ws = [w(1, -200, 100, 600, 400)];
        assert_eq!(
            hovered_window(&ws, &monitors, PixelPoint { x: 50, y: 200 }),
            Some(PixelRect::new(-200, 100, 600, 400))
        );
        assert_eq!(hovered_window(&ws, &monitors, PixelPoint { x: 900, y: 900 }), None);
    }

    #[test]
    fn hovered_window_leaves_out_areas_no_monitor_shows() {
        // 1080p laptop left of a taller 1440p monitor: below the laptop there is no screen.
        let monitors = [PixelRect::new(0, 0, 1920, 1080), PixelRect::new(1920, 0, 2560, 1440)];
        let ws = [w(1, 100, 900, 600, 400)];
        assert_eq!(
            hovered_window(&ws, &monitors, PixelPoint { x: 300, y: 1000 }),
            Some(PixelRect::new(100, 900, 600, 180))
        );
    }

    #[test]
    fn maximized_window_stays_on_its_own_monitor() {
        let monitors = [PixelRect::new(0, 0, 1920, 1080), PixelRect::new(1920, 0, 1920, 1080)];
        let ws = [WindowCandidate { maximized: true, ..w(1, -8, -8, 1936, 1096) }];
        assert_eq!(
            hovered_window(&ws, &monitors, PixelPoint { x: 500, y: 500 }),
            Some(PixelRect::new(0, 0, 1920, 1080))
        );
    }

    #[test]
    fn transparent_or_cloaked_windows_are_not_capturable() {
        const WS_EX_TRANSPARENT: u32 = 0x20;
        const WS_EX_LAYERED: u32 = 0x80000;
        assert!(is_capturable(0, false));
        assert!(is_capturable(WS_EX_LAYERED, false));
        assert!(!is_capturable(WS_EX_TRANSPARENT | WS_EX_LAYERED, false));
        assert!(!is_capturable(0, true));
    }

    #[test]
    fn monitor_at_handles_negative_origins_and_gaps() {
        let monitors = [PixelRect::new(-1920, 0, 1920, 1080), PixelRect::new(0, 200, 2560, 1440)];
        assert_eq!(monitor_at(&monitors, PixelPoint { x: -1, y: 5 }), Some(monitors[0]));
        assert_eq!(monitor_at(&monitors, PixelPoint { x: 10, y: 300 }), Some(monitors[1]));
        assert_eq!(monitor_at(&monitors, PixelPoint { x: 10, y: 100 }), None, "gap above B");
    }

    #[test]
    fn point_outside_all_windows_returns_none() {
        assert!(window_at(&[w(1, 0, 0, 10, 10)], PixelPoint { x: 500, y: 500 }).is_none());
    }

    #[test]
    fn monitor_from_point_picks_negative_origin() {
        let ms = [
            MonitorInfo {
                device_name: "A".into(),
                bounds: PixelRect::new(0, 0, 1920, 1080),
                work_area: PixelRect::new(0, 0, 1920, 1080),
                scale: 1.0,
            },
            MonitorInfo {
                device_name: "B".into(),
                bounds: PixelRect::new(-1920, 0, 1920, 1080),
                work_area: PixelRect::new(-1920, 0, 1920, 1080),
                scale: 1.5,
            },
        ];
        assert_eq!(monitor_from_point(&ms, PixelPoint { x: -10, y: 500 }).device_name, "B");
    }
}
