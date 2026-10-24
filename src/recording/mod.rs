//! Video and GIF recording: frame loop, worker thread, sinks.

mod capture_loop;
mod gif_sink;
mod placement;
mod worker;

pub use capture_loop::run_capture_loop;
pub use gif_sink::GifSink;
pub use placement::{frame_bounds, place_panel, render_frame};
pub use worker::RecordingWorker;

use crate::geometry::PixelRect;
use crate::pixels::PixelBuffer;
use std::path::PathBuf;
use std::time::Duration;

/// Receives frames with their time since the recording started.
pub trait FrameSink: Send {
    fn push(&mut self, frame: &PixelBuffer, t: Duration) -> crate::Result<()>;
    fn finish(&mut self) -> crate::Result<()>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoopConfig {
    pub region: PixelRect,
    pub fps: u32,
    pub include_cursor: bool,
    pub max_duration: Duration,
}

/// Values of the loop's control flag.
pub const RUN: u8 = 0;
pub const STOP: u8 = 1;
pub const CANCEL: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopEnd {
    Stopped,
    MaxDuration,
    Canceled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingOutcome {
    Saved,
    Canceled,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordingResult {
    pub outcome: RecordingOutcome,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
}

/// H.264 bitrate: width × height × fps / 10, within 1–24 Mbit/s.
pub fn bitrate(size: crate::geometry::PixelSize, fps: u32) -> u32 {
    let raw =
        u64::from(size.width.max(0) as u32) * u64::from(size.height.max(0) as u32) * u64::from(fps)
            / 10;
    raw.clamp(1_000_000, 24_000_000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelSize;

    #[test]
    fn bitrate_is_clamped() {
        assert_eq!(bitrate(PixelSize { width: 320, height: 240 }, 30), 1_000_000);
        assert_eq!(bitrate(PixelSize { width: 1920, height: 1080 }, 30), 6_220_800);
        assert_eq!(bitrate(PixelSize { width: 3840, height: 2160 }, 60), 24_000_000);
    }
}
