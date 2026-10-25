//! Any speaker layout to stereo.

/// Left/right weights per speaker bit (`SPEAKER_FRONT_LEFT` = bit 0, …):
/// centre and surrounds at -3 dB, LFE dropped.
fn weights(bit: u32) -> (f32, f32) {
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    match bit {
        0 | 6 => (1.0, 0.0), // front left, front left of centre
        1 | 7 => (0.0, 1.0), // front right, front right of centre
        2 => (H, H),         // front centre
        3 => (0.0, 0.0),     // low frequency
        4 | 9 => (H, 0.0),   // back left, side left
        5 | 10 => (0.0, H),  // back right, side right
        _ => (0.5, 0.5),     // back centre and top speakers
    }
}

/// Speaker layout Windows assumes when a format gives no mask.
fn default_mask(channels: u16) -> u32 {
    match channels {
        1 => 0x4,
        2 => 0x3,
        3 => 0x7,
        4 => 0x33,
        5 => 0x37,
        6 => 0x3F,
        7 => 0x13F,
        _ => 0x63F,
    }
}

/// Interleaved `channels`-channel samples to interleaved stereo.
pub fn to_stereo(samples: &[f32], channels: u16, mask: u32) -> Vec<f32> {
    let n = usize::from(channels.max(1));
    if n == 1 {
        return samples.iter().flat_map(|&s| [s, s]).collect();
    }
    let mask = if mask == 0 { default_mask(channels) } else { mask };
    let bits: Vec<u32> = (0..32).filter(|b| mask & (1 << b) != 0).collect();
    let w: Vec<(f32, f32)> =
        (0..n).map(|i| bits.get(i).map_or((0.5, 0.5), |b| weights(*b))).collect();
    samples
        .chunks_exact(n)
        .flat_map(|frame| {
            let (l, r) = frame
                .iter()
                .zip(&w)
                .fold((0.0, 0.0), |(l, r), (s, (wl, wr))| (l + s * wl, r + s * wr));
            [l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_duplicated_and_stereo_kept() {
        assert_eq!(to_stereo(&[0.5, -0.25], 1, 0), vec![0.5, 0.5, -0.25, -0.25]);
        assert_eq!(to_stereo(&[0.1, 0.2, 0.3, 0.4], 2, 0), vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn downmix_51_keeps_centre_dialogue() {
        // FL FR FC LFE BL BR, only the centre speaks.
        let frame = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
        let st = to_stereo(&frame, 6, 0x3F);
        assert!((st[0] - 0.707).abs() < 0.01 && (st[1] - 0.707).abs() < 0.01, "{st:?}");
        // Surround left goes to the left side only.
        let st = to_stereo(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0], 6, 0x3F);
        assert!(st[0] > 0.5 && st[1].abs() < 1e-6, "{st:?}");
    }

    #[test]
    fn loud_downmix_is_clamped() {
        let st = to_stereo(&[1.0, 1.0, 1.0, 0.0, 1.0, 1.0], 6, 0x3F);
        assert!(st.iter().all(|v| v.abs() <= 1.0), "{st:?}");
    }
}
