//! Reads the frames of a video file with Media Foundation's source reader.

use super::mp4::MfSession;
use crate::convert::{VideoFrames, VideoReader};
use crate::geometry::{PixelRect, PixelSize};
use crate::pixels::PixelBuffer;
use std::path::Path;
use std::time::Duration;
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFMediaBuffer, IMFSample, IMFSourceReader, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Video, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToUInt64;
use windows::core::{HSTRING, Interface};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
/// Ticks (100 ns) per second.
const HNS: u64 = 10_000_000;

/// Decodes any video Windows can play into 32-bit frames.
pub struct MfVideoReader;

impl VideoReader for MfVideoReader {
    fn open(&self, path: &Path) -> crate::Result<Box<dyn VideoFrames>> {
        let session = MfSession::start()?;
        // SAFETY: COM objects created and used on this thread only, inside the
        // Media Foundation session that `MfVideo` keeps alive.
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 1)?;
            let attributes =
                attributes.ok_or_else(|| crate::Error::Message("no attributes".into()))?;
            attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
            let reader =
                MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes)?;
            reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
            reader.SetStreamSelection(VIDEO, true)?;
            let wanted = MFCreateMediaType()?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(VIDEO, None, &wanted)?;
            let duration = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|v| PropVariantToUInt64(&v).ok())
                .map(|hns| Duration::from_nanos(hns.saturating_mul(100)));
            let (size, stride) = current_format(&reader)?;
            Ok(Box::new(MfVideo { reader, size, stride, duration, _session: session }))
        }
    }
}

struct MfVideo {
    reader: IMFSourceReader,
    size: PixelSize,
    /// Bytes per row of a contiguous buffer (negative: bottom-up).
    stride: i32,
    duration: Option<Duration>,
    // Dropped last: the reader must go before Media Foundation shuts down.
    _session: MfSession,
}

/// Frame size and default stride of the reader's current output type.
///
/// # Safety
/// `reader` must be a live source reader on this thread.
unsafe fn current_format(reader: &IMFSourceReader) -> crate::Result<(PixelSize, i32)> {
    // SAFETY: per the function contract.
    unsafe {
        let media_type = reader.GetCurrentMediaType(VIDEO)?;
        let packed = media_type.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let size =
            PixelSize { width: (packed >> 32) as i32, height: (packed & 0xFFFF_FFFF) as i32 };
        let stride =
            media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(size.width * 4, |s| s as i32);
        Ok((size, stride))
    }
}

impl VideoFrames for MfVideo {
    fn duration(&self) -> Option<Duration> {
        self.duration
    }

    fn size(&self) -> PixelSize {
        self.size
    }

    fn next_frame(&mut self) -> Option<crate::Result<(PixelBuffer, Duration)>> {
        loop {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None::<IMFSample>);
            // SAFETY: out-parameters live across the call; same-thread reader.
            let read = unsafe {
                self.reader.ReadSample(
                    VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut time),
                    Some(&mut sample),
                )
            };
            if let Err(err) = read {
                return Some(Err(err.into()));
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return None;
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                // SAFETY: same-thread reader.
                match unsafe { current_format(&self.reader) } {
                    Ok((size, stride)) => (self.size, self.stride) = (size, stride),
                    Err(err) => return Some(Err(err)),
                }
            }
            // A stream tick carries no picture: read on.
            let Some(sample) = sample else { continue };
            let at = Duration::from_nanos(u64::try_from(time).unwrap_or(0) * (1_000_000_000 / HNS));
            return Some(self.picture(&sample).map(|img| (img, at)));
        }
    }
}

impl MfVideo {
    fn picture(&self, sample: &IMFSample) -> crate::Result<PixelBuffer> {
        let PixelSize { width, height } = self.size;
        // SAFETY: the buffer stays locked while its bytes are copied, and every
        // access is bounds-checked against the locked length.
        unsafe {
            let buffer: IMFMediaBuffer = sample.ConvertToContiguousBuffer()?;
            if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
                let (mut first, mut pitch) = (std::ptr::null_mut(), 0i32);
                two_d.Lock2D(&mut first, &mut pitch)?;
                // `first` is the top row as shown; rows follow at `pitch`.
                let rows = (0..height as isize)
                    .map(|y| {
                        std::slice::from_raw_parts(
                            first.offset(y * pitch as isize),
                            width as usize * 4,
                        )
                    })
                    .collect::<Vec<_>>();
                let bgra = rows_to_bgra(&rows);
                let _ = two_d.Unlock2D();
                return Ok(PixelBuffer::new(PixelRect::new(0, 0, width, height), bgra));
            }
            let (mut data, mut length) = (std::ptr::null_mut(), 0u32);
            buffer.Lock(&mut data, None, Some(&mut length))?;
            let bytes = std::slice::from_raw_parts(data, length as usize);
            let bgra =
                contiguous_to_bgra(bytes, width as usize, height as usize, self.stride as isize);
            let _ = buffer.Unlock();
            bgra.map(|b| PixelBuffer::new(PixelRect::new(0, 0, width, height), b))
                .ok_or_else(|| crate::UserError::VideoUnreadable.into())
        }
    }
}

/// Rows (top first) of BGRX pixels into BGRA with opaque alpha.
fn rows_to_bgra(rows: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rows.iter().map(|r| r.len()).sum());
    for row in rows {
        for px in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    out
}

/// A contiguous buffer with `stride` bytes per row (negative: the last row
/// comes first); `None` when the buffer is too short.
fn contiguous_to_bgra(bytes: &[u8], width: usize, height: usize, stride: isize) -> Option<Vec<u8>> {
    let pitch = stride.unsigned_abs();
    if pitch < width * 4 || bytes.len() < pitch * height.saturating_sub(1) + width * 4 {
        return None;
    }
    let rows: Vec<&[u8]> = (0..height)
        .map(|y| {
            let row = if stride < 0 { height - 1 - y } else { y };
            &bytes[row * pitch..row * pitch + width * 4]
        })
        .collect();
    Some(rows_to_bgra(&rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_up_rows_are_flipped() {
        // Two rows of 2 BGRX pixels; row 0 in memory is the bottom row.
        let bytes = [1, 1, 1, 0, 2, 2, 2, 0, 9, 9, 9, 0, 8, 8, 8, 0];
        let out = contiguous_to_bgra(&bytes, 2, 2, -8).unwrap();
        assert_eq!(out, [9, 9, 9, 255, 8, 8, 8, 255, 1, 1, 1, 255, 2, 2, 2, 255]);
    }

    #[test]
    fn padded_rows_skip_the_padding() {
        // stride 12 for 2 pixels: 4 bytes of padding per row.
        let bytes = [1, 1, 1, 0, 2, 2, 2, 0, 7, 7, 7, 7, 3, 3, 3, 0, 4, 4, 4, 0];
        let out = contiguous_to_bgra(&bytes, 2, 2, 12).unwrap();
        assert_eq!(out, [1, 1, 1, 255, 2, 2, 2, 255, 3, 3, 3, 255, 4, 4, 4, 255]);
    }

    #[test]
    fn short_buffer_is_refused() {
        assert!(contiguous_to_bgra(&[0; 12], 2, 2, 8).is_none());
        assert!(contiguous_to_bgra(&[0; 16], 2, 2, 4).is_none(), "stride below the row size");
    }

    // CI runners (Windows Server) usually lack Media Foundation.
    #[test]
    #[ignore = "media-foundation"]
    fn reads_back_a_written_mp4() {
        use crate::recording::FrameSink;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.mp4");
        let size = PixelSize { width: 320, height: 240 };
        let mut s = super::super::Mp4Sink::new(&p, size, 30, false).unwrap();
        for i in 0..30u64 {
            let frame = PixelBuffer::solid(PixelRect::new(0, 0, 320, 240), 20, 20, 200 + i as u8);
            s.push(&frame, Duration::from_millis(i * 33)).unwrap();
        }
        s.finish().unwrap();
        drop(s);
        let mut video = MfVideoReader.open(&p).unwrap();
        assert_eq!(video.size(), size);
        assert!(video.duration().is_some_and(|d| d >= Duration::from_millis(900)));
        let mut times = vec![];
        let mut first = None;
        while let Some(frame) = video.next_frame() {
            let (img, t) = frame.unwrap();
            first.get_or_insert(img);
            times.push(t);
        }
        assert!(times.len() >= 25, "{} frames", times.len());
        assert!(times.windows(2).all(|w| w[0] < w[1]));
        let px = first.unwrap().pixel(crate::geometry::PixelPoint { x: 160, y: 120 });
        assert!(px[2] > 150 && px[0] < 80, "mostly red: {px:?}");
    }

    #[test]
    #[ignore = "media-foundation"]
    fn converts_a_written_mp4_to_gif() {
        use crate::convert::{ConvertEnd, ConvertJob, convert_to_gif};
        use crate::recording::FrameSink;
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("clip.mp4");
        let mut s =
            super::super::Mp4Sink::new(&source, PixelSize { width: 640, height: 360 }, 30, false)
                .unwrap();
        for i in 0..90u64 {
            let frame = PixelBuffer::solid(PixelRect::new(0, 0, 640, 360), (i * 2) as u8, 90, 30);
            s.push(&frame, Duration::from_millis(i * 33)).unwrap();
        }
        s.finish().unwrap();
        drop(s);
        let job = ConvertJob {
            source,
            target: d.path().join("clip.gif"),
            fps: 10,
            max_long_side: 320,
            max_duration: Duration::from_secs(15),
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let end = convert_to_gif(&MfVideoReader, &job, &cancel, &mut |_| {}).unwrap();
        assert_eq!(end, ConvertEnd::Done);
        let mut dec =
            gif::DecodeOptions::new().read_info(std::fs::File::open(&job.target).unwrap()).unwrap();
        assert_eq!((dec.width(), dec.height()), (320, 180));
        let mut total = 0u32;
        while let Some(f) = dec.read_next_frame().unwrap() {
            total += u32::from(f.delay);
        }
        assert!((280..=320).contains(&total), "{total} centiseconds for 3 s");
    }
}
