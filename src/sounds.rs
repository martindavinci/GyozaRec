//! Short tones for recording events, made in code (no sound files).

use std::time::Duration;

/// Longest cue plus output latency: how long audio waits around a cue so
/// the cue never ends up in the recording.
pub const CUE_GUARD: Duration = Duration::from_millis(250);
/// Time for a pause to take effect in the recording loop before its cue.
pub const PAUSE_SETTLE: Duration = Duration::from_millis(100);

/// When a sound plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// A capture was saved (the system's own sound).
    Saved,
    Start,
    Pause,
    Resume,
    Stop,
}

const RATE: u32 = 44_100;

/// Notes of each cue (Hz) and the length of each note.
fn notes(cue: Cue) -> Option<(&'static [f32], Duration)> {
    let short = Duration::from_millis(75);
    match cue {
        Cue::Saved => None,
        Cue::Start => Some((&[660.0, 880.0], short)),
        Cue::Pause => Some((&[440.0], Duration::from_millis(120))),
        Cue::Resume => Some((&[660.0], Duration::from_millis(120))),
        Cue::Stop => Some((&[880.0, 660.0], short)),
    }
}

/// A 16-bit mono 44.1 kHz WAV file for a recording cue; `None` for `Saved`.
pub fn wav(cue: Cue) -> Option<Vec<u8>> {
    let (freqs, note) = notes(cue)?;
    let per_note = (note.as_secs_f64() * f64::from(RATE)) as usize;
    let fade = (0.005 * f64::from(RATE)) as usize;
    let mut pcm: Vec<i16> = Vec::with_capacity(per_note * freqs.len());
    for &freq in freqs {
        for i in 0..per_note {
            let envelope = (i.min(per_note - 1 - i) as f32 / fade as f32).min(1.0);
            let phase = std::f32::consts::TAU * freq * i as f32 / RATE as f32;
            pcm.push((phase.sin() * envelope * 0.3 * f32::from(i16::MAX)) as i16);
        }
    }
    let data = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_wavs_are_short_valid_wav() {
        for cue in [Cue::Start, Cue::Pause, Cue::Resume, Cue::Stop] {
            let bytes = wav(cue).unwrap();
            assert_eq!(&bytes[0..4], b"RIFF");
            assert_eq!(&bytes[8..16], b"WAVEfmt ");
            assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 44_100);
            let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
            assert_eq!(bytes.len(), 44 + data);
            let seconds = data as f64 / 2.0 / 44_100.0;
            assert!(seconds > 0.05 && seconds <= 0.18, "{cue:?}: {seconds} s");
        }
        assert!(wav(Cue::Saved).is_none());
    }

    #[test]
    fn cues_sound_different() {
        assert_ne!(wav(Cue::Start), wav(Cue::Stop));
        assert_ne!(wav(Cue::Pause), wav(Cue::Resume));
    }
}
