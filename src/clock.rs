//! Injectable time source, so tests control waits and timestamps.

use std::time::{Duration, Instant};

pub trait Clock: Send + Sync {
    /// Monotonic time since the clock was created.
    fn elapsed(&self) -> Duration;
    fn sleep(&self, d: Duration);
    fn local_now(&self) -> chrono::NaiveDateTime;
}

pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self { start: Instant::now() }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }

    fn local_now(&self) -> chrono::NaiveDateTime {
        chrono::Local::now().naive_local()
    }
}
