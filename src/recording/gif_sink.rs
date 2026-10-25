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
                // The last distinct frame, and the GIF frame made from it, held
                // back so a repeat of it only adds to its delay.
                let mut previous: Option<Vec<u8>> = None;
                let mut held: Option<gif::Frame<'static>> = None;
                for (rgba, delay) in frames {
                    if previous.as_ref() == Some(&rgba) {
                        if let Some(frame) = held.as_mut() {
                            frame.delay = frame.delay.saturating_add(delay);
                        }
                        continue;
                    }
                    let mut frame = match &previous {
                        Some(prev) => changed_frame(prev, &rgba, w, h),
                        None => {
                            gif::Frame::from_rgba_speed(w, h, &mut rgba.clone(), QUANTIZE_SPEED)
                        }
                    };
                    frame.delay = delay;
                    frame.dispose = gif::DisposalMethod::Keep;
                    if let Some(done) = held.replace(frame) {
                        encoder.write_frame(&done)?;
                    }
                    previous = Some(rgba);
                }
                if let Some(done) = held {
                    encoder.write_frame(&done)?;
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

/// Bounds `(x, y, w, h)` of the pixels that differ between two RGBA frames of
/// `width` pixels per row; `None` when they are identical.
fn changed_bounds(prev: &[u8], cur: &[u8], width: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, (a, b)) in prev.as_chunks::<4>().0.iter().zip(cur.as_chunks::<4>().0).enumerate() {
        if a != b {
            let (x, y) = (i % width, i / width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    (x0 != usize::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// Only the changed area of `cur`, placed where it belongs; pixels that did
/// not change are transparent so the previous frame shows through.
fn changed_frame(prev: &[u8], cur: &[u8], w: u16, h: u16) -> gif::Frame<'static> {
    let width = usize::from(w);
    let Some((x, y, cw, ch)) = changed_bounds(prev, cur, width) else {
        return gif::Frame::from_rgba_speed(w, h, &mut cur.to_vec(), QUANTIZE_SPEED);
    };
    let mut crop = Vec::with_capacity(cw * ch * 4);
    for row in y..y + ch {
        let start = (row * width + x) * 4;
        let (before, after) = (&prev[start..start + cw * 4], &cur[start..start + cw * 4]);
        for (a, b) in before.as_chunks::<4>().0.iter().zip(after.as_chunks::<4>().0) {
            if a == b {
                crop.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                crop.extend_from_slice(b);
            }
        }
    }
    let mut frame = gif::Frame::from_rgba_speed(cw as u16, ch as u16, &mut crop, QUANTIZE_SPEED);
    (frame.left, frame.top) = (x as u16, y as u16);
    frame
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

    /// A solid frame whose color depends on `i`, so consecutive ones differ.
    fn shade(w: i32, h: i32, i: u64) -> PixelBuffer {
        PixelBuffer::solid(PixelRect::new(0, 0, w, h), (i * 37 % 256) as u8, 200, 30)
    }

    /// `base` with an `size`×`size` square of another color at (`x`, `y`).
    fn with_square(base: &PixelBuffer, x: i32, y: i32, size: i32) -> PixelBuffer {
        let mut f = base.clone();
        for dy in 0..size {
            for dx in 0..size {
                f.put_pixel(
                    crate::geometry::PixelPoint { x: x + dx, y: y + dy },
                    [200, 40, 90, 255],
                );
            }
        }
        f
    }

    /// Every decoded frame drawn over the previous ones, as a viewer shows them.
    fn composited(path: &Path) -> Vec<Vec<u8>> {
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts.read_info(fs::File::open(path).unwrap()).unwrap();
        let (w, h) = (usize::from(dec.width()), usize::from(dec.height()));
        let mut canvas = vec![0u8; w * h * 4];
        let mut out = vec![];
        while let Some(f) = dec.read_next_frame().unwrap() {
            let (fl, ft, fw) = (usize::from(f.left), usize::from(f.top), usize::from(f.width));
            for (i, px) in f.buffer.as_chunks::<4>().0.iter().enumerate() {
                if px[3] == 0 {
                    continue;
                }
                let (x, y) = (fl + i % fw, ft + i / fw);
                canvas[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(px);
            }
            out.push(canvas.clone());
        }
        out
    }

    /// (left, top, width, height) of every frame.
    fn placements(path: &Path) -> Vec<(u16, u16, u16, u16)> {
        let mut dec = gif::DecodeOptions::new().read_info(fs::File::open(path).unwrap()).unwrap();
        let mut out = vec![];
        while let Some(f) = dec.read_next_frame().unwrap() {
            out.push((f.left, f.top, f.width, f.height));
        }
        out
    }

    #[test]
    fn identical_frames_merge_into_one_with_summed_delay() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let mut s = GifSink::create(&p, PixelSize { width: 40, height: 30 }, 800, 10).unwrap();
        for t in [0, 100, 250] {
            s.push(&solid(40, 30), Duration::from_millis(t)).unwrap();
        }
        s.finish().unwrap();
        assert_eq!(frames(&p).2, vec![35]);
    }

    #[test]
    fn only_the_changed_area_is_written() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let base = solid(200, 100);
        let mut s = GifSink::create(&p, PixelSize { width: 200, height: 100 }, 800, 10).unwrap();
        s.push(&base, Duration::ZERO).unwrap();
        s.push(&with_square(&base, 50, 20, 10), Duration::from_millis(100)).unwrap();
        s.finish().unwrap();
        assert_eq!(placements(&p), vec![(0, 0, 200, 100), (50, 20, 10, 10)]);
    }

    #[test]
    fn composited_frames_match_the_sources() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let base = shade(64, 48, 3);
        let sources: Vec<PixelBuffer> =
            (0..4).map(|i| with_square(&base, 4 + i * 12, 6 + i * 8, 10)).collect();
        let mut s = GifSink::create(&p, PixelSize { width: 64, height: 48 }, 800, 10).unwrap();
        for (i, f) in sources.iter().enumerate() {
            s.push(f, Duration::from_millis(i as u64 * 100)).unwrap();
        }
        s.finish().unwrap();
        let shown = composited(&p);
        assert_eq!(shown.len(), 4);
        for (frame, source) in shown.iter().zip(&sources) {
            let expected = source.to_rgba();
            let worst = frame.iter().zip(&expected).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(worst <= 48, "max channel error {worst}");
        }
    }

    #[test]
    fn mostly_static_recording_is_much_smaller() {
        let d = tempdir().unwrap();
        let (moving, changing) = (d.path().join("m.gif"), d.path().join("c.gif"));
        let size = PixelSize { width: 320, height: 240 };
        let noise = |seed: u64| {
            let mut f = PixelBuffer::solid(PixelRect::new(0, 0, 320, 240), 0, 0, 0);
            let mut v = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            for y in 0..240 {
                for x in 0..320 {
                    v = v.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let c = (v >> 33) as u8;
                    f.put_pixel(crate::geometry::PixelPoint { x, y }, [c, c / 2, 255 - c, 255]);
                }
            }
            f
        };
        let background = noise(1);
        let mut m = GifSink::create(&moving, size, 800, 10).unwrap();
        let mut c = GifSink::create(&changing, size, 800, 10).unwrap();
        for i in 0..30u64 {
            let t = Duration::from_millis(i * 100);
            m.push(&with_square(&background, (i * 9) as i32, 100, 16), t).unwrap();
            c.push(&noise(i + 2), t).unwrap();
        }
        m.finish().unwrap();
        c.finish().unwrap();
        let (ms, cs) =
            (fs::metadata(&moving).unwrap().len(), fs::metadata(&changing).unwrap().len());
        assert!(ms * 10 < cs * 4, "{ms} bytes vs {cs} bytes");
    }

    #[test]
    fn delays_follow_real_timestamps() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.gif");
        let mut s = GifSink::create(&p, PixelSize { width: 40, height: 30 }, 800, 10).unwrap();
        for (i, t) in [0, 100, 250].into_iter().enumerate() {
            s.push(&shade(40, 30, i as u64), Duration::from_millis(t)).unwrap();
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
        s.push(&shade(20, 20, 0), Duration::ZERO).unwrap();
        s.push(&shade(20, 20, 1), Duration::from_millis(5)).unwrap();
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
            failed |= s.push(&shade(40, 30, t), Duration::from_millis(t * 100)).is_err();
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
            s.push(&shade(24, 16, t), Duration::from_millis(t * 100)).unwrap();
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
