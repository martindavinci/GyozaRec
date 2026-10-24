//! Streams frames into an animated GIF while recording.

use super::FrameSink;
use crate::geometry::{PixelSize, fit};
use crate::pixels::PixelBuffer;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Duration;

/// Shortest frame delay in centiseconds; browsers slow down anything below.
const MIN_DELAY_CS: u16 = 2;
/// NeuQuant speed: 1 is best quality, 30 fastest.
const QUANTIZE_SPEED: i32 = 10;

/// Each frame is held until the next one arrives, so its delay comes from
/// the real gap between timestamps.
pub struct GifSink {
    encoder: Option<gif::Encoder<BufWriter<File>>>,
    size: PixelSize,
    fps: u32,
    pending: Option<(Vec<u8>, Duration)>,
}

impl GifSink {
    pub fn create(
        path: &Path,
        source: PixelSize,
        max_long_side: u32,
        fps: u32,
    ) -> crate::Result<GifSink> {
        let size = fit(source, max_long_side.min(i32::MAX as u32) as i32);
        let file = BufWriter::new(File::create(path)?);
        let mut encoder = gif::Encoder::new(file, size.width as u16, size.height as u16, &[])?;
        encoder.set_repeat(gif::Repeat::Infinite)?;
        Ok(GifSink { encoder: Some(encoder), size, fps, pending: None })
    }

    fn write(&mut self, mut rgba: Vec<u8>, delay_cs: u16) -> crate::Result<()> {
        let encoder = self.encoder.as_mut().ok_or_else(closed)?;
        let (w, h) = (self.size.width as u16, self.size.height as u16);
        let mut frame = gif::Frame::from_rgba_speed(w, h, &mut rgba, QUANTIZE_SPEED);
        frame.delay = delay_cs;
        encoder.write_frame(&frame)?;
        Ok(())
    }
}

fn closed() -> crate::Error {
    crate::Error::Message("GIF encoder already closed".into())
}

fn centiseconds(d: Duration) -> u16 {
    ((d.as_millis() + 5) / 10).clamp(u128::from(MIN_DELAY_CS), u128::from(u16::MAX)) as u16
}

impl FrameSink for GifSink {
    fn push(&mut self, frame: &PixelBuffer, t: Duration) -> crate::Result<()> {
        let rgba = if frame.bounds.size() == self.size {
            frame.to_rgba()
        } else {
            frame.resized(self.size).to_rgba()
        };
        if let Some((prev, prev_t)) = self.pending.replace((rgba, t)) {
            self.write(prev, centiseconds(t.saturating_sub(prev_t)))?;
        }
        Ok(())
    }

    fn finish(&mut self) -> crate::Result<()> {
        let Some((last, _)) = self.pending.take() else {
            return Err(crate::UserError::NoFrames.into());
        };
        let nominal = Duration::from_secs(1) / self.fps.max(1);
        self.write(last, centiseconds(nominal))?;
        let encoder = self.encoder.take().ok_or_else(closed)?;
        encoder.into_inner()?.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PixelRect, PixelSize};
    use crate::pixels::PixelBuffer;
    use crate::recording::FrameSink;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    fn frames(path: &Path) -> (u16, u16, Vec<u16>, gif::Repeat) {
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts.read_info(fs::File::open(path).unwrap()).unwrap();
        let mut delays = vec![];
        while let Some(f) = dec.read_next_frame().unwrap() {
            delays.push(f.delay);
        }
        (dec.width(), dec.height(), delays, dec.repeat())
    }

    fn solid(w: i32, h: i32) -> PixelBuffer {
        PixelBuffer::solid(PixelRect::new(0, 0, w, h), 10, 200, 30)
    }

    #[test]
    fn delays_follow_real_timestamps() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let mut s = GifSink::create(&p, PixelSize { width: 40, height: 30 }, 800, 10).unwrap();
        for t in [0, 100, 250] {
            s.push(&solid(40, 30), Duration::from_millis(t)).unwrap();
        }
        s.finish().unwrap();
        assert_eq!(frames(&p), (40, 30, vec![10, 15, 10], gif::Repeat::Infinite));
    }

    #[test]
    fn downscales_to_max_long_side() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let mut s = GifSink::create(&p, PixelSize { width: 1600, height: 900 }, 800, 10).unwrap();
        s.push(&solid(1600, 900), Duration::ZERO).unwrap();
        s.finish().unwrap();
        let (w, h, ..) = frames(&p);
        assert_eq!((w, h), (800, 450));
    }

    #[test]
    fn very_short_delay_is_clamped_to_two_centiseconds() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let mut s = GifSink::create(&p, PixelSize { width: 20, height: 20 }, 800, 10).unwrap();
        s.push(&solid(20, 20), Duration::ZERO).unwrap();
        s.push(&solid(20, 20), Duration::from_millis(5)).unwrap();
        s.finish().unwrap();
        assert_eq!(frames(&p).2, vec![2, 10]);
    }

    #[test]
    fn finish_without_frames_is_error() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let mut s = GifSink::create(&p, PixelSize { width: 20, height: 20 }, 800, 10).unwrap();
        assert!(s.finish().is_err());
    }
}
