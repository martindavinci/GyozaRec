// SPDX-License-Identifier: GPL-3.0-or-later
//! Splits one recording into files under a size limit. Each part starts at
//! time zero and plays on its own; the next part continues where it stopped.

use super::FrameSink;
use crate::pixels::PixelBuffer;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A sink for one part that can tell how many bytes it has written so far.
pub trait PartSink: FrameSink {
    fn bytes(&self) -> u64;
}

/// Opens the sink of one part at the given path.
pub type OpenPart = Box<dyn FnMut(&Path) -> crate::Result<Box<dyn PartSink>> + Send>;

/// Tells whether a path is already taken.
pub type PathTaken = Box<dyn Fn(&Path) -> bool + Send>;

const AUDIO_RATE: u64 = crate::audio::OUT_RATE as u64;
const CHANNELS: usize = 2;

/// Path of part `n` (counted from 1) of a recording whose first file is
/// `first`: `clip.mp4`, `clip_part2.mp4`, `clip_part3.mp4`…
pub fn part_path(first: &Path, n: u32) -> PathBuf {
    if n <= 1 {
        return first.to_path_buf();
    }
    let stem = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match first.extension() {
        Some(ext) => format!("{stem}_part{n}.{}", ext.to_string_lossy()),
        None => format!("{stem}_part{n}"),
    };
    first.with_file_name(name)
}

/// Audio frame (one stereo sample pair) at time `t` of the recording.
fn audio_frame_at(t: Duration) -> u64 {
    (t.as_nanos() * u128::from(AUDIO_RATE) / 1_000_000_000) as u64
}

struct Part {
    sink: Box<dyn PartSink>,
    /// Recording time where this part starts.
    start: Duration,
    /// Audio frame of the recording where this part starts.
    start_frame: u64,
    /// Video frames written to this part.
    frames: u64,
}

impl Part {
    fn push_audio(&mut self, samples: &[i16], start_frame: u64) -> crate::Result<()> {
        // Audio from before this part began is dropped (only reachable when a
        // part had to be closed before its audio caught up).
        let skip = self.start_frame.saturating_sub(start_frame) as usize * CHANNELS;
        if skip >= samples.len() {
            return Ok(());
        }
        let first = start_frame.max(self.start_frame);
        self.sink.push_audio(&samples[skip..], first - self.start_frame)
    }
}

/// Writes a recording as consecutive parts, each at most `limit` bytes.
/// A new part begins before a frame when the current part's bytes plus
/// `reserve` (what the encoder still holds, and the index written when the
/// file closes) reach the limit. The audio runs behind the video, so the old
/// part stays open for audio until the audio reaches the cut, then closes.
pub struct SplitSink {
    first: PathBuf,
    limit: u64,
    reserve: u64,
    audio: bool,
    open: OpenPart,
    taken: PathTaken,
    current: Part,
    /// The previous part, waiting for its last audio.
    closing: Option<Part>,
    next_number: u32,
    files: Vec<PathBuf>,
}

impl SplitSink {
    /// Opens the first part at `first`. `audio`: the recording has sound, so a
    /// part is closed only once its audio is complete.
    pub fn new(
        first: PathBuf,
        limit: u64,
        reserve: u64,
        audio: bool,
        mut open: OpenPart,
        taken: PathTaken,
    ) -> crate::Result<SplitSink> {
        let sink = open(&first)?;
        let current = Part { sink, start: Duration::ZERO, start_frame: 0, frames: 0 };
        Ok(SplitSink {
            files: vec![first.clone()],
            first,
            limit,
            reserve,
            audio,
            open,
            taken,
            current,
            closing: None,
            next_number: 2,
        })
    }

    fn next_path(&mut self) -> PathBuf {
        loop {
            let path = part_path(&self.first, self.next_number);
            self.next_number += 1;
            if !(self.taken)(&path) {
                return path;
            }
        }
    }

    /// Starts a new part at recording time `t`.
    fn rotate(&mut self, t: Duration) -> crate::Result<()> {
        // A part whose audio never caught up closes now: parts are never shorter
        // than the audio delay in practice, so nothing is lost.
        if let Some(mut old) = self.closing.take() {
            old.sink.finish()?;
        }
        let path = self.next_path();
        let sink = (self.open)(&path)?;
        self.files.push(path);
        let part = Part { sink, start: t, start_frame: audio_frame_at(t), frames: 0 };
        let mut old = std::mem::replace(&mut self.current, part);
        if self.audio {
            self.closing = Some(old);
        } else {
            old.sink.finish()?;
        }
        Ok(())
    }
}

impl FrameSink for SplitSink {
    fn push(&mut self, frame: &PixelBuffer, t: Duration) -> crate::Result<()> {
        if self.current.frames > 0
            && self.current.sink.bytes().saturating_add(self.reserve) >= self.limit
        {
            self.rotate(t)?;
        }
        let part = &mut self.current;
        part.sink.push(frame, t.saturating_sub(part.start))?;
        part.frames += 1;
        Ok(())
    }

    fn push_audio(&mut self, samples: &[i16], start_frame: u64) -> crate::Result<()> {
        let mut samples = samples;
        let mut start = start_frame;
        if let Some(old) = self.closing.as_mut() {
            let cut = self.current.start_frame;
            if start < cut {
                let before = ((cut - start) as usize).min(samples.len() / CHANNELS);
                old.push_audio(&samples[..before * CHANNELS], start)?;
                samples = &samples[before * CHANNELS..];
                start += before as u64;
            }
            if start >= cut
                && let Some(mut old) = self.closing.take()
            {
                old.sink.finish()?;
            }
        }
        if samples.is_empty() {
            return Ok(());
        }
        self.current.push_audio(samples, start)
    }

    fn finish(&mut self) -> crate::Result<()> {
        if let Some(mut old) = self.closing.take() {
            old.sink.finish()?;
        }
        self.current.sink.finish()
    }

    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelRect;
    use std::sync::{Arc, Mutex};

    /// What one fake part received.
    #[derive(Default)]
    struct Log {
        path: PathBuf,
        times: Vec<Duration>,
        /// (start frame within the part, frames)
        audio: Vec<(u64, usize)>,
        finished: bool,
    }

    type Logs = Arc<Mutex<Vec<Log>>>;

    /// Grows by `per_frame` bytes for every video frame.
    struct FakePart {
        logs: Logs,
        index: usize,
        per_frame: u64,
    }

    impl FrameSink for FakePart {
        fn push(&mut self, _: &PixelBuffer, t: Duration) -> crate::Result<()> {
            self.logs.lock().unwrap()[self.index].times.push(t);
            Ok(())
        }
        fn push_audio(&mut self, samples: &[i16], start_frame: u64) -> crate::Result<()> {
            assert!(!samples.is_empty(), "sinks never get empty audio");
            self.logs.lock().unwrap()[self.index].audio.push((start_frame, samples.len() / 2));
            Ok(())
        }
        fn finish(&mut self) -> crate::Result<()> {
            self.logs.lock().unwrap()[self.index].finished = true;
            Ok(())
        }
    }

    impl PartSink for FakePart {
        fn bytes(&self) -> u64 {
            self.logs.lock().unwrap()[self.index].times.len() as u64 * self.per_frame
        }
    }

    fn opener(logs: &Logs, per_frame: u64) -> OpenPart {
        let logs = logs.clone();
        Box::new(move |path: &Path| {
            let mut all = logs.lock().unwrap();
            all.push(Log { path: path.to_path_buf(), ..Log::default() });
            let index = all.len() - 1;
            Ok(Box::new(FakePart { logs: logs.clone(), index, per_frame }) as Box<dyn PartSink>)
        })
    }

    fn free() -> PathTaken {
        Box::new(|_: &Path| false)
    }

    fn sink(logs: &Logs, limit: u64, reserve: u64, audio: bool) -> SplitSink {
        let first = PathBuf::from(r"C:\out\clip.mp4");
        SplitSink::new(first, limit, reserve, audio, opener(logs, 10), free()).unwrap()
    }

    fn frame() -> PixelBuffer {
        PixelBuffer::solid(PixelRect::new(0, 0, 2, 2), 0, 0, 0)
    }

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    fn push_frames(s: &mut SplitSink, times: &[u64]) {
        for &t in times {
            s.push(&frame(), ms(t)).unwrap();
        }
    }

    #[test]
    fn part_paths_number_from_the_second() {
        let first = Path::new(r"C:\out\clip.mp4");
        assert_eq!(part_path(first, 1), first);
        assert_eq!(part_path(first, 2), Path::new(r"C:\out\clip_part2.mp4"));
        assert_eq!(part_path(first, 12), Path::new(r"C:\out\clip_part12.mp4"));
        assert_eq!(part_path(Path::new("raw"), 3), Path::new("raw_part3"));
    }

    #[test]
    fn stays_in_one_file_under_the_limit() {
        let logs = Logs::default();
        let mut s = sink(&logs, 1_000, 0, false);
        push_frames(&mut s, &[0, 100, 200]);
        s.finish().unwrap();
        let logs = logs.lock().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].times, vec![ms(0), ms(100), ms(200)]);
        assert!(logs[0].finished);
        assert_eq!(s.files(), vec![PathBuf::from(r"C:\out\clip.mp4")]);
    }

    #[test]
    fn splits_before_the_frame_that_would_reach_the_limit_and_restarts_time() {
        let logs = Logs::default();
        // 10 bytes a frame, limit 50, reserve 20: a part closes at 30 bytes.
        let mut s = sink(&logs, 50, 20, false);
        push_frames(&mut s, &[0, 100, 200, 300, 400, 500, 600, 700]);
        s.finish().unwrap();
        let logs = logs.lock().unwrap();
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].times, vec![ms(0), ms(100), ms(200)]);
        assert_eq!(logs[1].times, vec![ms(0), ms(100), ms(200)]);
        assert_eq!(logs[2].times, vec![ms(0), ms(100)]);
        assert!(logs.iter().all(|l| l.finished));
        let names: Vec<_> = logs.iter().map(|l| l.path.clone()).collect();
        assert_eq!(names, s.files());
        assert_eq!(names[1], PathBuf::from(r"C:\out\clip_part2.mp4"));
        assert_eq!(names[2], PathBuf::from(r"C:\out\clip_part3.mp4"));
    }

    #[test]
    fn a_part_always_gets_at_least_one_frame() {
        let logs = Logs::default();
        // Even a frame bigger than the limit is written, one per part.
        let mut s = sink(&logs, 5, 0, false);
        push_frames(&mut s, &[0, 100]);
        s.finish().unwrap();
        let logs = logs.lock().unwrap();
        assert_eq!(logs.iter().map(|l| l.times.len()).collect::<Vec<_>>(), vec![1, 1]);
    }

    #[test]
    fn without_audio_the_old_part_closes_at_the_split() {
        let logs = Logs::default();
        let mut s = sink(&logs, 30, 0, false);
        push_frames(&mut s, &[0, 100, 200, 300]);
        assert!(logs.lock().unwrap()[0].finished);
        assert!(!logs.lock().unwrap()[1].finished);
    }

    #[test]
    fn audio_before_the_cut_finishes_the_old_part_then_it_closes() {
        let logs = Logs::default();
        let mut s = sink(&logs, 30, 0, true);
        // Frames at 0, 100, 200 fill part 1; the frame at 300 ms opens part 2.
        push_frames(&mut s, &[0, 100, 200, 300]);
        assert!(!logs.lock().unwrap()[0].finished, "waits for its audio");
        // Audio runs behind: 0–250 ms all belongs to part 1.
        s.push_audio(&vec![0; 12_000 * 2], 0).unwrap();
        assert!(!logs.lock().unwrap()[0].finished);
        // 250–350 ms crosses the cut at 300 ms (frame 14 400).
        s.push_audio(&vec![0; 4_800 * 2], 12_000).unwrap();
        let logs = logs.lock().unwrap();
        assert!(logs[0].finished);
        assert_eq!(logs[0].audio, vec![(0, 12_000), (12_000, 2_400)]);
        assert_eq!(logs[1].audio, vec![(0, 2_400)]);
    }

    #[test]
    fn audio_after_the_cut_goes_to_the_new_part_at_its_own_time() {
        let logs = Logs::default();
        let mut s = sink(&logs, 30, 0, true);
        push_frames(&mut s, &[0, 100, 200, 300]);
        s.push_audio(&vec![0; 14_400 * 2], 0).unwrap(); // exactly up to the cut
        assert!(logs.lock().unwrap()[0].finished);
        s.push_audio(&vec![0; 480 * 2], 14_400).unwrap();
        s.push_audio(&vec![0; 480 * 2], 15_360).unwrap();
        let logs = logs.lock().unwrap();
        assert_eq!(logs[0].audio, vec![(0, 14_400)]);
        assert_eq!(logs[1].audio, vec![(0, 480), (960, 480)]);
    }

    #[test]
    fn finish_closes_a_part_still_waiting_for_audio() {
        let logs = Logs::default();
        let mut s = sink(&logs, 30, 0, true);
        push_frames(&mut s, &[0, 100, 200, 300]);
        s.finish().unwrap();
        let logs = logs.lock().unwrap();
        assert!(logs[0].finished && logs[1].finished);
    }

    #[test]
    fn a_second_split_closes_a_part_whose_audio_never_came() {
        let logs = Logs::default();
        let mut s = sink(&logs, 10, 0, true);
        push_frames(&mut s, &[0, 100, 200]);
        let logs_now = logs.lock().unwrap();
        assert_eq!(logs_now.len(), 3);
        assert!(logs_now[0].finished && !logs_now[1].finished);
        drop(logs_now);
        // 100–300 ms: part 2's share closes it, the rest starts part 3.
        s.push_audio(&vec![0; 9_600 * 2], 4_800).unwrap();
        let logs = logs.lock().unwrap();
        assert!(logs[1].finished && !logs[2].finished);
        assert_eq!(logs[0].audio, Vec::new());
        assert_eq!(logs[1].audio, vec![(0, 4_800)]);
        assert_eq!(logs[2].audio, vec![(0, 4_800)]);
    }

    #[test]
    fn taken_names_are_skipped() {
        let logs = Logs::default();
        let taken: PathTaken = Box::new(|p: &Path| p.ends_with("clip_part2.mp4"));
        let first = PathBuf::from(r"C:\out\clip.mp4");
        let mut s = SplitSink::new(first, 10, 0, false, opener(&logs, 10), taken).unwrap();
        push_frames(&mut s, &[0, 100]);
        assert_eq!(s.files()[1], PathBuf::from(r"C:\out\clip_part3.mp4"));
    }
}
