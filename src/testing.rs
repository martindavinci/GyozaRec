//! Fakes shared by unit tests. Later tasks pick them up one by one.
#![allow(dead_code)]

use crate::bridge::{RecordingCommand, UiRequest, UiSender};
use crate::capture::{
    CaptureCoordinator, CaptureOutcome, ClipboardService, CursorLocator, EditorLauncher,
    FileRevealer, MonitorProvider, RecordingJob, RecordingUi, ScreenGrabber, Selection,
    SelectionRequest, SelectionUi, SinkFactory, SinkKind, SoundPlayer, ToolbarVisibility,
    WindowCandidate, WindowSnapshotter,
};
use crate::clock::Clock;
use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};
use crate::pixels::PixelBuffer;
use crate::recording::{FrameSink, RecordingOutcome, RecordingResult};
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
    /// (start frame, sample count) of every audio push.
    audio: Arc<Mutex<Vec<(u64, usize)>>>,
    /// Every audio sample, in push order.
    samples: Arc<Mutex<Vec<i16>>>,
}

impl VecSink {
    pub fn audio(&self) -> Vec<(u64, usize)> {
        self.audio.lock().unwrap().clone()
    }

    pub fn audio_samples(&self) -> Vec<i16> {
        self.samples.lock().unwrap().clone()
    }

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

    fn push_audio(&mut self, samples: &[i16], start_frame: u64) -> crate::Result<()> {
        self.audio.lock().unwrap().push((start_frame, samples.len()));
        self.samples.lock().unwrap().extend_from_slice(samples);
        Ok(())
    }
}

/// Stands in for the event loop: answers selections at once, records requests.
pub struct FakeUiSender {
    pub log: EventLog,
    select_reply: Option<PixelRect>,
    closed: bool,
    finished: Mutex<Option<std::sync::mpsc::Sender<CaptureOutcome>>>,
    /// Sent back as soon as a recording starts.
    answer: Vec<RecordingCommand>,
    /// Kept alive like the real panel does while recording.
    held: Mutex<Option<std::sync::mpsc::Sender<RecordingCommand>>>,
}

impl Default for FakeUiSender {
    fn default() -> Self {
        Self {
            log: Arc::default(),
            select_reply: None,
            closed: false,
            finished: Mutex::new(None),
            answer: Vec::new(),
            held: Mutex::new(None),
        }
    }
}

impl FakeUiSender {
    pub fn answering(command: RecordingCommand) -> Self {
        Self::answering_all(vec![command])
    }

    /// Sends these panel commands, in order, as soon as a recording starts.
    pub fn answering_all(commands: Vec<RecordingCommand>) -> Self {
        Self { answer: commands, ..Self::default() }
    }

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
                let _ = reply.send(self.select_reply.map(Selection::from));
            }
            UiRequest::CaptureFinished(outcome) => {
                log_event(&log, format!("ui:finished:{:?}", outcome.kind));
                if let Some(tx) = self.finished.lock().unwrap().as_ref() {
                    let _ = tx.send(outcome);
                }
            }
            UiRequest::RecordingStarted { commands, .. } => {
                log_event(&log, "ui:rec-start");
                for cmd in &self.answer {
                    let _ = commands.send(*cmd);
                }
                *self.held.lock().unwrap() = Some(commands);
            }
            UiRequest::OpenEditor { .. } => log_event(&log, "ui:editor"),
            UiRequest::Notify(_) => log_event(&log, "ui:notice"),
            UiRequest::RecordingTick { .. } => log_event(&log, "ui:rec-tick"),
            UiRequest::RecordingEnded => {
                log_event(&log, "ui:rec-end");
                self.held.lock().unwrap().take();
            }
        }
        true
    }
}

/// Records the job and answers with a preset outcome, without recording.
pub struct FakeRecordingUi {
    pub outcome: RecordingOutcome,
    pub log: EventLog,
}

impl RecordingUi for FakeRecordingUi {
    fn record(&mut self, job: RecordingJob) -> RecordingResult {
        let ext =
            job.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        log_event(
            &Some(self.log.clone()),
            format!(
                "record:{}:{ext}:fps={}:max={}",
                rect_label(&job.region),
                job.config.fps,
                job.config.max_duration.as_secs()
            ),
        );
        let path = (self.outcome == RecordingOutcome::Saved).then_some(job.path);
        RecordingResult { outcome: self.outcome, path, error: None }
    }
}

pub struct FakeSinkFactory {
    pub log: EventLog,
}

impl SinkFactory for FakeSinkFactory {
    fn create(
        &self,
        kind: &SinkKind,
        _path: &Path,
        size: crate::geometry::PixelSize,
        _fps: u32,
    ) -> crate::Result<Box<dyn FrameSink>> {
        log_event(&Some(self.log.clone()), format!("sink:{kind:?}:{}x{}", size.width, size.height));
        Ok(Box::new(VecSink::default()))
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
        let monitor =
            MonitorInfo { device_name: "A".into(), bounds, work_area: bounds, scale: 1.0 };
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
    pub result: Option<Selection>,
    pub log: EventLog,
}

impl SelectionUi for FakeSelection {
    fn select_region(&mut self, req: SelectionRequest) -> Option<Selection> {
        log_event(
            &Some(self.log.clone()),
            format!(
                "select:{:?}:frozen={}:windows={}{}",
                req.area,
                req.frozen.is_some(),
                req.windows.len(),
                if req.switcher { ":bar" } else { "" }
            ),
        );
        self.result.clone()
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
    fn play(&self, cue: crate::sounds::Cue) {
        let name = match cue {
            crate::sounds::Cue::Saved => "sound".to_string(),
            other => format!("sound:{}", format!("{other:?}").to_lowercase()),
        };
        log_event(&Some(self.log.clone()), name);
    }
}

/// Logs each start; hands out a silent feed when a device is "present".
pub struct FakeAudioStarter {
    pub available: bool,
    pub log: EventLog,
}

impl crate::capture::AudioStarter for FakeAudioStarter {
    fn start(&self, _clock: Arc<dyn Clock>) -> Option<crate::audio::AudioFeed> {
        log_event(&Some(self.log.clone()), "audio:start");
        self.available.then(|| {
            let (_tx, rx) = std::sync::mpsc::channel();
            crate::audio::AudioFeed::new(rx, Box::new(|| {}), false)
        })
    }
}

/// Logs `editor:<file>:<w>x<h>:work=<x>,<y>,<w>,<h>:scale=<s>`, with
/// `:unsaved` at the end when the file is not written.
pub struct FakeEditor {
    pub log: EventLog,
}

impl EditorLauncher for FakeEditor {
    fn open(
        &self,
        image: Arc<PixelBuffer>,
        path: std::path::PathBuf,
        on_disk: bool,
        work: PixelRect,
        scale: f64,
    ) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (w, h) = (image.width(), image.height());
        let area = format!("{},{},{},{}", work.x, work.y, work.width, work.height);
        log_event(
            &Some(self.log.clone()),
            format!(
                "editor:{name}:{w}x{h}:work={area}:scale={scale}{}",
                if on_disk { "" } else { ":unsaved" }
            ),
        );
    }
}

/// Logs `notice:<image|video>:<file>:<thumbnail w>x<h>`.
pub struct FakeNotifier {
    pub log: EventLog,
}

impl crate::capture::Notifier for FakeNotifier {
    fn notify(&self, n: crate::presentation::Notice) {
        let name = n.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let kind = format!("{:?}", n.kind).to_lowercase();
        let (w, h) = (n.thumbnail.width(), n.thumbnail.height());
        log_event(&Some(self.log.clone()), format!("notice:{kind}:{name}:{w}x{h}"));
    }
}

pub struct FakeRevealer {
    pub log: EventLog,
}

impl FileRevealer for FakeRevealer {
    fn reveal(&self, path: &Path) {
        log_event(&Some(self.log.clone()), format!("reveal:{}", path.display()));
    }

    fn open(&self, path: &Path) {
        log_event(&Some(self.log.clone()), format!("open:{}", path.display()));
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
            selection: Box::new(FakeSelection {
                result: selection.map(Selection::from),
                log: log.clone(),
            }),
            clipboard: Box::new(FakeClipboard { log: log.clone() }),
            sound: Box::new(FakeSound { log: log.clone() }),
            revealer: Box::new(FakeRevealer { log: log.clone() }),
            editor: Box::new(FakeEditor { log: log.clone() }),
            notifier: Box::new(FakeNotifier { log: log.clone() }),
            audio: Box::new(FakeAudioStarter { available: false, log: log.clone() }),
            toolbar: Box::new(FakeToolbar { log: log.clone() }),
            recording_ui: Box::new(FakeRecordingUi {
                outcome: RecordingOutcome::Saved,
                log: log.clone(),
            }),
            sinks: Box::new(FakeSinkFactory { log: log.clone() }),
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
        Self::two_monitors_with_selection(cursor, None)
    }

    /// Monitor A at the origin, B to its left; the selection answers `selection`.
    pub fn two_monitors_with_selection(cursor: PixelPoint, selection: Option<PixelRect>) -> Self {
        let a = MonitorInfo {
            device_name: "A".into(),
            bounds: PixelRect::new(0, 0, 1920, 1080),
            work_area: PixelRect::new(0, 0, 1920, 1080),
            scale: 1.0,
        };
        let b = MonitorInfo {
            device_name: "B".into(),
            bounds: PixelRect::new(-1920, 0, 1920, 1080),
            work_area: PixelRect::new(-1920, 0, 1920, 1080),
            scale: 1.5,
        };
        let monitors = FakeMonitors {
            monitors: vec![a, b],
            virtual_screen: PixelRect::new(-1920, 0, 3840, 1080),
        };
        Self::build(selection, monitors, cursor, Vec::new())
    }

    /// The selection answers a lasso drawn along `outline`, inside `rect`.
    pub fn lasso(mut self, rect: PixelRect, outline: Vec<PixelPoint>) -> Self {
        let result = Some(Selection { rect, outline });
        self.coordinator.selection = Box::new(FakeSelection { result, log: self.log.clone() });
        self
    }

    /// An audio device is present.
    pub fn with_audio(mut self) -> Self {
        self.coordinator.audio =
            Box::new(FakeAudioStarter { available: true, log: self.log.clone() });
        self
    }

    /// Makes the fake recording UI answer with `outcome`.
    pub fn recording(mut self, outcome: RecordingOutcome) -> Self {
        self.coordinator.recording_ui =
            Box::new(FakeRecordingUi { outcome, log: self.log.clone() });
        self
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
    on_sleep: Option<Arc<dyn Fn(Duration) + Send + Sync>>,
}

impl FakeClock {
    pub fn new(local_start: NaiveDateTime) -> Self {
        Self { local_start, state: Arc::default(), on_sleep: None }
    }

    /// Calls `hook` with the elapsed time after every `sleep`.
    pub fn with_sleep_hook(self, hook: impl Fn(Duration) + Send + Sync + 'static) -> Self {
        Self { on_sleep: Some(Arc::new(hook)), ..self }
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
        let elapsed = {
            let mut s = self.state.lock().unwrap();
            s.sleeps.push(d);
            s.elapsed += d;
            s.elapsed
        };
        if let Some(hook) = &self.on_sleep {
            hook(elapsed);
        }
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
