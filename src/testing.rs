//! Fakes shared by unit tests.

use crate::clock::Clock;
use chrono::{NaiveDate, NaiveDateTime};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
