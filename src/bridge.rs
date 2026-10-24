//! Lets the capture thread ask the UI thread for things and wait for answers.

use crate::capture::{
    CaptureCoordinator, CaptureOutcome, SelectionRequest, SelectionUi, ToolbarVisibility,
};
use crate::geometry::PixelRect;
use crate::settings::AppSettings;
use std::sync::mpsc;

/// Requests from the capture thread to the UI thread.
pub enum UiRequest {
    HideToolbar,
    ShowToolbar,
    Select { request: SelectionRequest, reply: mpsc::Sender<Option<PixelRect>> },
    CaptureFinished(CaptureOutcome),
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
    use crate::geometry::{CaptureMode, PixelRect};
    use crate::testing::{CoordinatorHarness, FakeUiSender};
    use std::time::Duration;

    fn req() -> SelectionRequest {
        SelectionRequest {
            mode: CaptureMode::Rectangle,
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
