//! Runs the capture loop on its own thread.

use super::{
    CANCEL, FrameSink, LoopConfig, LoopEnd, RecordingOutcome, RecordingResult, STOP,
    run_capture_loop,
};
use crate::capture::ScreenGrabber;
use crate::clock::Clock;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct RecordingWorker {
    handle: JoinHandle<crate::Result<LoopEnd>>,
    control: Arc<AtomicU8>,
    clock: Arc<dyn Clock>,
    started: Duration,
    output: PathBuf,
}

impl RecordingWorker {
    pub fn spawn(
        grabber: Arc<dyn ScreenGrabber>,
        clock: Arc<dyn Clock>,
        cfg: LoopConfig,
        mut sink: Box<dyn FrameSink>,
        output: PathBuf,
    ) -> RecordingWorker {
        let control = Arc::new(AtomicU8::new(super::RUN));
        let started = clock.elapsed();
        let (loop_control, loop_clock) = (control.clone(), clock.clone());
        let handle = std::thread::Builder::new()
            .name("gyozarec-recording".into())
            .spawn(move || {
                // The sink (and its open file) is dropped when this returns.
                run_capture_loop(&*grabber, &mut *sink, &*loop_clock, &cfg, &loop_control)
            })
            .expect("spawn recording thread");
        RecordingWorker { handle, control, clock, started, output }
    }

    pub fn elapsed(&self) -> Duration {
        self.clock.elapsed().saturating_sub(self.started)
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
        let failed = |error: String| RecordingResult {
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
                failed(err.to_string())
            }
            Err(_) => {
                remove_partial(&self.output);
                failed("errore interno durante la registrazione".into())
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
