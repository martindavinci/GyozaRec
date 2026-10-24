//! Fakes shared by unit tests. Later tasks pick them up one by one.
#![allow(dead_code)]

use crate::capture::{
    CursorLocator, MonitorProvider, ScreenGrabber, WindowCandidate, WindowSnapshotter,
};
use crate::clock::Clock;
use crate::geometry::{MonitorInfo, PixelPoint, PixelRect};
use crate::pixels::PixelBuffer;
use chrono::{NaiveDate, NaiveDateTime};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Ordered record of what fakes were asked to do.
pub type EventLog = Arc<Mutex<Vec<String>>>;

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
