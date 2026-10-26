//! System audio for recordings: WASAPI loopback on the default output device.

use crate::audio::{AudioFeed, PcmChunk, StampFix};
use crate::capture::AudioStarter;
use crate::clock::Clock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, SyncSender};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::core::GUID;

/// How often the capture thread collects packets.
const POLL: Duration = Duration::from_millis(10);
/// Device buffer: generous, so a slow turn never loses audio.
const BUFFER_HNS: i64 = 10_000_000;
const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

/// What the device delivers; anything else is not recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleKind {
    Float32,
    Pcm16,
}

#[derive(Clone, Copy, Debug)]
struct Format {
    rate: u32,
    channels: u16,
    mask: u32,
    kind: SampleKind,
}

/// Captures what the computer plays (the default output device).
pub struct LoopbackStarter;

impl AudioStarter for LoopbackStarter {
    fn start(&self, clock: Arc<dyn Clock>) -> Option<AudioFeed> {
        let (chunks, feed) = mpsc::channel();
        let (ready, is_ready) = mpsc::sync_channel::<bool>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = std::thread::Builder::new()
            .name("gyozarec-audio".into())
            .spawn(move || {
                let failed = ready.clone();
                if capture(chunks, ready, &thread_stop, &*clock).is_err() {
                    let _ = failed.try_send(false);
                }
            })
            .ok()?;
        // Only a source that really started gets an audio track.
        if is_ready.recv_timeout(Duration::from_secs(3)) != Ok(true) {
            stop.store(true, Ordering::SeqCst);
            let _ = handle.join();
            return None;
        }
        let stopper = Box::new(move || {
            stop.store(true, Ordering::SeqCst);
            let _ = handle.join();
        });
        Some(AudioFeed::new(feed, stopper, false))
    }
}

/// Reads the shared-mode mix format, including the extensible form.
///
/// # Safety
/// `format` must point to a valid `WAVEFORMATEX` from `GetMixFormat`.
unsafe fn read_format(format: *const WAVEFORMATEX) -> Option<Format> {
    // SAFETY: the caller guarantees a valid pointer; fields are read unaligned.
    let base = unsafe { std::ptr::read_unaligned(format) };
    let (rate, channels, bits, tag) =
        (base.nSamplesPerSec, base.nChannels, base.wBitsPerSample, base.wFormatTag);
    let (kind, mask) = match tag {
        WAVE_FORMAT_IEEE_FLOAT if bits == 32 => (SampleKind::Float32, 0),
        WAVE_FORMAT_PCM if bits == 16 => (SampleKind::Pcm16, 0),
        WAVE_FORMAT_EXTENSIBLE => {
            // SAFETY: the tag says the structure is the extensible one.
            let ext = unsafe { std::ptr::read_unaligned(format.cast::<WAVEFORMATEXTENSIBLE>()) };
            let sub = ext.SubFormat;
            let mask = ext.dwChannelMask;
            if sub == SUBTYPE_FLOAT && bits == 32 {
                (SampleKind::Float32, mask)
            } else if sub == SUBTYPE_PCM && bits == 16 {
                (SampleKind::Pcm16, mask)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    (rate > 0 && channels > 0).then_some(Format { rate, channels, mask, kind })
}

/// The performance counter in 100 ns units, the scale WASAPI timestamps use.
fn qpc_hns() -> Option<u64> {
    let (mut count, mut freq) = (0i64, 0i64);
    // SAFETY: plain out-parameters.
    unsafe {
        QueryPerformanceCounter(&mut count).ok()?;
        QueryPerformanceFrequency(&mut freq).ok()?;
    }
    (freq > 0).then(|| (i128::from(count) * 10_000_000 / i128::from(freq)) as u64)
}

fn capture(
    chunks: Sender<PcmChunk>,
    ready: SyncSender<bool>,
    stop: &AtomicBool,
    clock: &dyn Clock,
) -> windows::core::Result<()> {
    // SAFETY: COM for this thread only, balanced at the end; every call checks
    // its HRESULT, the mix format is freed with CoTaskMemFree, and buffers are
    // released right after copying.
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let result = (|| -> windows::core::Result<()> {
            let devices: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            let mix = client.GetMixFormat()?;
            let format = read_format(mix);
            let init = format.map(|_| {
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    BUFFER_HNS,
                    0,
                    mix,
                    None,
                )
            });
            CoTaskMemFree(Some(mix as _));
            let (Some(format), Some(Ok(()))) = (format, init) else {
                let _ = ready.try_send(false);
                return Ok(());
            };
            let reader: IAudioCaptureClient = client.GetService()?;
            // Calibration: map the device's QPC timestamps onto the recording clock.
            let (Some(qpc_at), clock_at) = (qpc_hns(), clock.elapsed()) else {
                let _ = ready.try_send(false);
                return Ok(());
            };
            client.Start()?;
            let _ = ready.try_send(true);
            let to_clock = |qpc: u64| {
                let delta = Duration::from_nanos(qpc.abs_diff(qpc_at) * 100);
                if qpc >= qpc_at { clock_at + delta } else { clock_at.saturating_sub(delta) }
            };
            let mut last_end: Option<Duration> = None;
            // Some devices stamp packets seconds away from the real time.
            let mut fix = StampFix::default();
            let mut batch: Vec<(PcmChunk, Option<Duration>, Duration)> = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(POLL);
                while reader.GetNextPacketSize()? > 0 {
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                    let mut qpc = 0u64;
                    reader.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc))?;
                    let count = frames as usize * usize::from(format.channels);
                    let samples: Vec<f32> = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        vec![0.0; count]
                    } else {
                        match format.kind {
                            SampleKind::Float32 => {
                                std::slice::from_raw_parts(data.cast::<f32>(), count).to_vec()
                            }
                            SampleKind::Pcm16 => {
                                std::slice::from_raw_parts(data.cast::<i16>(), count)
                                    .iter()
                                    .map(|&s| f32::from(s) / 32_768.0)
                                    .collect()
                            }
                        }
                    };
                    reader.ReleaseBuffer(frames)?;
                    let read_at = clock.elapsed();
                    let length = Duration::from_secs_f64(frames as f64 / f64::from(format.rate));
                    // Device time, or `None` when the device could not tell.
                    let stamped = (flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 == 0)
                        .then(|| to_clock(qpc));
                    if let Some(stamped) = stamped {
                        fix.observe(stamped, length, read_at);
                    }
                    let chunk = PcmChunk {
                        source: 0,
                        rate: format.rate,
                        channels: format.channels,
                        channel_mask: format.mask,
                        samples,
                        at: read_at.saturating_sub(length),
                        discontinuity: flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0,
                    };
                    batch.push((chunk, stamped, length));
                }
                // Packets read together share what the whole group teaches
                // about the device clock, so they keep their spacing.
                for (mut chunk, stamped, length) in batch.drain(..) {
                    chunk.at = match stamped {
                        Some(stamped) => fix.apply(stamped),
                        None => last_end.unwrap_or(chunk.at),
                    };
                    last_end = Some(chunk.at + length);
                    if chunks.send(chunk).is_err() {
                        return Ok(()); // the recording is gone
                    }
                }
            }
            let _ = client.Stop();
            Ok(())
        })();
        if com {
            CoUninitialize();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints when loopback packets arrive and where the start tone lands
    /// on the recording clock. Plays the start tone once.
    #[test]
    #[ignore = "desktop-audio"]
    fn loopback_timestamps_follow_the_clock() {
        use crate::capture::SoundPlayer;
        let clock = Arc::new(crate::clock::SystemClock::new());
        let start = clock.elapsed();
        let mut feed = LoopbackStarter.start(clock.clone()).expect("output device");
        let ready = clock.elapsed();
        std::thread::sleep(Duration::from_millis(500));
        let played = clock.elapsed();
        super::super::SystemSound.play(crate::sounds::Cue::Start);
        std::thread::sleep(Duration::from_millis(1500));
        let stopped = clock.elapsed();
        feed.stop();
        let chunks: Vec<PcmChunk> = feed.chunks.try_iter().collect();
        eprintln!("start {start:?} ready {ready:?} played {played:?} stopped {stopped:?}");
        let mut frames = 0usize;
        for c in &chunks {
            let n = c.samples.len() / usize::from(c.channels);
            frames += n;
            let peak = c.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            if peak > 0.02 || c.discontinuity {
                eprintln!(
                    "at {:?} frames {n} rate {} ch {} mask {:#x} peak {peak:.3} disc {}",
                    c.at, c.rate, c.channels, c.channel_mask, c.discontinuity
                );
            }
        }
        let first = chunks.first().map(|c| c.at);
        let last = chunks.last().map(|c| c.at);
        eprintln!("chunks {} frames {frames} first {first:?} last {last:?}", chunks.len());
    }

    /// Starts and stops loopback; a quiet system may deliver no packets.
    /// Plays nothing.
    #[test]
    #[ignore = "desktop"]
    fn loopback_starts_and_stops() {
        let clock: Arc<dyn Clock> = Arc::new(crate::clock::SystemClock::new());
        let Some(mut feed) = LoopbackStarter.start(clock) else {
            eprintln!("no output device: nothing to test");
            return;
        };
        std::thread::sleep(Duration::from_millis(300));
        feed.stop();
        let chunks: Vec<PcmChunk> = feed.chunks.try_iter().collect();
        for c in &chunks {
            assert!(c.rate > 0 && c.channels > 0);
            assert_eq!(c.samples.len() % usize::from(c.channels), 0);
        }
    }
}
