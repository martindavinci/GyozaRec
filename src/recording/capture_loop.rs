//! Grabs frames at a fixed rate and feeds them to a sink.

use super::{CANCEL, FrameSink, LoopConfig, LoopEnd, STOP};
use crate::capture::ScreenGrabber;
use crate::clock::Clock;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

/// Longest single wait, so STOP and CANCEL are noticed quickly.
const MAX_WAIT: Duration = Duration::from_millis(50);

/// Frame `k` is due at `k × period`. A slow grab skips the frames it made
/// late instead of catching up in a burst.
pub fn run_capture_loop(
    grabber: &dyn ScreenGrabber,
    sink: &mut dyn FrameSink,
    clock: &dyn Clock,
    cfg: &LoopConfig,
    control: &AtomicU8,
) -> crate::Result<LoopEnd> {
    let start = clock.elapsed();
    let period = Duration::from_secs(1) / cfg.fps.max(1);
    let mut next = Duration::ZERO;
    loop {
        match control.load(Ordering::SeqCst) {
            CANCEL => return Ok(LoopEnd::Canceled),
            STOP => {
                sink.finish()?;
                return Ok(LoopEnd::Stopped);
            }
            _ => {}
        }
        let t = clock.elapsed().saturating_sub(start);
        if t >= cfg.max_duration {
            sink.finish()?;
            return Ok(LoopEnd::MaxDuration);
        }
        if t >= next {
            let frame = grabber.grab(cfg.region, cfg.include_cursor)?;
            sink.push(&frame, t)?;
            let after = clock.elapsed().saturating_sub(start);
            let done = (after.as_nanos() / period.as_nanos()) as u32;
            next = period * (done + 1);
        } else {
            clock.sleep((next - t).min(MAX_WAIT));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelRect;
    use crate::recording::{CANCEL, LoopConfig, LoopEnd, RUN, STOP};
    use crate::testing::{FakeClock, FakeGrabber, VecSink};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};
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
