//! Places audio packets by their capture time and turns them into one
//! contiguous 48 kHz stereo track on the video's active timeline.

use super::{OUT_RATE, PcmChunk};
use std::time::Duration;

/// Paused stretches of a recording, on the recording clock.
#[derive(Clone, Debug)]
pub struct PauseHistory {
    start: Duration,
    done: Vec<(Duration, Duration)>,
    current: Option<Duration>,
}

impl PauseHistory {
    pub fn new(start: Duration) -> PauseHistory {
        PauseHistory { start, done: Vec::new(), current: None }
    }

    /// A pause never starts before the start or the last resume.
    pub fn pause(&mut self, now: Duration) {
        let floor = self.done.last().map_or(self.start, |&(_, end)| end);
        self.current.get_or_insert(now.max(floor));
    }

    pub fn resume(&mut self, now: Duration) {
        if let Some(since) = self.current.take() {
            self.done.push((since, now.max(since)));
        }
    }

    pub fn is_paused(&self) -> bool {
        self.current.is_some()
    }

    /// Recorded (non-paused) time from the start up to `now`.
    pub fn active_elapsed(&self, now: Duration) -> Duration {
        let overlap = |s: Duration, e: Duration| e.min(now).saturating_sub(s.max(self.start));
        let paused: Duration = self.done.iter().map(|&(s, e)| overlap(s, e)).sum::<Duration>()
            + self.current.map_or(Duration::ZERO, |s| overlap(s, now.max(s)));
        now.saturating_sub(self.start).saturating_sub(paused)
    }

    /// Where `t` lands on the recorded timeline; `None` inside a pause or
    /// before the start.
    pub fn map(&self, t: Duration) -> Option<Duration> {
        let inside = |&(s, e): &(Duration, Duration)| s <= t && t < e;
        if t < self.start || self.done.iter().any(inside) || self.current.is_some_and(|s| t >= s) {
            return None;
        }
        let before: Duration =
            self.done.iter().filter(|(_, e)| *e <= t).map(|(s, e)| *e - *s).sum();
        Some(t - self.start - before)
    }
}

/// A slice of the audio track: interleaved stereo i16, starting at frame
/// `start_frame` of the recording's active time.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioOut {
    pub samples: Vec<i16>,
    pub start_frame: u64,
}

pub struct Timeline {
    latency: Duration,
    committed: u64,
    placed: Vec<Placed>,
}

struct Placed {
    source: u8,
    at: Duration,
    /// Stereo frames at `OUT_RATE`.
    frames: Vec<[f32; 2]>,
}

impl Timeline {
    pub fn new(latency: Duration) -> Timeline {
        Timeline { latency, committed: 0, placed: Vec::new() }
    }

    pub fn push(&mut self, chunk: PcmChunk) {
        let stereo = super::to_stereo(&chunk.samples, chunk.channels, chunk.channel_mask);
        let frames = resample(&stereo, chunk.rate);
        if !frames.is_empty() {
            self.placed.push(Placed { source: chunk.source, at: chunk.at, frames });
        }
    }

    /// Commits audio up to `now - latency` (or up to `now` when `final_`):
    /// each packet's frames land at their capture time mapped through the
    /// pauses, the first packet of a source to claim a frame keeps it, holes
    /// stay silent, and frames before the committed point are dropped.
    pub fn drain(
        &mut self,
        now: Duration,
        pauses: &PauseHistory,
        final_: bool,
    ) -> Option<AudioOut> {
        let horizon = if final_ { now } else { now.saturating_sub(self.latency) };
        let target = to_frame_floor(pauses.active_elapsed(horizon));
        if target <= self.committed {
            // Paused: nothing to commit, but what came in meanwhile goes.
            self.prune(horizon);
            return None;
        }
        let n = (target - self.committed) as usize;
        let mut sources: Vec<(u8, Vec<Option<[f32; 2]>>)> = Vec::new();
        for p in &self.placed {
            let slot = match sources.iter().position(|(s, _)| *s == p.source) {
                Some(i) => i,
                None => {
                    sources.push((p.source, vec![None; n]));
                    sources.len() - 1
                }
            };
            let buffer = &mut sources[slot].1;
            for (k, frame) in p.frames.iter().enumerate() {
                let Some(active) = pauses.map(p.at + frame_time(k as u64)) else { continue };
                let index = to_frame_round(active);
                if index >= self.committed && index < target {
                    let cell = &mut buffer[(index - self.committed) as usize];
                    cell.get_or_insert(*frame);
                }
            }
        }
        let mut samples = Vec::with_capacity(n * 2);
        for i in 0..n {
            let (l, r) = sources.iter().fold((0.0f32, 0.0f32), |(l, r), (_, b)| {
                b[i].map_or((l, r), |[fl, fr]| (l + fl, r + fr))
            });
            samples.push(to_i16(l));
            samples.push(to_i16(r));
        }
        self.prune(horizon);
        let start_frame = self.committed;
        self.committed = target;
        Some(AudioOut { samples, start_frame })
    }

    /// Packets that ended before the horizon are fully committed or dropped.
    fn prune(&mut self, horizon: Duration) {
        self.placed.retain(|p| p.at + frame_time(p.frames.len() as u64) > horizon);
    }
}

fn frame_time(frame: u64) -> Duration {
    Duration::from_nanos(frame * 1_000_000_000 / u64::from(OUT_RATE))
}

fn to_frame_floor(d: Duration) -> u64 {
    (d.as_nanos() * u128::from(OUT_RATE) / 1_000_000_000) as u64
}

fn to_frame_round(d: Duration) -> u64 {
    ((d.as_nanos() * u128::from(OUT_RATE) + 500_000_000) / 1_000_000_000) as u64
}

fn to_i16(v: f32) -> i16 {
    (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

/// Interleaved stereo at `rate` to stereo frames at `OUT_RATE` (linear).
fn resample(stereo: &[f32], rate: u32) -> Vec<[f32; 2]> {
    let input: Vec<[f32; 2]> = stereo.as_chunks::<2>().0.to_vec();
    if rate == OUT_RATE || input.len() < 2 || rate == 0 {
        return input;
    }
    let out_len = (input.len() as u64 * u64::from(OUT_RATE) / u64::from(rate)) as usize;
    let step = f64::from(rate) / f64::from(OUT_RATE);
    (0..out_len)
        .map(|j| {
            let pos = j as f64 * step;
            let i = (pos as usize).min(input.len() - 2);
            let frac = (pos - i as f64).min(1.0) as f32;
            let (a, b) = (input[i], input[i + 1]);
            [a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    /// A 48 kHz mono chunk of `len_ms` filled with `value`.
    fn chunk(at_ms: u64, len_ms: u64, value: f32) -> PcmChunk {
        PcmChunk {
            source: 0,
            rate: OUT_RATE,
            channels: 1,
            channel_mask: 0,
            samples: vec![value; (len_ms * 48) as usize],
            at: ms(at_ms),
            discontinuity: false,
        }
    }

    /// A recording left paused while sound plays must not pile up packets.
    #[test]
    fn packets_inside_a_long_pause_are_not_kept() {
        let mut t = Timeline::new(ms(300));
        let mut p = PauseHistory::new(ms(0));
        t.push(chunk(0, 100, 0.5));
        assert!(t.drain(ms(700), &p, false).is_some(), "commits up to 400 ms");
        p.pause(ms(400));
        for i in 0..100 {
            t.push(chunk(400 + i * 10, 10, 0.5));
        }
        assert!(t.drain(ms(2000), &p, false).is_none());
        assert!(t.placed.is_empty(), "{} packets kept", t.placed.len());
    }

    #[test]
    fn a_pause_never_starts_before_the_last_resume() {
        let mut p = PauseHistory::new(ms(0));
        p.pause(ms(100));
        p.resume(ms(300));
        p.pause(ms(250));
        assert_eq!(p.active_elapsed(ms(400)), ms(100));
        assert_eq!(p.map(ms(299)), None);
    }

    fn frames(out: &AudioOut) -> usize {
        out.samples.len() / 2
    }

    #[test]
    fn pause_history_maps_and_skips_pauses() {
        let mut p = PauseHistory::new(ms(1000));
        p.pause(ms(2000));
        p.resume(ms(3000));
        assert_eq!(p.map(ms(500)), None, "before the start");
        assert_eq!(p.map(ms(1500)), Some(ms(500)));
        assert_eq!(p.map(ms(2500)), None, "inside the pause");
        assert_eq!(p.map(ms(3500)), Some(ms(1500)));
        assert_eq!(p.active_elapsed(ms(4000)), ms(2000));
        p.pause(ms(4000));
        assert!(p.is_paused());
        assert_eq!(p.map(ms(4500)), None, "inside the current pause");
        assert_eq!(p.active_elapsed(ms(9000)), ms(2000));
    }

    #[test]
    fn resample_44100_mono_length() {
        let mut t = Timeline::new(Duration::ZERO);
        let mut c = chunk(0, 0, 0.0);
        c.rate = 44_100;
        c.samples = vec![0.25; 4_410];
        t.push(c);
        let out = t.drain(ms(100), &PauseHistory::new(Duration::ZERO), true).unwrap();
        assert!((frames(&out) as i64 - 4_800).abs() <= 1, "{}", frames(&out));
    }

    #[test]
    fn holes_become_silence_and_output_is_contiguous() {
        let mut t = Timeline::new(Duration::ZERO);
        t.push(chunk(0, 100, 0.5));
        t.push(chunk(300, 100, 0.5));
        let out = t.drain(ms(400), &PauseHistory::new(Duration::ZERO), true).unwrap();
        assert_eq!((out.start_frame, frames(&out)), (0, 19_200));
        let at = |frame: usize| out.samples[frame * 2];
        // Frames 0..4 800 and 14 400..19 200 carry audio; the rest is the hole.
        assert!(at(100) > 10_000 && at(4_000) > 10_000);
        assert_eq!((at(4_800), at(9_000)), (0, 0), "the hole between 100 and 300 ms is silent");
        assert_eq!(at(15_000), 16_384);
        assert!(t.drain(ms(400), &PauseHistory::new(Duration::ZERO), true).is_none());
    }

    #[test]
    fn markers_stay_in_sync_across_pauses_and_delays() {
        let mut pauses = PauseHistory::new(Duration::ZERO);
        let pause_spans = [(2_000, 3_000), (5_000, 6_500), (7_000, 7_200)];
        let clicks_ms = [1_000u64, 4_000, 8_000];
        // 10 ms packets carrying 2% more frames than nominal (device clock skew),
        // with a click 100 frames into the packet that starts at each click time.
        let mut packets = Vec::new();
        for i in 0..1_000u64 {
            let at = i * 10;
            let mut c = chunk(at, 0, 0.0);
            c.samples = vec![0.01; 490];
            if clicks_ms.contains(&at) {
                c.samples[100] = 1.0;
            }
            // Delivered up to 250 ms late, in a scrambled order.
            let delay = (i * 37) % 250;
            packets.push((at + delay, c));
        }
        packets.sort_by_key(|(arrival, _)| *arrival);
        let mut t = Timeline::new(ms(300));
        let mut out = Vec::new();
        let mut next = packets.into_iter().peekable();
        for now in (0..=10_000u64).step_by(50) {
            for (start, end) in pause_spans {
                if now == start {
                    pauses.pause(ms(now));
                }
                if now == end {
                    pauses.resume(ms(now));
                }
            }
            while next.peek().is_some_and(|(arrival, _)| *arrival <= now) {
                t.push(next.next().unwrap().1);
            }
            if let Some(o) = t.drain(ms(now), &pauses, false) {
                assert_eq!(o.start_frame as usize, out.len() / 2, "contiguous");
                out.extend(o.samples);
            }
        }
        for (_, c) in next {
            t.push(c);
        }
        if let Some(o) = t.drain(ms(10_000), &pauses, true) {
            out.extend(o.samples);
        }
        let found: Vec<f64> = out
            .chunks(2)
            .enumerate()
            .filter(|(_, f)| f[0] > 30_000)
            .map(|(i, _)| i as f64 / 48.0)
            .collect();
        // Expected active times: the click plus 100 frames, minus earlier pauses.
        let expected = [1_000.0, 4_000.0 - 1_000.0, 8_000.0 - 1_000.0 - 1_500.0 - 200.0];
        assert_eq!(found.len(), 3, "{found:?}");
        for (got, want) in found.iter().zip(expected) {
            let want = want + 100.0 / 48.0;
            assert!((got - want).abs() <= 20.0, "click at {got} ms, expected {want} ms");
        }
    }

    #[test]
    fn late_chunks_beyond_latency_are_dropped() {
        let mut t = Timeline::new(ms(300));
        let pauses = PauseHistory::new(Duration::ZERO);
        let first = t.drain(ms(1_000), &pauses, false).unwrap();
        assert_eq!(frames(&first), 33_600, "silence up to 700 ms");
        t.push(chunk(200, 100, 0.5));
        let second = t.drain(ms(1_100), &pauses, false).unwrap();
        assert_eq!(second.start_frame, 33_600);
        assert!(second.samples.iter().all(|s| *s == 0), "the late packet is dropped");
    }

    #[test]
    fn stop_while_paused_drains_cleanly() {
        let mut t = Timeline::new(ms(300));
        let mut pauses = PauseHistory::new(Duration::ZERO);
        t.push(chunk(0, 2_000, 0.5));
        pauses.pause(ms(1_000));
        let out = t.drain(ms(2_000), &pauses, true).unwrap();
        assert_eq!(frames(&out), 48_000, "only the recorded second");
    }

    #[test]
    fn source_loss_continues_with_silence() {
        let mut t = Timeline::new(Duration::ZERO);
        t.push(chunk(0, 500, 0.5));
        let out = t.drain(ms(1_000), &PauseHistory::new(Duration::ZERO), true).unwrap();
        assert_eq!(frames(&out), 48_000);
        assert_eq!(out.samples[2 * 47_000], 0);
    }
}
