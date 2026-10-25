//! Grabs frames at a fixed rate and feeds them to a sink, with the audio
//! of the recording when there is any.

use super::{CANCEL, FrameSink, LoopConfig, LoopEnd, PAUSE, STOP};
use crate::audio::{AudioFeed, LATENCY, PauseHistory, Timeline};
use crate::capture::ScreenGrabber;
use crate::clock::Clock;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

/// Longest single wait, so STOP and CANCEL are noticed quickly.
const MAX_WAIT: Duration = Duration::from_millis(50);

/// A video-only recording.
pub fn run_capture_loop(
    grabber: &dyn ScreenGrabber,
    sink: &mut dyn FrameSink,
    clock: &dyn Clock,
    cfg: &LoopConfig,
    control: &AtomicU8,
) -> crate::Result<LoopEnd> {
    let never = AtomicU64::new(u64::MAX);
    run_capture_loop_with_audio(grabber, sink, clock, cfg, control, &never, None)
}

/// Frame `k` is due at `k × period`. A slow grab skips the frames it made
/// late instead of catching up in a burst. While paused nothing is grabbed,
/// and the paused time counts neither in the timestamps nor in the limit.
/// Audio chunks are taken every turn (also while paused, so the queue never
/// grows) and committed after `LATENCY`; at the end the sources are stopped
/// and everything left is written before the sink finishes. `paused_at` holds
/// when the last pause was asked (nanoseconds on `clock`, `u64::MAX` if never).
pub fn run_capture_loop_with_audio(
    grabber: &dyn ScreenGrabber,
    sink: &mut dyn FrameSink,
    clock: &dyn Clock,
    cfg: &LoopConfig,
    control: &AtomicU8,
    paused_at: &AtomicU64,
    audio: Option<AudioFeed>,
) -> crate::Result<LoopEnd> {
    let start = clock.elapsed();
    let period = Duration::from_secs(1) / cfg.fps.max(1);
    let mut next = Duration::ZERO;
    let mut pauses = PauseHistory::new(start);
    let mut audio = audio.map(|feed| (feed, Timeline::new(LATENCY)));
    loop {
        let state = control.load(Ordering::SeqCst);
        if let Some((feed, timeline)) = audio.as_mut() {
            while let Ok(chunk) = feed.chunks.try_recv() {
                timeline.push(chunk);
            }
        }
        match state {
            CANCEL => {
                if let Some((mut feed, _)) = audio.take() {
                    feed.stop();
                }
                return Ok(LoopEnd::Canceled);
            }
            STOP => {
                finish_audio(&mut audio, sink, &pauses, clock.elapsed())?;
                sink.finish()?;
                return Ok(LoopEnd::Stopped);
            }
            PAUSE => {
                // From when the pause was asked: a slow grab or encode may
                // have kept this turn from noticing it for a while.
                let asked = Duration::from_nanos(paused_at.load(Ordering::SeqCst));
                pauses.pause(asked.min(clock.elapsed()));
                write_audio(&mut audio, sink, &pauses, clock.elapsed())?;
                clock.sleep(MAX_WAIT);
                continue;
            }
            _ => pauses.resume(clock.elapsed()),
        }
        write_audio(&mut audio, sink, &pauses, clock.elapsed())?;
        let t = pauses.active_elapsed(clock.elapsed());
        if t >= cfg.max_duration {
            finish_audio(&mut audio, sink, &pauses, clock.elapsed())?;
            sink.finish()?;
            return Ok(LoopEnd::MaxDuration);
        }
        if t >= next {
            let frame = grabber.grab(cfg.region, cfg.include_cursor)?;
            sink.push(&frame, t)?;
            let after = pauses.active_elapsed(clock.elapsed());
            let done = (after.as_nanos() / period.as_nanos()) as u32;
            next = period * (done + 1);
        } else {
            clock.sleep((next - t).min(MAX_WAIT));
        }
    }
}

type Audio = Option<(AudioFeed, Timeline)>;

fn write_audio(
    audio: &mut Audio,
    sink: &mut dyn FrameSink,
    pauses: &PauseHistory,
    now: Duration,
) -> crate::Result<()> {
    if let Some((_, timeline)) = audio.as_mut()
        && let Some(out) = timeline.drain(now, pauses, false)
        && !out.samples.is_empty()
    {
        sink.push_audio(&out.samples, out.start_frame)?;
    }
    Ok(())
}

/// Stops the sources, takes their last chunks and writes the audio up to `end`.
fn finish_audio(
    audio: &mut Audio,
    sink: &mut dyn FrameSink,
    pauses: &PauseHistory,
    end: Duration,
) -> crate::Result<()> {
    let Some((mut feed, mut timeline)) = audio.take() else { return Ok(()) };
    feed.stop();
    while let Ok(chunk) = feed.chunks.try_recv() {
        timeline.push(chunk);
    }
    if let Some(out) = timeline.drain(end, pauses, true)
        && !out.samples.is_empty()
    {
        sink.push_audio(&out.samples, out.start_frame)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::geometry::PixelRect;
    use crate::recording::{CANCEL, LoopConfig, LoopEnd, PAUSE, RUN, STOP};
    use crate::testing::{FakeClock, FakeGrabber, VecSink};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
    use std::time::Duration;

    fn cfg(fps: u32, max_ms: u64) -> LoopConfig {
        LoopConfig {
            region: PixelRect::new(0, 0, 32, 32),
            fps,
            include_cursor: false,
            max_duration: Duration::from_millis(max_ms),
        }
    }

    fn ms(v: &[u64]) -> Vec<Duration> {
        v.iter().map(|&m| Duration::from_millis(m)).collect()
    }

    #[test]
    fn grabs_at_fps_until_max_duration() {
        let (clock, grabber, mut sink) =
            (FakeClock::default(), FakeGrabber::default(), VecSink::default());
        let end =
            run_capture_loop(&grabber, &mut sink, &clock, &cfg(10, 1000), &AtomicU8::new(RUN))
                .unwrap();
        assert_eq!(end, LoopEnd::MaxDuration);
        assert_eq!(sink.times(), ms(&[0, 100, 200, 300, 400, 500, 600, 700, 800, 900]));
        assert!(sink.is_finished());
    }

    #[test]
    fn slow_grabs_skip_missed_frames() {
        let clock = FakeClock::default();
        let grabber = FakeGrabber::slow(&clock, Duration::from_millis(150));
        let mut sink = VecSink::default();
        run_capture_loop(&grabber, &mut sink, &clock, &cfg(10, 1000), &AtomicU8::new(RUN)).unwrap();
        assert_eq!(sink.times(), ms(&[0, 200, 400, 600, 800]));
    }

    #[test]
    fn stop_finishes_sink() {
        let control = Arc::new(AtomicU8::new(RUN));
        let c = control.clone();
        let grabber = FakeGrabber::with_hook(move |n| {
            if n == 3 {
                c.store(STOP, Ordering::SeqCst)
            }
        });
        let mut sink = VecSink::default();
        let end = run_capture_loop(
            &grabber,
            &mut sink,
            &FakeClock::default(),
            &cfg(10, 10_000),
            &control,
        );
        assert_eq!(end.unwrap(), LoopEnd::Stopped);
        assert_eq!((sink.times().len(), sink.is_finished()), (3, true));
    }

    #[test]
    fn pause_removes_paused_time_from_timestamps_and_limit() {
        let control = Arc::new(AtomicU8::new(RUN));
        let (on_grab, on_sleep) = (control.clone(), control.clone());
        let grabber = FakeGrabber::with_hook(move |n| {
            if n == 3 {
                on_grab.store(PAUSE, Ordering::SeqCst)
            }
        });
        let clock = FakeClock::default().with_sleep_hook(move |elapsed| {
            if elapsed >= Duration::from_millis(700) {
                let _ = on_sleep.compare_exchange(PAUSE, RUN, Ordering::SeqCst, Ordering::SeqCst);
            }
        });
        let mut sink = VecSink::default();
        let end = run_capture_loop(&grabber, &mut sink, &clock, &cfg(10, 600), &control).unwrap();
        assert_eq!(end, LoopEnd::MaxDuration);
        assert_eq!(sink.times(), ms(&[0, 100, 200, 300, 400, 500]));
        assert!(clock.elapsed() >= Duration::from_millis(1100), "{:?}", clock.elapsed());
    }

    #[test]
    fn stop_while_paused_finishes_sink() {
        let control = Arc::new(AtomicU8::new(RUN));
        let (on_grab, on_sleep) = (control.clone(), control.clone());
        let grabber = FakeGrabber::with_hook(move |n| {
            if n == 2 {
                on_grab.store(PAUSE, Ordering::SeqCst)
            }
        });
        let clock = FakeClock::default().with_sleep_hook(move |elapsed| {
            if elapsed >= Duration::from_millis(500) {
                on_sleep.store(STOP, Ordering::SeqCst);
            }
        });
        let mut sink = VecSink::default();
        let end = run_capture_loop(&grabber, &mut sink, &clock, &cfg(10, 10_000), &control);
        assert_eq!(end.unwrap(), LoopEnd::Stopped);
        assert_eq!((sink.times().len(), sink.is_finished()), (2, true));
    }

    fn feed(chunks: Vec<crate::audio::PcmChunk>) -> crate::audio::AudioFeed {
        let (tx, rx) = std::sync::mpsc::channel();
        for c in chunks {
            tx.send(c).unwrap();
        }
        crate::audio::AudioFeed::new(rx, Box::new(|| {}), false)
    }

    fn second_of_audio() -> Vec<crate::audio::PcmChunk> {
        (0..100u64)
            .map(|i| crate::audio::PcmChunk {
                source: 0,
                rate: 48_000,
                channels: 2,
                channel_mask: 0,
                samples: vec![0.1; 960],
                at: Duration::from_millis(i * 10),
                discontinuity: false,
            })
            .collect()
    }

    #[test]
    fn capture_loop_pushes_audio_and_never_empty() {
        let mut sink = VecSink::default();
        let end = run_capture_loop_with_audio(
            &FakeGrabber::default(),
            &mut sink,
            &FakeClock::default(),
            &cfg(10, 1000),
            &AtomicU8::new(RUN),
            &AtomicU64::new(u64::MAX),
            Some(feed(second_of_audio())),
        )
        .unwrap();
        assert_eq!(end, LoopEnd::MaxDuration);
        let pushes = sink.audio();
        assert!(!pushes.is_empty());
        assert!(pushes.iter().all(|(_, len)| *len > 0 && len % 2 == 0), "{pushes:?}");
        let mut next = 0;
        for (start, len) in &pushes {
            assert_eq!(*start, next, "contiguous");
            next += (*len / 2) as u64;
        }
        assert!((next as i64 - 48_000).abs() <= 480, "{next} frames for one second");
        assert!(sink.is_finished());
    }

    /// The pause was asked at 150 ms, while a grab the loop only finished at
    /// 200 ms was running: audio from 150 ms on is out, so the pause cue the
    /// panel plays right after asking never lands in the file.
    #[test]
    fn pause_starts_when_it_was_asked_not_when_the_loop_noticed() {
        let control = Arc::new(AtomicU8::new(RUN));
        let paused_at = Arc::new(AtomicU64::new(u64::MAX));
        let (on_grab, on_sleep, asked) = (control.clone(), control.clone(), paused_at.clone());
        let grabber = FakeGrabber::with_hook(move |n| {
            if n == 3 {
                asked.store(150_000_000, Ordering::SeqCst);
                on_grab.store(PAUSE, Ordering::SeqCst);
            }
        });
        let clock = FakeClock::default().with_sleep_hook(move |elapsed| {
            if elapsed >= Duration::from_millis(700) {
                let _ = on_sleep.compare_exchange(PAUSE, RUN, Ordering::SeqCst, Ordering::SeqCst);
            }
        });
        // Each 10 ms chunk holds its capture time: chunk i is i / 1000.
        let chunks = second_of_audio()
            .into_iter()
            .enumerate()
            .map(|(i, c)| crate::audio::PcmChunk { samples: vec![i as f32 / 1000.0; 960], ..c })
            .collect();
        let mut sink = VecSink::default();
        run_capture_loop_with_audio(
            &grabber,
            &mut sink,
            &clock,
            &cfg(10, 600),
            &control,
            &paused_at,
            Some(feed(chunks)),
        )
        .unwrap();
        let samples = sink.audio_samples();
        let at = |ms: usize| f32::from(samples[ms * 48 * 2]) / f32::from(i16::MAX);
        assert!(at(100) < 0.02, "before the pause: {}", at(100));
        assert!(at(160) > 0.06, "160 ms of recording is after the resume: {}", at(160));
    }

    #[test]
    fn cancel_discards_audio() {
        let mut sink = VecSink::default();
        let end = run_capture_loop_with_audio(
            &FakeGrabber::default(),
            &mut sink,
            &FakeClock::default(),
            &cfg(10, 1000),
            &AtomicU8::new(CANCEL),
            &AtomicU64::new(u64::MAX),
            Some(feed(second_of_audio())),
        )
        .unwrap();
        assert_eq!(end, LoopEnd::Canceled);
        assert!(sink.audio().is_empty() && !sink.is_finished());
    }

    #[test]
    fn cancel_does_not_finish_sink() {
        let mut sink = VecSink::default();
        let end = run_capture_loop(
            &FakeGrabber::default(),
            &mut sink,
            &FakeClock::default(),
            &cfg(10, 1000),
            &AtomicU8::new(CANCEL),
        )
        .unwrap();
        assert_eq!((end, sink.is_finished()), (LoopEnd::Canceled, false));
    }
}
