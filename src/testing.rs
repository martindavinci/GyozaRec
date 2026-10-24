//! Fakes shared by unit tests. Later tasks pick them up one by one.
#![allow(dead_code)]

use crate::bridge::{UiRequest, UiSender};
use crate::capture::{
    CaptureCoordinator, CaptureOutcome, ClipboardService, CursorLocator, MonitorProvider,
    ScreenGrabber, SelectionRequest, SelectionUi, SoundPlayer, ToolbarVisibility, WindowCandidate,
    WindowSnapshotter,
};
use crate::clock::Clock;
use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};
use crate::pixels::PixelBuffer;
use crate::settings::AppSettings;
use chrono::{NaiveDate, NaiveDateTime};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Ordered record of what fakes were asked to do.
pub type EventLog = Arc<Mutex<Vec<String>>>;

/// Frame sink that only records timestamps.
#[derive(Clone, Default)]
pub struct VecSink {
    times: Arc<Mutex<Vec<Duration>>>,
    finished: Arc<std::sync::atomic::AtomicBool>,
}

impl VecSink {
    pub fn times(&self) -> Vec<Duration> {
        self.times.lock().unwrap().clone()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

impl crate::recording::FrameSink for VecSink {
    fn push(&mut self, _frame: &PixelBuffer, t: Duration) -> crate::Result<()> {
        self.times.lock().unwrap().push(t);
        Ok(())
    }

    fn finish(&mut self) -> crate::Result<()> {
        self.finished.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// Stands in for the event loop: answers selections at once, records requests.
pub struct FakeUiSender {
    pub log: EventLog,
    select_reply: Option<PixelRect>,
    closed: bool,
    finished: Mutex<Option<std::sync::mpsc::Sender<CaptureOutcome>>>,
}

impl Default for FakeUiSender {
    fn default() -> Self {
        Self { log: Arc::default(), select_reply: None, closed: false, finished: Mutex::new(None) }
    }
}

impl FakeUiSender {
    pub fn replying(reply: Option<PixelRect>) -> Self {
        Self { select_reply: reply, ..Self::default() }
    }

    pub fn closed() -> Self {
        Self { closed: true, ..Self::default() }
    }

    pub fn with_log(log: EventLog) -> Self {
        Self { log, ..Self::default() }
    }

    /// Receives every `CaptureFinished` outcome.
    pub fn finished_signal(&self) -> std::sync::mpsc::Receiver<CaptureOutcome> {
        let (tx, rx) = std::sync::mpsc::channel();
        *self.finished.lock().unwrap() = Some(tx);
        rx
    }
}

impl UiSender for FakeUiSender {
    fn send(&self, req: UiRequest) -> bool {
        if self.closed {
            return false;
        }
        let log = Some(self.log.clone());
        match req {
            UiRequest::HideToolbar => log_event(&log, "ui:hide"),
            UiRequest::ShowToolbar => log_event(&log, "ui:show"),
            UiRequest::Select { reply, .. } => {
                log_event(&log, "ui:select");
                let _ = reply.send(self.select_reply);
            }
            UiRequest::CaptureFinished(outcome) => {
                log_event(&log, format!("ui:finished:{:?}", outcome.kind));
                if let Some(tx) = self.finished.lock().unwrap().as_ref() {
                    let _ = tx.send(outcome);
                }
            }
        }
        true
    }
}

pub fn log_event(log: &Option<EventLog>, event: impl Into<String>) {
    if let Some(log) = log {
        log.lock().unwrap().push(event.into());
    }
}

pub fn rect_label(r: &PixelRect) -> String {
    format!("{},{},{},{}", r.x, r.y, r.width, r.height)
}

#[derive(Clone)]
pub struct FakeMonitors {
    pub monitors: Vec<MonitorInfo>,
    pub virtual_screen: PixelRect,
}

impl FakeMonitors {
    pub fn single(bounds: PixelRect) -> Self {
        let monitor = MonitorInfo { device_name: "A".into(), bounds, scale: 1.0 };
        Self { monitors: vec![monitor], virtual_screen: bounds }
    }
}

impl MonitorProvider for FakeMonitors {
    fn monitors(&self) -> Vec<MonitorInfo> {
        self.monitors.clone()
    }

    fn virtual_screen(&self) -> PixelRect {
        self.virtual_screen
    }
}

/// Returns solid buffers; records every requested rect.
#[derive(Clone, Default)]
pub struct FakeGrabber {
    pub calls: Arc<Mutex<Vec<PixelRect>>>,
    pub log: Option<EventLog>,
    cost: Duration,
    clock: Option<FakeClock>,
    count: Arc<AtomicUsize>,
    on_grab: Option<Arc<dyn Fn(usize) + Send + Sync>>,
}

impl FakeGrabber {
    /// Each grab advances `clock` by `cost`.
    pub fn slow(clock: &FakeClock, cost: Duration) -> Self {
        Self { cost, clock: Some(clock.clone()), ..Self::default() }
    }

    /// `hook` gets the 1-based grab number after the grab is recorded.
    pub fn with_hook(hook: impl Fn(usize) + Send + Sync + 'static) -> Self {
        Self { on_grab: Some(Arc::new(hook)), ..Self::default() }
    }

    pub fn with_log(log: EventLog) -> Self {
        Self { log: Some(log), ..Self::default() }
    }
}

impl ScreenGrabber for FakeGrabber {
    fn grab(&self, rect: PixelRect, _include_cursor: bool) -> crate::Result<PixelBuffer> {
        self.calls.lock().unwrap().push(rect);
        log_event(&self.log, format!("grab:{}", rect_label(&rect)));
        if let Some(clock) = &self.clock {
            clock.advance(self.cost);
        }
        let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(hook) = &self.on_grab {
            hook(n);
        }
        Ok(PixelBuffer::solid(rect, 40, 80, 120))
    }
}

pub struct FakeCursor(pub PixelPoint);

impl CursorLocator for FakeCursor {
    fn position(&self) -> PixelPoint {
        self.0
    }
}

pub struct FakeSelection {
    pub result: Option<PixelRect>,
    pub log: EventLog,
}

impl SelectionUi for FakeSelection {
    fn select_region(&mut self, req: SelectionRequest) -> Option<PixelRect> {
        log_event(
            &Some(self.log.clone()),
            format!(
                "select:{:?}:frozen={}:windows={}",
                req.mode,
                req.frozen.is_some(),
                req.windows.len()
            ),
        );
        self.result
    }
}

pub struct FakeClipboard {
    pub log: EventLog,
}

impl ClipboardService for FakeClipboard {
    fn set_image(&self, img: &PixelBuffer) -> bool {
        log_event(&Some(self.log.clone()), format!("clip:image:{}", rect_label(&img.bounds)));
        true
    }

    fn set_file(&self, path: &Path) -> bool {
        log_event(&Some(self.log.clone()), format!("clip:file:{}", path.display()));
        true
    }
}

pub struct FakeSound {
    pub log: EventLog,
}

impl SoundPlayer for FakeSound {
    fn play_capture(&self) {
        log_event(&Some(self.log.clone()), "sound");
    }
}

pub struct FakeToolbar {
    pub log: EventLog,
}

impl ToolbarVisibility for FakeToolbar {
    fn hide(&mut self) {
        log_event(&Some(self.log.clone()), "hide");
    }

    fn show(&mut self) {
        log_event(&Some(self.log.clone()), "show");
    }
}

/// A coordinator wired to fakes, a fake clock and a temporary capture folder.
pub struct CoordinatorHarness {
    pub coordinator: CaptureCoordinator,
    pub clock: FakeClock,
    pub log: EventLog,
    pub temp: tempfile::TempDir,
}

impl CoordinatorHarness {
    fn build(
        selection: Option<PixelRect>,
        monitors: FakeMonitors,
        cursor: PixelPoint,
        windows: Vec<WindowCandidate>,
    ) -> Self {
        let log: EventLog = Arc::default();
        let clock = FakeClock::default();
        let coordinator = CaptureCoordinator {
            monitors: Box::new(monitors),
            grabber: Arc::new(FakeGrabber::with_log(log.clone())),
            cursor: Box::new(FakeCursor(cursor)),
            windows: Box::new(FakeWindows(windows)),
            selection: Box::new(FakeSelection { result: selection, log: log.clone() }),
            clipboard: Box::new(FakeClipboard { log: log.clone() }),
            sound: Box::new(FakeSound { log: log.clone() }),
            toolbar: Box::new(FakeToolbar { log: log.clone() }),
            clock: Arc::new(clock.clone()),
        };
        Self { coordinator, clock, log, temp: tempfile::tempdir().unwrap() }
    }

    /// One 1920×1080 monitor at the origin.
    pub fn rect(selection: Option<PixelRect>) -> Self {
        Self::with_windows(selection, Vec::new())
    }

    pub fn with_windows(selection: Option<PixelRect>, windows: Vec<WindowCandidate>) -> Self {
        let monitors = FakeMonitors::single(PixelRect::new(0, 0, 1920, 1080));
        Self::build(selection, monitors, PixelPoint::default(), windows)
    }

    /// A (0,0,1920,1080) at scale 1.0 and B (-1920,0,1920,1080) at scale 1.5.
    pub fn two_monitors_cursor_at(cursor: PixelPoint) -> Self {
        let a = MonitorInfo {
            device_name: "A".into(),
            bounds: PixelRect::new(0, 0, 1920, 1080),
            scale: 1.0,
        };
        let b = MonitorInfo {
            device_name: "B".into(),
            bounds: PixelRect::new(-1920, 0, 1920, 1080),
            scale: 1.5,
        };
        let monitors = FakeMonitors {
            monitors: vec![a, b],
            virtual_screen: PixelRect::new(-1920, 0, 3840, 1080),
        };
        Self::build(None, monitors, cursor, Vec::new())
    }

    pub fn run(&mut self, settings: AppSettings) -> CaptureOutcome {
        self.coordinator.capture(&settings)
    }

    pub fn settings(&self) -> AppSettings {
        AppSettings::defaults(self.temp.path())
    }

    pub fn dir(&self) -> &Path {
        self.temp.path()
    }

    pub fn events(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

pub struct FakeWindows(pub Vec<WindowCandidate>);

impl WindowSnapshotter for FakeWindows {
    fn snapshot(&self) -> Vec<WindowCandidate> {
        self.0.clone()
    }
}

#[derive(Default)]
struct FakeClockState {
    elapsed: Duration,
    sleeps: Vec<Duration>,
}

/// Virtual clock: `sleep` records the wait and advances time instantly.
#[derive(Clone)]
pub struct FakeClock {
    local_start: NaiveDateTime,
    state: Arc<Mutex<FakeClockState>>,
}

impl FakeClock {
    pub fn new(local_start: NaiveDateTime) -> Self {
        Self { local_start, state: Arc::default() }
    }

    pub fn advance(&self, d: Duration) {
        self.state.lock().unwrap().elapsed += d;
    }

    pub fn sleeps(&self) -> Vec<Duration> {
        self.state.lock().unwrap().sleeps.clone()
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new(NaiveDate::from_ymd_opt(2026, 10, 24).unwrap().and_hms_opt(12, 0, 0).unwrap())
    }
}

impl Clock for FakeClock {
    fn elapsed(&self) -> Duration {
        self.state.lock().unwrap().elapsed
    }

    fn sleep(&self, d: Duration) {
        let mut s = self.state.lock().unwrap();
        s.sleeps.push(d);
        s.elapsed += d;
    }

    fn local_now(&self) -> NaiveDateTime {
        let elapsed = self.elapsed();
        self.local_start + chrono::Duration::from_std(elapsed).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use std::time::Duration;

    #[test]
    fn fake_clock_sleep_advances_time() {
        let c = FakeClock::default();
        c.sleep(Duration::from_secs(2));
        assert_eq!(
            (c.elapsed(), c.sleeps()),
            (Duration::from_secs(2), vec![Duration::from_secs(2)])
        );
    }

    #[test]
    fn fake_clock_advance_moves_local_time_without_recording_sleep() {
        let c = FakeClock::default();
        c.advance(Duration::from_secs(90));
        assert_eq!(c.local_now().format("%H:%M:%S").to_string(), "12:01:30");
        assert!(c.sleeps().is_empty());
    }
}
