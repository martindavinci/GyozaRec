//! Turns a video file into an animated GIF with the GIF options.

use crate::geometry::PixelSize;
use crate::pixels::PixelBuffer;
use crate::recording::{FrameSink, GifSink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Decoded frames of one video, in presentation order.
pub trait VideoFrames {
    fn duration(&self) -> Option<Duration>;
    fn size(&self) -> PixelSize;
    /// `None` at the end of the video.
    fn next_frame(&mut self) -> Option<crate::Result<(PixelBuffer, Duration)>>;
}

pub trait VideoReader: Send + Sync {
    fn open(&self, path: &Path) -> crate::Result<Box<dyn VideoFrames>>;
}

pub struct ConvertJob {
    pub source: PathBuf,
    pub target: PathBuf,
    pub fps: u32,
    pub max_long_side: u32,
    pub max_duration: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConvertEnd {
    Done,
    Canceled,
}

/// Keeps one frame per `1 / fps` period, up to `max_duration`, and reports
/// progress (0.0 to 1.0) each time the whole percent changes. A canceled or
/// failed conversion leaves no file behind.
pub fn convert_to_gif(
    reader: &dyn VideoReader,
    job: &ConvertJob,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> crate::Result<ConvertEnd> {
    let result = run(reader, job, cancel, progress);
    if !matches!(result, Ok(ConvertEnd::Done)) {
        let _ = std::fs::remove_file(&job.target);
    }
    result
}

fn run(
    reader: &dyn VideoReader,
    job: &ConvertJob,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> crate::Result<ConvertEnd> {
    let unreadable = |_| crate::Error::from(crate::UserError::VideoUnreadable);
    let mut video = reader.open(&job.source).map_err(unreadable)?;
    let span = video.duration().map_or(job.max_duration, |d| d.min(job.max_duration));
    let period = Duration::from_secs(1) / job.fps.max(1);
    let mut sink = GifSink::create(&job.target, video.size(), job.max_long_side, job.fps)?;
    // Times count from the first frame: some files start later than zero.
    let (mut next, mut first, mut percent) = (Duration::ZERO, None, None);
    let mut end = video.duration().unwrap_or(Duration::ZERO);
    while let Some(frame) = video.next_frame() {
        if cancel.load(Ordering::SeqCst) {
            return Ok(ConvertEnd::Canceled);
        }
        let (frame, at) = frame.map_err(unreadable)?;
        let start = *first.get_or_insert(at);
        let t = at.saturating_sub(start);
        if t >= job.max_duration {
            end = job.max_duration + start;
            break;
        }
        if t < next {
            continue;
        }
        next = period * ((t.as_nanos() / period.as_nanos()) as u32 + 1);
        sink.push(&frame, t)?;
        let now = if span.is_zero() {
            100
        } else {
            (t.as_secs_f32() / span.as_secs_f32() * 100.0) as u32
        };
        if percent != Some(now.min(100)) {
            percent = Some(now.min(100));
            progress(now.min(100) as f32 / 100.0);
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(ConvertEnd::Canceled);
    }
    let start = first.unwrap_or(Duration::ZERO);
    sink.finish_at(end.saturating_sub(start).min(job.max_duration))?;
    if percent != Some(100) {
        progress(1.0);
    }
    Ok(ConvertEnd::Done)
}

/// `clip.mp4` → `clip.gif` in the same folder, or `clip (2).gif`… when taken.
pub fn gif_path_for(source: &Path) -> PathBuf {
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = source.parent().unwrap_or(Path::new(""));
    let first = folder.join(format!("{stem}.gif"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| folder.join(format!("{stem} ({n}).gif")))
        .find(|p| !p.exists())
        .expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelRect;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    /// Frames of one color each, at the given milliseconds.
    struct FakeReader {
        frames: Vec<(u8, u64)>,
        fail_open: bool,
    }

    struct FakeFrames {
        frames: std::vec::IntoIter<(u8, u64)>,
        duration: Option<Duration>,
    }

    impl VideoFrames for FakeFrames {
        fn duration(&self) -> Option<Duration> {
            self.duration
        }
        fn size(&self) -> PixelSize {
            PixelSize { width: 16, height: 12 }
        }
        fn next_frame(&mut self) -> Option<crate::Result<(PixelBuffer, Duration)>> {
            let (c, ms) = self.frames.next()?;
            let img = PixelBuffer::solid(PixelRect::new(0, 0, 16, 12), c, 255 - c, 40);
            Some(Ok((img, Duration::from_millis(ms))))
        }
    }

    impl VideoReader for FakeReader {
        fn open(&self, _path: &Path) -> crate::Result<Box<dyn VideoFrames>> {
            if self.fail_open {
                return Err(crate::Error::Message("not a video".into()));
            }
            let duration = self.frames.last().map(|(_, ms)| Duration::from_millis(*ms));
            Ok(Box::new(FakeFrames { frames: self.frames.clone().into_iter(), duration }))
        }
    }

    fn reader(ms: &[u64]) -> FakeReader {
        let frames = ms.iter().enumerate().map(|(i, m)| ((i * 23 % 256) as u8, *m)).collect();
        FakeReader { frames, fail_open: false }
    }

    fn job(dir: &Path, max_ms: u64) -> ConvertJob {
        ConvertJob {
            source: dir.join("clip.mp4"),
            target: dir.join("clip.gif"),
            fps: 10,
            max_long_side: 800,
            max_duration: Duration::from_millis(max_ms),
        }
    }

    fn delays(path: &Path) -> Vec<u16> {
        let mut dec = gif::DecodeOptions::new().read_info(fs::File::open(path).unwrap()).unwrap();
        let mut out = vec![];
        while let Some(f) = dec.read_next_frame().unwrap() {
            out.push(f.delay);
        }
        out
    }

    fn convert(r: &FakeReader, j: &ConvertJob) -> (crate::Result<ConvertEnd>, Vec<f32>) {
        let mut seen = vec![];
        let end = convert_to_gif(r, j, &AtomicBool::new(false), &mut |p| seen.push(p));
        (end, seen)
    }

    #[test]
    fn conversion_keeps_one_frame_per_period_with_variable_timestamps() {
        let d = tempdir().unwrap();
        let j = job(d.path(), 10_000);
        let (end, _) = convert(&reader(&[0, 30, 70, 90, 140, 260]), &j);
        assert_eq!(end.unwrap(), ConvertEnd::Done);
        assert_eq!(delays(&j.target), vec![14, 12, 10]);
    }

    #[test]
    fn conversion_stops_at_the_maximum_length() {
        let d = tempdir().unwrap();
        let j = job(d.path(), 2000);
        let ms: Vec<u64> = (0..=50).map(|i| i * 100).collect();
        convert(&reader(&ms), &j).0.unwrap();
        assert_eq!(delays(&j.target).len(), 20);
    }

    #[test]
    fn canceled_conversion_leaves_no_file() {
        let d = tempdir().unwrap();
        let j = job(d.path(), 10_000);
        let ms: Vec<u64> = (0..100).map(|i| i * 100).collect();
        let cancel = AtomicBool::new(false);
        let end = convert_to_gif(&reader(&ms), &j, &cancel, &mut |p| {
            if p >= 0.5 {
                cancel.store(true, Ordering::SeqCst);
            }
        });
        assert_eq!(end.unwrap(), ConvertEnd::Canceled);
        assert!(!j.target.exists());
    }

    #[test]
    fn unreadable_video_fails_without_a_file() {
        let d = tempdir().unwrap();
        let j = job(d.path(), 10_000);
        let r = FakeReader { frames: vec![], fail_open: true };
        let err = convert(&r, &j).0.unwrap_err();
        assert_eq!(
            crate::Failure::from(&err),
            crate::Failure::User(crate::UserError::VideoUnreadable)
        );
        assert!(!j.target.exists());
    }

    #[test]
    fn offset_timestamps_are_measured_from_the_first_frame() {
        let d = tempdir().unwrap();
        let j = job(d.path(), 2000);
        let ms: Vec<u64> = (0..=40).map(|i| 5000 + i * 100).collect();
        convert(&reader(&ms), &j).0.unwrap();
        assert_eq!(delays(&j.target).len(), 20);
    }

    #[test]
    fn last_frame_ends_at_the_limit() {
        let d = tempdir().unwrap();
        let j = job(d.path(), 2000);
        let ms: Vec<u64> = (0..=50).map(|i| i * 100).collect();
        convert(&reader(&ms), &j).0.unwrap();
        assert_eq!(delays(&j.target).iter().map(|d| u32::from(*d)).sum::<u32>(), 200);
    }

    #[test]
    fn gif_path_avoids_existing_files() {
        let d = tempdir().unwrap();
        let src = d.path().join("clip.mp4");
        assert_eq!(gif_path_for(&src), d.path().join("clip.gif"));
        fs::write(d.path().join("clip.gif"), b"x").unwrap();
        assert_eq!(gif_path_for(&src), d.path().join("clip (2).gif"));
    }

    #[test]
    fn progress_is_reported_once_per_percent() {
        let d = tempdir().unwrap();
        let j = ConvertJob { fps: 1000, ..job(d.path(), 100_000) };
        let ms: Vec<u64> = (0..1000).map(|i| i * 10).collect();
        let (end, seen) = convert(&reader(&ms), &j);
        end.unwrap();
        assert!(seen.len() <= 101, "{} reports", seen.len());
        assert_eq!(seen.last(), Some(&1.0));
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
    }
}
