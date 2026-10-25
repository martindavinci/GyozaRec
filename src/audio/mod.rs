//! Audio for recordings: captured chunks, stereo downmix and the timeline
//! that turns them into a contiguous 48 kHz track aligned with the video.

mod downmix;
mod timeline;

pub use downmix::to_stereo;
pub use timeline::{AudioOut, PauseHistory, Timeline};

use std::sync::mpsc::Receiver;
use std::time::Duration;

/// Output sample rate of every recording's audio track.
pub const OUT_RATE: u32 = 48_000;

/// How long audio waits before it is committed, so late packets still land.
pub const LATENCY: Duration = Duration::from_millis(300);

/// One packet from an audio device.
#[derive(Clone, Debug, PartialEq)]
pub struct PcmChunk {
    /// Which source it comes from; sources are mixed, packets of one source
    /// never overlap each other.
    pub source: u8,
    pub rate: u32,
    pub channels: u16,
    /// Speaker positions (`WAVEFORMATEXTENSIBLE` bits); 0 = default order.
    pub channel_mask: u32,
    /// Interleaved, -1.0..1.0.
    pub samples: Vec<f32>,
    /// When its first sample was captured, on the recording clock.
    pub at: Duration,
    pub discontinuity: bool,
}

/// Chunks from running audio sources, and a way to stop them.
pub struct AudioFeed {
    pub chunks: Receiver<PcmChunk>,
    stop: Option<Box<dyn FnOnce() + Send>>,
    /// The source leaves out GyozaRec's own sounds.
    pub excludes_self: bool,
}

impl AudioFeed {
    pub fn new(
        chunks: Receiver<PcmChunk>,
        stop: Box<dyn FnOnce() + Send>,
        excludes_self: bool,
    ) -> AudioFeed {
        AudioFeed { chunks, stop: Some(stop), excludes_self }
    }

    /// Stops the sources and waits for them; later chunks are already queued.
    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

impl Drop for AudioFeed {
    fn drop(&mut self) {
        self.stop();
    }
}
