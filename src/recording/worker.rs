//! Runs the capture loop on its own thread.

use super::{
    CANCEL, FrameSink, LoopConfig, LoopEnd, PAUSE, PauseClock, RUN, RecordingOutcome,
    RecordingResult, STOP, run_capture_loop_with_audio,
};
use crate::capture::ScreenGrabber;
use crate::clock::Clock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct RecordingWorker {
    handle: JoinHandle<crate::Result<LoopEnd>>,
    control: Arc<AtomicU8>,
    /// When the last pause was asked, in nanoseconds on `clock`.
    paused_at: Arc<AtomicU64>,
    clock: Arc<dyn Clock>,
    started: Duration,
    pauses: Mutex<PauseClock>,
    output: PathBuf,
}

impl RecordingWorker {
    pub fn spawn(
        grabber: Arc<dyn ScreenGrabber>,
        clock: Arc<dyn Clock>,
        cfg: LoopConfig,
        sink: Box<dyn FrameSink>,
        output: PathBuf,
    ) -> RecordingWorker {
        RecordingWorker::spawn_with_audio(grabber, clock, cfg, sink, output, None)
    }

    pub fn spawn_with_audio(
        grabber: Arc<dyn ScreenGrabber>,
        clock: Arc<dyn Clock>,
        cfg: LoopConfig,
        mut sink: Box<dyn FrameSink>,
        output: PathBuf,
        audio: Option<crate::audio::AudioFeed>,
    ) -> RecordingWorker {
        let control = Arc::new(AtomicU8::new(super::RUN));
        let paused_at = Arc::new(AtomicU64::new(u64::MAX));
        let started = clock.elapsed();
        let (loop_control, loop_paused_at, loop_clock) =
            (control.clone(), paused_at.clone(), clock.clone());
        let handle = std::thread::Builder::new()
            .name("gyozarec-recording".into())
            .spawn(move || {
                // The sink (and its open file) is dropped when this returns.
                run_capture_loop_with_audio(
                    &*grabber,
                    &mut *sink,
                    &*loop_clock,
                    &cfg,
                    &loop_control,
                    &loop_paused_at,
                    audio,
                )
            })
            .expect("spawn recording thread");
        let pauses = Mutex::new(PauseClock::default());
        RecordingWorker { handle, control, paused_at, clock, started, pauses, output }
    }

    #[cfg(test)]
    pub fn control(&self) -> Arc<AtomicU8> {
        self.control.clone()
    }

    /// Recorded time so far, without pauses.
    pub fn elapsed(&self) -> Duration {
        let pauses = self.pauses.lock().unwrap_or_else(|e| e.into_inner());
        pauses.active(self.started, self.clock.elapsed())
    }

    /// The recording leaves out everything from this moment, even if the loop
    /// notices the pause later.
    pub fn pause(&self) {
        let now = self.clock.elapsed();
        let nanos = u64::try_from(now.as_nanos()).unwrap_or(u64::MAX);
        self.paused_at.store(nanos, Ordering::SeqCst);
        if self.control.compare_exchange(RUN, PAUSE, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            self.pauses.lock().unwrap_or_else(|e| e.into_inner()).pause(now);
        }
    }

    pub fn resume(&self) {
        if self.control.compare_exchange(PAUSE, RUN, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            self.pauses.lock().unwrap_or_else(|e| e.into_inner()).resume(self.clock.elapsed());
        }
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    pub fn stop(self) -> RecordingResult {
        self.control.store(STOP, Ordering::SeqCst);
        self.wait()
    }

    /// Discards the recording, even if the loop had already finished.
    pub fn cancel(self) -> RecordingResult {
        self.control.store(CANCEL, Ordering::SeqCst);
        let _ = self.handle.join();
        remove_partial(&self.output);
        RecordingResult { outcome: RecordingOutcome::Canceled, path: None, error: None }
    }

    pub fn wait(self) -> RecordingResult {
        let failed = |error: crate::Failure| RecordingResult {
            outcome: RecordingOutcome::Failed,
            path: None,
            error: Some(error),
        };
        match self.handle.join() {
            Ok(Ok(LoopEnd::Canceled)) => {
                remove_partial(&self.output);
                RecordingResult { outcome: RecordingOutcome::Canceled, path: None, error: None }
            }
            Ok(Ok(_)) => RecordingResult {
                outcome: RecordingOutcome::Saved,
                path: Some(self.output),
                error: None,
            },
            Ok(Err(err)) => {
                remove_partial(&self.output);
                failed(err.into())
            }
            Err(_) => {
                // The recording thread panicked.
                remove_partial(&self.output);
                failed(crate::UserError::RecordingFailed.into())
            }
        }
    }
}

fn remove_partial(path: &std::path::Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PixelRect;
    use crate::recording::{LoopConfig, RecordingOutcome, RecordingResult};
    use crate::testing::{FakeClock, FakeGrabber, VecSink};
    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;

    fn cfg() -> LoopConfig {
        LoopConfig {
            region: PixelRect::new(0, 0, 32, 32),
            fps: 10,
            include_cursor: false,
            max_duration: Duration::from_millis(1000),
        }
    }

    #[test]
    fn worker_wait_reports_saved_path() {
        let d = tempdir().unwrap();
        let out = d.path().join("a.gif");
        let w = RecordingWorker::spawn(
            Arc::new(FakeGrabber::default()),
            Arc::new(FakeClock::default()),
            cfg(),
            Box::new(VecSink::default()),
            out.clone(),
        );
        assert_eq!(
            w.wait(),
            RecordingResult { outcome: RecordingOutcome::Saved, path: Some(out), error: None }
        );
    }

    #[test]
    fn worker_pause_and_resume_switch_the_control_flag() {
        use crate::recording::{PAUSE, RUN, STOP};
        use std::sync::Mutex;
        use std::sync::atomic::Ordering;
        use std::sync::mpsc;
        let (entered_tx, entered) = mpsc::channel::<()>();
        let (release, release_rx) = mpsc::channel::<()>();
        let (entered_tx, release_rx) = (Mutex::new(entered_tx), Mutex::new(release_rx));
        let grabber = FakeGrabber::with_hook(move |n| {
            if n == 1 {
                entered_tx.lock().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
        });
        let d = tempdir().unwrap();
        let w = RecordingWorker::spawn(
            Arc::new(grabber),
            Arc::new(FakeClock::default()),
            LoopConfig { max_duration: Duration::from_secs(60), ..cfg() },
            Box::new(VecSink::default()),
            d.path().join("a.gif"),
        );
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let control = w.control();
        w.pause();
        assert_eq!(control.load(Ordering::SeqCst), PAUSE);
        w.resume();
        assert_eq!(control.load(Ordering::SeqCst), RUN);
        control.store(STOP, Ordering::SeqCst);
        w.pause();
        assert_eq!(control.load(Ordering::SeqCst), STOP, "pause must not undo a stop");
        release.send(()).unwrap();
        assert_eq!(w.wait().outcome, RecordingOutcome::Saved);
    }

    #[test]
    fn cancelled_gif_recording_deletes_the_file() {
        use crate::geometry::PixelSize;
        use crate::recording::{CANCEL, GifSink};
        use std::sync::Mutex;
        use std::sync::atomic::Ordering;
        use std::sync::mpsc;
        let (release, release_rx) = mpsc::channel::<()>();
        let (entered_tx, entered) = mpsc::channel::<()>();
        let (release_rx, entered_tx) = (Mutex::new(release_rx), Mutex::new(entered_tx));
        let grabber = FakeGrabber::with_hook(move |n| {
            if n == 3 {
                entered_tx.lock().unwrap().send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
        });
        let d = tempdir().unwrap();
        let out = d.path().join("a.gif");
        let sink = GifSink::create(&out, PixelSize { width: 32, height: 32 }, 800, 10).unwrap();
        let w = RecordingWorker::spawn(
            Arc::new(grabber),
            Arc::new(FakeClock::default()),
            LoopConfig { max_duration: Duration::from_secs(60), ..cfg() },
            Box::new(sink),
            out.clone(),
        );
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        w.control().store(CANCEL, Ordering::SeqCst);
        release.send(()).unwrap();
        assert_eq!(w.cancel().outcome, RecordingOutcome::Canceled);
        assert!(!out.exists(), "the partial GIF must be deleted");
    }

    #[test]
    fn worker_cancel_deletes_output() {
        let d = tempdir().unwrap();
        let out = d.path().join("a.gif");
        fs::write(&out, b"partial").unwrap();
        let w = RecordingWorker::spawn(
            Arc::new(FakeGrabber::default()),
            Arc::new(FakeClock::default()),
            cfg(),
            Box::new(VecSink::default()),
            out.clone(),
        );
        assert_eq!(w.cancel().outcome, RecordingOutcome::Canceled);
        assert!(!out.exists());
    }
}
