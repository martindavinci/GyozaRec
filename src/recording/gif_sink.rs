//! Streams frames into an animated GIF while recording. Colour quantizing,
//! the slow part, runs on its own thread so the capture keeps its frame rate.

use super::FrameSink;
use crate::geometry::{PixelSize, fit};
use crate::pixels::PixelBuffer;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

/// Shortest frame delay in centiseconds; browsers slow down anything below.
const MIN_DELAY_CS: u16 = 2;
/// NeuQuant speed: 1 is best quality, 30 fastest.
const QUANTIZE_SPEED: i32 = 10;
/// Frames waiting for the encoder; a full queue makes `push` wait.
const QUEUE: usize = 4;

type Writer = Box<dyn Write + Send>;

/// Each frame is held until the next one arrives, so its delay comes from
/// the real gap between timestamps.
pub struct GifSink {
    size: PixelSize,
    fps: u32,
    pending: Option<(Vec<u8>, Duration)>,
    queue: Option<SyncSender<(Vec<u8>, u16)>>,
    encoder: Option<JoinHandle<crate::Result<()>>>,
}

impl GifSink {
    pub fn create(
        path: &Path,
        source: PixelSize,
        max_long_side: u32,
        fps: u32,
    ) -> crate::Result<GifSink> {
        let file = BufWriter::new(File::create(path)?);
        GifSink::with_writer(Box::new(file), source, max_long_side, fps)
    }

    pub fn with_writer(
        writer: Writer,
        source: PixelSize,
        max_long_side: u32,
        fps: u32,
    ) -> crate::Result<GifSink> {
        let size = fit(source, max_long_side.min(i32::MAX as u32) as i32);
        let mut encoder = gif::Encoder::new(writer, size.width as u16, size.height as u16, &[])?;
        encoder.set_repeat(gif::Repeat::Infinite)?;
        let (queue, frames) = sync_channel::<(Vec<u8>, u16)>(QUEUE);
        let (w, h) = (size.width as u16, size.height as u16);
        let handle = std::thread::Builder::new().name("gyozarec-gif".into()).spawn(
            move || -> crate::Result<()> {
                for (mut rgba, delay) in frames {
                    let mut frame = gif::Frame::from_rgba_speed(w, h, &mut rgba, QUANTIZE_SPEED);
                    frame.delay = delay;
                    encoder.write_frame(&frame)?;
                }
                // The queue closed: the recording ended (or was dropped).
                encoder.into_inner()?.flush()?;
                Ok(())
            },
        )?;
        Ok(GifSink { size, fps, pending: None, queue: Some(queue), encoder: Some(handle) })
    }

    fn send(&mut self, rgba: Vec<u8>, delay_cs: u16) -> crate::Result<()> {
        let queue = self.queue.as_ref().ok_or_else(closed)?;
        if queue.send((rgba, delay_cs)).is_err() {
            // The encoder stopped early; report why.
            return Err(self.join().err().unwrap_or_else(closed));
        }
        Ok(())
    }

    /// Closes the queue and waits for the encoder, returning its result.
    fn join(&mut self) -> crate::Result<()> {
        self.queue = None;
        match self.encoder.take() {
            Some(handle) => handle.join().unwrap_or_else(|_| Err(closed())),
            None => Err(closed()),
        }
    }
}

/// A cancelled recording drops the sink without `finish`: the encoder still
/// has to let go of the file before it is deleted.
impl Drop for GifSink {
    fn drop(&mut self) {
        let _ = self.join();
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
            self.send(prev, centiseconds(t.saturating_sub(prev_t)))?;
        }
        Ok(())
    }

    fn finish(&mut self) -> crate::Result<()> {
        let Some((last, _)) = self.pending.take() else {
            return Err(crate::UserError::NoFrames.into());
        };
        let nominal = Duration::from_secs(1) / self.fps.max(1);
        self.send(last, centiseconds(nominal))?;
        self.join()
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

    /// Accepts `budget` bytes, then fails every write.
    struct FailingWriter {
        budget: usize,
    }

    impl Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if buf.len() > self.budget {
                return Err(std::io::Error::other("disk full"));
            }
            self.budget -= buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Collects bytes slowly, like a busy disk.
    struct SlowWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for SlowWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(5));
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn gif_encoder_error_surfaces() {
        let w = Box::new(FailingWriter { budget: 200 });
        let mut s = GifSink::with_writer(w, PixelSize { width: 40, height: 30 }, 800, 10).unwrap();
        let mut failed = false;
        for t in 0..6 {
            failed |= s.push(&solid(40, 30), Duration::from_millis(t * 100)).is_err();
        }
        failed |= s.finish().is_err();
        assert!(failed, "a failing writer must surface as an error");
    }

    #[test]
    fn gif_slow_writer_keeps_every_frame() {
        let bytes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let w = Box::new(SlowWriter(bytes.clone()));
        let mut s = GifSink::with_writer(w, PixelSize { width: 24, height: 16 }, 800, 10).unwrap();
        for t in 0..20 {
            s.push(&solid(24, 16), Duration::from_millis(t * 100)).unwrap();
        }
        s.finish().unwrap();
        let data = bytes.lock().unwrap().clone();
        let mut dec = gif::DecodeOptions::new().read_info(std::io::Cursor::new(data)).unwrap();
        let mut count = 0;
        while dec.read_next_frame().unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 20);
    }

    #[test]
    fn finish_without_frames_is_error() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let mut s = GifSink::create(&p, PixelSize { width: 20, height: 20 }, 800, 10).unwrap();
        assert!(s.finish().is_err());
    }
}
