//! Lets the capture thread ask the UI thread for things and wait for answers.

use crate::capture::{
    CaptureCoordinator, CaptureOutcome, RecordingJob, RecordingUi, ScreenGrabber, SelectionRequest,
    SelectionUi, ToolbarVisibility,
};
use crate::clock::Clock;
use crate::geometry::{MonitorInfo, PixelRect};
use crate::recording::{RecordingResult, RecordingWorker};
use crate::settings::AppSettings;
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Requests from the capture thread to the UI thread.
pub enum UiRequest {
    HideToolbar,
    ShowToolbar,
    Select {
        request: SelectionRequest,
        reply: mpsc::Sender<Option<PixelRect>>,
    },
    CaptureFinished(CaptureOutcome),
    RecordingStarted {
        region: PixelRect,
        monitor: MonitorInfo,
        max: Duration,
        commands: mpsc::Sender<RecordingCommand>,
    },
    RecordingTick {
        elapsed: Duration,
        max: Duration,
    },
    RecordingEnded,
}

/// Buttons of the recording panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingCommand {
    Stop,
    Cancel,
}

/// How often the panel's timer is refreshed.
const TICK: Duration = Duration::from_millis(250);

/// Runs the recording worker and relays the panel's Stop/Cancel to it.
pub struct BridgeRecordingUi<S: UiSender> {
    pub sender: S,
    pub grabber: Arc<dyn ScreenGrabber>,
    pub clock: Arc<dyn Clock>,
}

impl<S: UiSender> RecordingUi for BridgeRecordingUi<S> {
    fn record(&mut self, job: RecordingJob) -> RecordingResult {
        let (commands, inbox) = mpsc::channel();
        let max = job.config.max_duration;
        self.sender.send(UiRequest::RecordingStarted {
            region: job.region,
            monitor: job.monitor,
            max,
            commands,
        });
        let worker = RecordingWorker::spawn(
            self.grabber.clone(),
            self.clock.clone(),
            job.config,
            job.sink,
            job.path,
        );
        let result = loop {
            match inbox.recv_timeout(TICK) {
                Ok(RecordingCommand::Stop) => break worker.stop(),
                Ok(RecordingCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break worker.cancel();
                }
                Err(mpsc::RecvTimeoutError::Timeout) if worker.is_finished() => {
                    break worker.wait();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.sender.send(UiRequest::RecordingTick { elapsed: worker.elapsed(), max });
                }
            }
        };
        self.sender.send(UiRequest::RecordingEnded);
        result
    }
}

pub trait UiSender: Send + 'static {
    /// `false` once the event loop has gone away.
    fn send(&self, req: UiRequest) -> bool;
}

pub struct BridgeSelectionUi<S: UiSender> {
    pub sender: S,
}

impl<S: UiSender> SelectionUi for BridgeSelectionUi<S> {
    fn select_region(&mut self, request: SelectionRequest) -> Option<PixelRect> {
        let (reply, answer) = mpsc::channel();
        if !self.sender.send(UiRequest::Select { request, reply }) {
            return None;
        }
        answer.recv().ok().flatten()
    }
}

pub struct BridgeToolbar<S: UiSender> {
    pub sender: S,
}

impl<S: UiSender> ToolbarVisibility for BridgeToolbar<S> {
    fn hide(&mut self) {
        self.sender.send(UiRequest::HideToolbar);
    }

    fn show(&mut self) {
        self.sender.send(UiRequest::ShowToolbar);
    }
}

/// Runs captures one at a time on a dedicated thread.
pub struct CaptureThread {
    requests: mpsc::Sender<AppSettings>,
}

impl CaptureThread {
    pub fn spawn<S: UiSender>(mut coordinator: CaptureCoordinator, sender: S) -> CaptureThread {
        let (requests, queue) = mpsc::channel::<AppSettings>();
        std::thread::Builder::new()
            .name("gyozarec-capture".into())
            .spawn(move || {
                for settings in queue {
                    let outcome = coordinator.capture(&settings);
                    if !sender.send(UiRequest::CaptureFinished(outcome)) {
                        break;
                    }
                }
            })
            .expect("spawn capture thread");
        CaptureThread { requests }
    }

    /// Queues a capture; `false` if the thread has stopped.
    pub fn request(&self, settings: AppSettings) -> bool {
        self.requests.send(settings).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureOutcomeKind, SelectionRequest, SelectionUi};
    use crate::geometry::{CaptureArea, PixelRect};
    use crate::testing::{CoordinatorHarness, FakeUiSender};
    use std::time::Duration;

    fn req() -> SelectionRequest {
        SelectionRequest {
            area: CaptureArea::Rectangle,
            frozen: None,
            windows: Vec::new(),
            monitors: Vec::new(),
        }
    }

    #[test]
    fn selection_returns_ui_reply() {
        let mut ui =
            BridgeSelectionUi { sender: FakeUiSender::replying(Some(PixelRect::new(1, 2, 3, 4))) };
        assert_eq!(ui.select_region(req()), Some(PixelRect::new(1, 2, 3, 4)));
    }

    #[test]
    fn closed_event_loop_cancels_selection() {
        let mut ui = BridgeSelectionUi { sender: FakeUiSender::closed() };
        assert_eq!(ui.select_region(req()), None);
    }

    fn job(dir: &std::path::Path, max_s: u64) -> crate::capture::RecordingJob {
        let region = PixelRect::new(0, 0, 32, 32);
        crate::capture::RecordingJob {
            region,
            monitor: crate::geometry::MonitorInfo {
                device_name: "A".into(),
                bounds: PixelRect::new(0, 0, 1920, 1080),
                scale: 1.0,
            },
            path: dir.join("a.gif"),
            config: crate::recording::LoopConfig {
                region,
                fps: 1,
                include_cursor: false,
                max_duration: Duration::from_secs(max_s),
            },
            sink: Box::new(crate::testing::VecSink::default()),
        }
    }

    fn recording_ui(sender: FakeUiSender) -> BridgeRecordingUi<FakeUiSender> {
        BridgeRecordingUi {
            sender,
            grabber: std::sync::Arc::new(crate::testing::FakeGrabber::default()),
            clock: std::sync::Arc::new(crate::testing::FakeClock::default()),
        }
    }

    #[test]
    fn recording_ends_at_max_duration_without_commands() {
        use crate::capture::RecordingUi;
        let d = tempfile::tempdir().unwrap();
        let sender = FakeUiSender::default();
        let log = sender.log.clone();
        let mut ui = recording_ui(sender);
        assert_eq!(ui.record(job(d.path(), 3)).outcome, crate::recording::RecordingOutcome::Saved);
        let ev = log.lock().unwrap().clone();
        assert_eq!(
            (ev.first().unwrap().as_str(), ev.last().unwrap().as_str()),
            ("ui:rec-start", "ui:rec-end")
        );
    }

    #[test]
    fn cancel_command_cancels_recording() {
        use crate::capture::RecordingUi;
        let d = tempfile::tempdir().unwrap();
        let mut ui = recording_ui(FakeUiSender::answering(RecordingCommand::Cancel));
        assert_eq!(
            ui.record(job(d.path(), 3600)).outcome,
            crate::recording::RecordingOutcome::Canceled
        );
    }

    #[test]
    fn capture_thread_runs_capture_and_reports_outcome() {
        let h = CoordinatorHarness::rect(Some(PixelRect::new(0, 0, 10, 10)));
        let (settings, log) = (h.settings(), h.log.clone());
        let sender = FakeUiSender::with_log(log.clone());
        let done = sender.finished_signal();
        let t = CaptureThread::spawn(h.coordinator, sender);
        assert!(t.request(settings));
        let o = done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(o.kind, CaptureOutcomeKind::Saved);
        assert!(log.lock().unwrap().contains(&"ui:finished:Saved".to_string()));
    }
}
