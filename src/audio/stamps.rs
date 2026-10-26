//! Keeps device timestamps honest: a packet cannot end after the moment it
//! was read, so the smallest gap seen between the two is the clock error.

use std::time::Duration;

/// Corrects a source whose timestamps run ahead of (or behind) the
/// recording clock by a constant amount.
#[derive(Default)]
pub struct StampFix {
    /// Nanoseconds to add to device times; only ever decreases.
    correction: Option<i128>,
}

impl StampFix {
    /// Learns from a packet stamped `stamped`, `length` long, read at `read_at`.
    pub fn observe(&mut self, stamped: Duration, length: Duration, read_at: Duration) {
        let gap = nanos(read_at) - nanos(stamped + length);
        self.correction = Some(self.correction.map_or(gap, |c| c.min(gap)));
    }

    /// Where a packet stamped `stamped` really starts, as far as known.
    pub fn apply(&self, stamped: Duration) -> Duration {
        let start = (nanos(stamped) + self.correction.unwrap_or(0)).max(0);
        Duration::from_nanos(start as u64)
    }

    /// `observe`, then `apply`, for a packet read on its own.
    pub fn fix(&mut self, stamped: Duration, length: Duration, read_at: Duration) -> Duration {
        self.observe(stamped, length, read_at);
        self.apply(stamped)
    }
}

fn nanos(d: Duration) -> i128 {
    d.as_nanos() as i128
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    /// Packets of 10 ms really captured at 3000, 3010…, read `gap` ms after
    /// they ended, stamped `skew` ms off; returns the corrected starts.
    fn run(skew: i64, gaps: &[u64]) -> Vec<Duration> {
        let mut fix = StampFix::default();
        gaps.iter()
            .enumerate()
            .map(|(i, gap)| {
                let real = 3000 + i as u64 * 10;
                let stamped = Duration::from_millis((real as i64 + skew) as u64);
                fix.fix(stamped, ms(10), ms(real + 10 + gap))
            })
            .collect()
    }

    #[test]
    fn stamps_seconds_in_the_future_are_pulled_back() {
        let out = run(5000, &[7, 3, 9, 0, 4, 6]);
        assert_eq!(out[0], ms(3007), "first guess: ended when read");
        assert_eq!(out[3], ms(3030), "a packet read right away fixes the error");
        assert_eq!(out[5], ms(3050));
    }

    #[test]
    fn stamps_behind_the_clock_are_pushed_forward() {
        let out = run(-2000, &[1, 0, 2]);
        assert_eq!(out[2], ms(3020));
    }

    #[test]
    fn packets_read_together_stay_apart() {
        let mut fix = StampFix::default();
        let stamps: Vec<Duration> = (0..5).map(|i| ms(8000 + i * 10)).collect();
        for s in &stamps {
            fix.observe(*s, ms(10), ms(3050));
        }
        let starts: Vec<Duration> = stamps.iter().map(|s| fix.apply(*s)).collect();
        assert_eq!(starts, [ms(3000), ms(3010), ms(3020), ms(3030), ms(3040)]);
    }

    #[test]
    fn honest_stamps_move_at_most_by_the_smallest_read_gap() {
        let out = run(0, &[8, 2, 5, 2, 9]);
        assert_eq!(out[4], ms(3042));
        assert!(out.windows(2).all(|w| w[1] > w[0]));
    }
}
