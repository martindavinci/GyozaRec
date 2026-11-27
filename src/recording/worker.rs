// SPDX-License-Identifier: GPL-3.0-or-later
//! Runs the capture loop on its own thread.

use super::{
    CANCEL, FrameSink, LoopConfig, LoopEnd, PAUSE, PauseClock, RUN, RecordingResult, STOP,
    run_capture_loop_with_audio,
};
use crate::capture::ScreenGrabber;
use crate::clock::Clock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct RecordingWorker {
    /// The loop's end and every file the sink wrote.
    handle: JoinHandle<(crate::Result<LoopEnd>, Vec<PathBuf>)>,
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
                let end = run_capture_loop_with_audio(
                    &*grabber,
                    &mut *sink,
                    &*loop_clock,
                    &cfg,
                    &loop_control,
                    &loop_paused_at,
                    audio,
                );
                (end, sink.files())
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
        let files = self.handle.join().map(|(_, files)| files).unwrap_or_default();
        remove_partial(&written(files, self.output));
        RecordingResult::canceled()
    }

    pub fn wait(self) -> RecordingResult {
        let (end, files) = match self.handle.join() {
            Ok((end, files)) => (Ok(end), files),
            // The recording thread panicked.
            Err(_) => (Err(()), Vec::new()),
        };
        let files = written(files, self.output);
        match end {
            Ok(Ok(LoopEnd::Canceled)) => {
                remove_partial(&files);
                RecordingResult::canceled()
            }
            Ok(Ok(_)) => RecordingResult::saved(files),
            Ok(Err(err)) => {
                remove_partial(&files);
                RecordingResult::failed(err.into())
            }
            Err(()) => {
                remove_partial(&files);
                RecordingResult::failed(crate::UserError::RecordingFailed.into())
            }
        }
    }
}

/// The files a sink reported, or the output it was created with.
fn written(files: Vec<PathBuf>, output: PathBuf) -> Vec<PathBuf> {
    if files.is_empty() { vec![output] } else { files }
}

fn remove_partial(paths: &[PathBuf]) {
    for path in paths {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
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
        assert_eq!(w.wait(), RecordingResult::saved(vec![out]));
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

    /// A sink that reports it wrote `files` (as a splitting sink does).
    #[derive(Default)]
    struct PartsSink {
        files: Vec<PathBuf>,
    }

    impl FrameSink for PartsSink {
        fn push(&mut self, _: &crate::pixels::PixelBuffer, _: Duration) -> crate::Result<()> {
            Ok(())
        }
        fn finish(&mut self) -> crate::Result<()> {
            Ok(())
        }
        fn files(&self) -> Vec<PathBuf> {
            self.files.clone()
        }
    }

    #[test]
    fn saved_result_lists_the_output_or_every_part() {
        let d = tempdir().unwrap();
        let out = d.path().join("a.mp4");
        let spawn = |sink: PartsSink| {
            RecordingWorker::spawn(
                Arc::new(FakeGrabber::default()),
                Arc::new(FakeClock::default()),
                cfg(),
                Box::new(sink),
                out.clone(),
            )
        };
        assert_eq!(spawn(PartsSink::default()).wait().parts, vec![out.clone()]);
        let parts = vec![out.clone(), d.path().join("a_part2.mp4")];
        let result = spawn(PartsSink { files: parts.clone() }).wait();
        assert_eq!((result.path, result.parts), (Some(out.clone()), parts));
    }

    #[test]
    fn cancel_deletes_every_part() {
        let d = tempdir().unwrap();
        let out = d.path().join("a.mp4");
        let parts = vec![out.clone(), d.path().join("a_part2.mp4")];
        for p in &parts {
            fs::write(p, b"partial").unwrap();
        }
        let w = RecordingWorker::spawn(
            Arc::new(FakeGrabber::default()),
            Arc::new(FakeClock::default()),
            cfg(),
            Box::new(PartsSink { files: parts.clone() }),
            out,
        );
        // Let the loop finish first, so the sink has reported its files.
        while !w.is_finished() {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(w.cancel().outcome, RecordingOutcome::Canceled);
        assert!(parts.iter().all(|p| !p.exists()));
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
