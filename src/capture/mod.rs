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
    Mp4,
    Gif { max_long_side: u32 },
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
}

pub trait RecordingUi: Send {
    /// Shows the recording controls and blocks until the recording ends.
    fn record(&mut self, job: RecordingJob) -> RecordingResult;
}

/// What the selection UI needs; owned so it can cross to the UI thread.
#[derive(Clone, Debug)]
pub struct SelectionRequest {
    pub area: CaptureArea,
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

pub trait SoundPlayer: Send {
    fn play_capture(&self);
}

pub trait ToolbarVisibility: Send {
    fn hide(&mut self);
    fn show(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOutcomeKind {
    Saved,
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

/// The whole rectangle of the topmost window under `at`, even where it
/// extends onto other monitors.
pub fn hovered_window(z_ordered: &[WindowCandidate], at: PixelPoint) -> Option<PixelRect> {
    window_at(z_ordered, at).map(|w| w.bounds).filter(|r| !r.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};

    fn w(id: u64, x: i32, y: i32, wd: i32, h: i32) -> WindowCandidate {
        WindowCandidate { id, pid: 1, title: format!("w{id}"), bounds: PixelRect::new(x, y, wd, h) }
    }

    #[test]
    fn topmost_window_wins_on_overlap() {
        let ws = [w(1, 0, 0, 100, 100), w(2, 50, 50, 100, 100)];
        assert_eq!(window_at(&ws, PixelPoint { x: 60, y: 60 }).unwrap().id, 1);
    }

    #[test]
    fn hovered_window_returns_the_whole_window_across_monitors() {
        let ws = [w(1, -200, 100, 600, 400)];
        assert_eq!(
            hovered_window(&ws, PixelPoint { x: 50, y: 200 }),
            Some(PixelRect::new(-200, 100, 600, 400))
        );
        assert_eq!(hovered_window(&ws, PixelPoint { x: 900, y: 900 }), None);
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
