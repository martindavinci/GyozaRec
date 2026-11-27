// SPDX-License-Identifier: GPL-3.0-or-later
//! Video and GIF recording: frame loop, worker thread, sinks.

mod capture_loop;
mod gif_sink;
mod panel;
mod placement;
mod split;
mod worker;

pub use capture_loop::{run_capture_loop, run_capture_loop_with_audio};
pub use gif_sink::GifSink;
pub use panel::{PanelAction, PanelInput, PanelMode, panel_step};
pub use placement::{frame_bounds, place_panel, render_frame};
pub use split::{OpenPart, PartSink, PathTaken, SplitSink, part_path};
pub use worker::RecordingWorker;

use crate::geometry::PixelRect;
use crate::pixels::PixelBuffer;
use std::path::PathBuf;
use std::time::Duration;

/// Receives frames with their time since the recording started.
pub trait FrameSink: Send {
    fn push(&mut self, frame: &PixelBuffer, t: Duration) -> crate::Result<()>;
    fn finish(&mut self) -> crate::Result<()>;
    /// Interleaved stereo 48 kHz audio starting at `start_frame` of the
    /// recording; never empty. Sinks without audio ignore it.
    fn push_audio(&mut self, _samples: &[i16], _start_frame: u64) -> crate::Result<()> {
        Ok(())
    }
    /// Every file written, in order, when the sink splits the recording;
    /// empty for a sink that writes only the file it was created with.
    fn files(&self) -> Vec<PathBuf> {
        Vec::new()
    }
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
pub const PAUSE: u8 = 3;

/// Keeps paused stretches out of a recording's running time.
#[derive(Clone, Copy, Debug, Default)]
pub struct PauseClock {
    paused_total: Duration,
    since: Option<Duration>,
}

impl PauseClock {
    pub fn pause(&mut self, now: Duration) {
        self.since.get_or_insert(now);
    }

    pub fn resume(&mut self, now: Duration) {
        if let Some(since) = self.since.take() {
            self.paused_total += now.saturating_sub(since);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.since.is_some()
    }

    /// Time between `start` and `now`, without the paused parts.
    pub fn active(&self, start: Duration, now: Duration) -> Duration {
        let current = self.since.map_or(Duration::ZERO, |since| now.saturating_sub(since));
        now.saturating_sub(start).saturating_sub(self.paused_total + current)
    }
}

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
    /// The first (or only) file of a saved recording.
    pub path: Option<PathBuf>,
    pub error: Option<crate::Failure>,
    /// Every file of a saved recording, in order; `path` is the first.
    pub parts: Vec<PathBuf>,
}

impl RecordingResult {
    pub fn canceled() -> RecordingResult {
        RecordingResult {
            outcome: RecordingOutcome::Canceled,
            path: None,
            error: None,
            parts: Vec::new(),
        }
    }

    pub fn failed(error: crate::Failure) -> RecordingResult {
        RecordingResult {
            outcome: RecordingOutcome::Failed,
            path: None,
            error: Some(error),
            parts: Vec::new(),
        }
    }

    /// A saved recording made of `parts` (never empty: the first is `path`).
    pub fn saved(parts: Vec<PathBuf>) -> RecordingResult {
        RecordingResult {
            outcome: RecordingOutcome::Saved,
            path: parts.first().cloned(),
            error: None,
            parts,
        }
    }
}

/// H.264 bitrate: width × height × fps / 10, within 1–24 Mbit/s.
pub fn bitrate(size: crate::geometry::PixelSize, fps: u32) -> u32 {
    let raw =
        u64::from(size.width.max(0) as u32) * u64::from(size.height.max(0) as u32) * u64::from(fps)
            / 10;
    raw.clamp(1_000_000, 24_000_000) as u32
}

/// AAC bitrate of the audio track.
pub const AUDIO_BITRATE: u32 = 192_000;

/// Bytes kept free under a part's size `limit`: about two seconds of the
/// stream (what the encoder and the file writer still hold) plus room for
/// the index written when the file closes. Never more than half the limit,
/// so a small limit with a fast stream cannot make a file per frame.
pub fn split_reserve(limit: u64, video_bitrate: u32, audio: bool) -> u64 {
    let bits = u64::from(video_bitrate) + if audio { u64::from(AUDIO_BITRATE) } else { 0 };
    (bits / 8 * 2 + 512 * 1024).min(limit / 2)
}

/// `mb` megabytes in bytes, counted as Windows Explorer does (1 MB = 1,048,576 bytes).
pub fn megabytes(mb: u32) -> u64 {
    u64::from(mb) * 1024 * 1024
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelSize;

    #[test]
    fn pause_clock_excludes_paused_time() {
        let s = |v: u64| Duration::from_secs(v);
        let mut p = PauseClock::default();
        assert_eq!(p.active(s(10), s(13)), s(3));
        p.pause(s(13));
        p.pause(s(14)); // already paused: keeps the first start
        assert!(p.is_paused());
        assert_eq!(p.active(s(10), s(20)), s(3));
        p.resume(s(20));
        assert_eq!(p.active(s(10), s(22)), s(5));
        p.resume(s(23)); // not paused: no effect
        assert_eq!(p.active(s(10), s(24)), s(7));
    }

    #[test]
    fn bitrate_is_clamped() {
        assert_eq!(bitrate(PixelSize { width: 320, height: 240 }, 30), 1_000_000);
        assert_eq!(bitrate(PixelSize { width: 1920, height: 1080 }, 30), 6_220_800);
        assert_eq!(bitrate(PixelSize { width: 3840, height: 2160 }, 60), 24_000_000);
    }

    #[test]
    fn split_reserve_is_two_seconds_of_stream_and_the_index() {
        let limit = megabytes(20);
        assert_eq!(split_reserve(limit, 6_220_800, false), 1_555_200 + 524_288);
        assert_eq!(split_reserve(limit, 6_220_800, true), 1_603_200 + 524_288);
        // Never more than half the limit, so a part always takes many frames.
        assert_eq!(split_reserve(megabytes(10), 24_000_000, true), megabytes(5));
    }

    #[test]
    fn megabytes_are_binary() {
        assert_eq!(megabytes(20), 20_971_520);
    }
}
