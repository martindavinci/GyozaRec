//! Video and GIF recording: frame loop, worker thread, sinks.

mod capture_loop;
mod worker;

pub use capture_loop::run_capture_loop;
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
