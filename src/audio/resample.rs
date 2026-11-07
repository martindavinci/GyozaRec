// SPDX-License-Identifier: GPL-3.0-or-later
//! One source's audio to `OUT_RATE`, packet after packet as if it were one
//! long stream: no phase jump or lost frame between packets, a low-pass
//! before going down in rate so nothing above 22 kHz folds back as noise,
//! and a slight speed-up or slow-down so the track follows the source's
//! clock.

use super::OUT_RATE;

/// Highest frequency kept when the rate goes down.
const CUTOFF_HZ: f64 = 22_000.0;
/// Filter taps on each side, per unit of rate ratio: at 384 kHz, 128 a
/// side, so the filter goes from passing to stopping within about 8 kHz.
const TAPS_PER_RATIO: f64 = 16.0;
/// Largest speed correction: 0.5 %, under a tenth of a semitone.
pub const MAX_SPEED: f64 = 0.005;

pub struct Resampler {
    rate: u32,
    /// Symmetric low-pass, odd length; `[1.0]` when the rate does not go down.
    taps: Vec<f32>,
    /// Input frames still needed, the first `taps.len() / 2` as history.
    buf: Vec<[f32; 2]>,
    /// Where the next output frame falls in `buf`, in input frames.
    pos: f64,
    /// Relative speed correction: above 0 gives more output frames.
    speed: f64,
}

impl Resampler {
    pub fn new(rate: u32) -> Resampler {
        let taps = lowpass(rate);
        let half = taps.len() / 2;
        // Silence before the first frame, so it is filtered like the others.
        Resampler { rate, taps, buf: vec![[0.0; 2]; half], pos: half as f64, speed: 0.0 }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Seconds between the next output frame and the next input frame: the
    /// input already taken in that has not come out yet.
    pub fn pending(&self) -> f64 {
        (self.buf.len() as f64 - self.pos) / f64::from(self.rate)
    }

    /// Sets the speed correction; clamped, so pitch never moves audibly.
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(-MAX_SPEED, MAX_SPEED);
    }

    /// Stereo frames in, stereo frames at `OUT_RATE` out; the last few input
    /// frames wait for the next packet.
    pub fn process(&mut self, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
        self.buf.extend_from_slice(input);
        let half = self.taps.len() / 2;
        let step = f64::from(self.rate) / f64::from(OUT_RATE) / (1.0 + self.speed);
        let mut out = Vec::with_capacity((input.len() as f64 / step) as usize + 2);
        loop {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let needed = if frac > 0.0 { i + 1 } else { i };
            if needed + half >= self.buf.len() {
                break;
            }
            let a = self.filtered(i);
            let frame = if frac > 0.0 {
                let b = self.filtered(i + 1);
                [a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]
            } else {
                a
            };
            out.push(frame);
            self.pos += step;
        }
        let used = (self.pos as usize).saturating_sub(half).min(self.buf.len());
        self.buf.drain(..used);
        self.pos -= used as f64;
        out
    }

    /// The input at frame `i` of `buf`, low-passed.
    fn filtered(&self, i: usize) -> [f32; 2] {
        let half = self.taps.len() / 2;
        if half == 0 {
            return self.buf[i];
        }
        let window = &self.buf[i - half..=i + half];
        window
            .iter()
            .zip(&self.taps)
            .fold([0.0, 0.0], |[l, r], (f, t)| [l + f[0] * t, r + f[1] * t])
    }
}

/// Windowed-sinc (Blackman) low-pass at `CUTOFF_HZ` for input at `rate`,
/// or a single tap when the rate does not go down.
fn lowpass(rate: u32) -> Vec<f32> {
    let ratio = f64::from(rate) / f64::from(OUT_RATE);
    if ratio <= 1.0 {
        return vec![1.0];
    }
    let half = (TAPS_PER_RATIO * ratio).ceil() as usize;
    let len = 2 * half + 1;
    let fc = CUTOFF_HZ / f64::from(rate);
    let taps: Vec<f64> = (0..len)
        .map(|n| {
            let x = n as f64 - half as f64;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x)
            };
            let w = n as f64 / (len - 1) as f64 * std::f64::consts::TAU;
            sinc * (0.42 - 0.5 * w.cos() + 0.08 * (2.0 * w).cos())
        })
        .collect();
    let sum: f64 = taps.iter().sum();
    taps.iter().map(|t| (t / sum) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|i| {
                let v = (i as f64 * hz / f64::from(rate) * std::f64::consts::TAU).sin() as f32;
                [v, v]
            })
            .collect()
    }

    fn rms(frames: &[[f32; 2]]) -> f32 {
        (frames.iter().map(|f| f[0] * f[0]).sum::<f32>() / frames.len() as f32).sqrt()
    }

    /// Runs `input` through in packets of `packet` frames.
    fn run(r: &mut Resampler, input: &[[f32; 2]], packet: usize) -> Vec<[f32; 2]> {
        input.chunks(packet).flat_map(|p| r.process(p)).collect()
    }

    #[test]
    fn same_rate_passes_every_frame_through_unchanged() {
        let input = sine(48_000, 440.0, 4_800);
        let out = run(&mut Resampler::new(48_000), &input, 441);
        assert_eq!(out, input);
    }

    #[test]
    fn packets_join_without_a_step() {
        // A packet length that is not a multiple of the 8:1 ratio.
        let input = sine(384_000, 1_000.0, 384_000 / 5);
        let whole = run(&mut Resampler::new(384_000), &input, input.len());
        let split = run(&mut Resampler::new(384_000), &input, 3_843);
        assert_eq!(whole.len(), split.len());
        for (a, b) in whole.iter().zip(&split) {
            assert!((a[0] - b[0]).abs() < 1e-5, "{a:?} {b:?}");
        }
        // Largest jump between neighbours: a 1 kHz sine at 48 kHz moves at
        // most 2π·1000/48000 ≈ 0.13 per frame.
        let jump = split.windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0f32, f32::max);
        assert!(jump < 0.14, "a step of {jump}");
    }

    #[test]
    fn going_down_from_384_khz_keeps_tones_and_drops_ultrasound() {
        let len = 384_000 / 10;
        let kept = run(&mut Resampler::new(384_000), &sine(384_000, 1_000.0, len), 3_840);
        let kept = rms(&kept[200..]);
        assert!((kept - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02, "1 kHz at {kept}");
        // 40 kHz cannot be in a 48 kHz track; without a filter it would come
        // back as an 8 kHz whistle.
        let folded = run(&mut Resampler::new(384_000), &sine(384_000, 40_000.0, len), 3_840);
        let folded = rms(&folded[200..]);
        assert!(folded < 0.01, "40 kHz folds back at {folded}");
    }

    #[test]
    fn speed_gives_more_or_fewer_frames() {
        let input = sine(48_000, 440.0, 48_000);
        let mut fast = Resampler::new(48_000);
        fast.set_speed(0.002);
        let more = run(&mut fast, &input, 480).len() as i64;
        assert!((more - 48_096).abs() <= 2, "{more}");
        let mut slow = Resampler::new(48_000);
        slow.set_speed(-1.0);
        let fewer = run(&mut slow, &input, 480).len() as i64;
        assert!((fewer - 47_760).abs() <= 2, "clamped to 0.5 %: {fewer}");
    }

    #[test]
    fn pending_counts_the_input_not_out_yet() {
        let mut r = Resampler::new(384_000);
        assert_eq!(r.pending(), 0.0);
        let out = r.process(&sine(384_000, 1_000.0, 3_840));
        let came_out = out.len() as f64 / 48_000.0;
        assert!((came_out + r.pending() - 0.01).abs() < 1e-9, "{came_out} + {}", r.pending());
    }
}
