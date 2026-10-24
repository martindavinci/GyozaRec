//! H.264 MP4 writer on Media Foundation's sink writer.

use crate::geometry::PixelSize;
use crate::pixels::PixelBuffer;
use crate::recording::{FrameSink, bitrate};
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::Win32::Media::MediaFoundation::{
    IMFMediaType, IMFSinkWriter, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_VERSION, MFCreateAttributes, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFCreateSourceReaderFromURL, MFMediaType_Video, MFSTARTUP_FULL,
    MFShutdown, MFStartup, MFVideoFormat_H264, MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::HSTRING;

/// 100-nanosecond units per second (Media Foundation time base).
const HNS_PER_SECOND: i64 = 10_000_000;

/// COM + Media Foundation started on the current thread; undone on drop.
struct MfSession {
    com: bool,
}

impl MfSession {
    fn start() -> crate::Result<MfSession> {
        // SAFETY: plain COM/MF initialization for this thread, balanced in Drop.
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Err(err) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                if com {
                    CoUninitialize();
                }
                return Err(err.into());
            }
            Ok(MfSession { com })
        }
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        // SAFETY: balances MfSession::start on the same thread.
        unsafe {
            let _ = MFShutdown();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

struct Writer {
    writer: Option<IMFSinkWriter>,
    stream: u32,
    // Declared last: dropped after the writer is released.
    _session: MfSession,
}

// SAFETY: the writer is created, used and dropped on the recording thread only;
// the sink merely moves there before first use.
unsafe impl Send for Writer {}

fn video_type(
    size: PixelSize,
    fps: u32,
    subtype: &windows::core::GUID,
) -> crate::Result<IMFMediaType> {
    let frame_size = (u64::from(size.width as u32) << 32) | u64::from(size.height as u32);
    // SAFETY: setting attributes on a freshly created media type.
    unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        t.SetUINT64(&MF_MT_FRAME_SIZE, frame_size)?;
        t.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(fps) << 32) | 1)?;
        t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
        Ok(t)
    }
}

impl Writer {
    fn open(path: &Path, size: PixelSize, fps: u32) -> crate::Result<Writer> {
        let session = MfSession::start()?;
        // SAFETY: standard sink-writer setup; every call checks its HRESULT.
        unsafe {
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 1)?;
            let attrs = attrs.ok_or_else(|| {
                crate::Error::Message("Media Foundation attributes unavailable".into())
            })?;
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
            let url = HSTRING::from(path.as_os_str());
            let writer = MFCreateSinkWriterFromURL(&url, None, &attrs)?;

            let output = video_type(size, fps, &MFVideoFormat_H264)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate(size, fps))?;
            let stream = writer.AddStream(&output)?;

            let input = video_type(size, fps, &MFVideoFormat_RGB32)?;
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, size.width as u32 * 4)?; // top-down
            writer.SetInputMediaType(stream, &input, None)?;
            writer.BeginWriting()?;
            Ok(Writer { writer: Some(writer), stream, _session: session })
        }
    }

    fn write(&self, frame: &PixelBuffer, t: Duration, fps: u32) -> crate::Result<()> {
        let writer = self
            .writer
            .as_ref()
            .ok_or_else(|| crate::Error::Message("MP4 writer already closed".into()))?;
        let len = frame.bgra.len() as u32;
        // SAFETY: the locked buffer holds `len` bytes; it is unlocked before use.
        unsafe {
            let buffer = MFCreateMemoryBuffer(len)?;
            let mut data = std::ptr::null_mut();
            buffer.Lock(&mut data, None, None)?;
            std::ptr::copy_nonoverlapping(frame.bgra.as_ptr(), data, frame.bgra.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(len)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime((t.as_nanos() / 100) as i64)?;
            sample.SetSampleDuration(HNS_PER_SECOND / i64::from(fps.max(1)))?;
            writer.WriteSample(self.stream, &sample)?;
        }
        Ok(())
    }

    fn finalize(&mut self) -> crate::Result<()> {
        if let Some(writer) = self.writer.take() {
            // SAFETY: finalizing a writer that began writing.
            unsafe { writer.Finalize()? };
        }
        Ok(())
    }
}

/// Media Foundation starts lazily on the first frame, on the thread that
/// pushes frames (the sink is built on another thread).
pub struct Mp4Sink {
    path: PathBuf,
    size: PixelSize,
    fps: u32,
    writer: Option<Writer>,
}

impl Mp4Sink {
    pub fn new(path: &Path, size: PixelSize, fps: u32) -> crate::Result<Mp4Sink> {
        if size.width < 2 || size.height < 2 || size.width % 2 != 0 || size.height % 2 != 0 {
            return Err(crate::UserError::InvalidVideoSize {
                width: size.width,
                height: size.height,
            }
            .into());
        }
        Ok(Mp4Sink { path: path.to_path_buf(), size, fps, writer: None })
    }
}

impl FrameSink for Mp4Sink {
    fn push(&mut self, frame: &PixelBuffer, t: Duration) -> crate::Result<()> {
        if self.writer.is_none() {
            self.writer = Some(Writer::open(&self.path, self.size, self.fps)?);
        }
        let writer = self.writer.as_ref().expect("opened above");
        if frame.bounds.size() == self.size {
            writer.write(frame, t, self.fps)
        } else {
            writer.write(&frame.resized(self.size), t, self.fps)
        }
    }

    fn finish(&mut self) -> crate::Result<()> {
        match self.writer.as_mut() {
            Some(writer) => writer.finalize(),
            None => Err(crate::Error::from(crate::UserError::NoFrames)),
        }
    }
}

/// First video frame of an MP4 file, decoded to BGRA (used to verify output).
#[cfg(test)]
pub fn decode_first_frame(path: &Path) -> crate::Result<PixelBuffer> {
    use windows::Win32::Media::MediaFoundation::{
        IMF2DBuffer, IMFSample, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    };
    use windows::core::Interface;
    let size = probe_mp4(path)?;
    let _session = MfSession::start()?;
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    // SAFETY: source reader calls with checked HRESULTs; the 2D buffer is unlocked
    // before release, and the copy stays within `pitch × height`.
    unsafe {
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.ok_or_else(|| {
            crate::Error::Message("Media Foundation attributes unavailable".into())
        })?;
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attrs)?;
        let wanted = MFCreateMediaType()?;
        wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(stream, None, &wanted)?;
        let mut sample: Option<IMFSample> = None;
        let mut flags = 0u32;
        reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))?;
        let sample = sample.ok_or_else(|| crate::Error::from(crate::UserError::NoFrames))?;
        let row = size.width as usize * 4;
        let mut bgra = Vec::with_capacity(row * size.height as usize);
        let mut copy_rows = |first: *const u8, pitch: isize| {
            for y in 0..size.height as isize {
                bgra.extend_from_slice(std::slice::from_raw_parts(first.offset(y * pitch), row));
            }
        };
        if let Ok(buffer2d) = sample.GetBufferByIndex(0)?.cast::<IMF2DBuffer>() {
            // Lock2D gives the top row and a signed pitch: orientation is explicit.
            let mut scanline0 = std::ptr::null_mut();
            let mut pitch = 0i32;
            buffer2d.Lock2D(&mut scanline0, &mut pitch)?;
            copy_rows(scanline0, pitch as isize);
            buffer2d.Unlock2D()?;
        } else {
            // Plain buffer: the media type's signed stride says which way rows go;
            // without it, RGB32 is bottom-up.
            let current = reader.GetCurrentMediaType(stream)?;
            let stride = current.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32);
            let stride = stride.unwrap_or(-(row as i32)) as isize;
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            buffer.Lock(&mut data, None, None)?;
            let first =
                if stride < 0 { data.offset(-stride * (size.height as isize - 1)) } else { data };
            copy_rows(first, stride);
            buffer.Unlock()?;
        }
        for px in bgra.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Ok(PixelBuffer::new(crate::geometry::PixelRect::new(0, 0, size.width, size.height), bgra))
    }
}

/// Frame size of the first video stream of an MP4 file.
pub fn probe_mp4(path: &Path) -> crate::Result<PixelSize> {
    let _session = MfSession::start()?;
    // SAFETY: read-only source reader query; released before the session ends.
    let packed = unsafe {
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), None)?;
        let media_type =
            reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?;
        media_type.GetUINT64(&MF_MT_FRAME_SIZE)?
    };
    Ok(PixelSize { width: (packed >> 32) as i32, height: (packed & 0xFFFF_FFFF) as i32 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PixelRect, PixelSize};
    use crate::pixels::PixelBuffer;
    use crate::recording::FrameSink;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn odd_size_is_rejected() {
        assert!(
            Mp4Sink::new(Path::new("x.mp4"), PixelSize { width: 101, height: 64 }, 30).is_err()
        );
    }

    // CI runners (Windows Server) usually lack Media Foundation.
    #[test]
    #[ignore = "media-foundation"]
    fn writes_playable_mp4_of_requested_size() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.mp4");
        let mut s = Mp4Sink::new(&p, PixelSize { width: 320, height: 240 }, 30).unwrap();
        for i in 0..30u64 {
            let frame = PixelBuffer::solid(PixelRect::new(0, 0, 320, 240), (i * 8) as u8, 0, 0);
            s.push(&frame, Duration::from_millis(i * 33)).unwrap();
        }
        s.finish().unwrap();
        drop(s);
        assert!(fs::metadata(&p).unwrap().len() > 1024);
        assert_eq!(probe_mp4(&p).unwrap(), PixelSize { width: 320, height: 240 });
    }

    /// Top half red, bottom half blue: catches flipped rows and swapped channels.
    #[test]
    #[ignore = "media-foundation"]
    fn decoded_frame_keeps_orientation_and_colors() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.mp4");
        let mut frame = PixelBuffer::solid(PixelRect::new(0, 0, 320, 240), 255, 0, 0); // blue
        for y in 0..120 {
            for x in 0..320 {
                frame.put_pixel(crate::geometry::PixelPoint { x, y }, [0, 0, 255, 255]); // red
            }
        }
        let mut s = Mp4Sink::new(&p, PixelSize { width: 320, height: 240 }, 30).unwrap();
        for i in 0..10u64 {
            s.push(&frame, Duration::from_millis(i * 33)).unwrap();
        }
        s.finish().unwrap();
        drop(s);

        let decoded = decode_first_frame(&p).unwrap();
        let top = decoded.pixel(crate::geometry::PixelPoint { x: 160, y: 40 });
        let bottom = decoded.pixel(crate::geometry::PixelPoint { x: 160, y: 200 });
        assert!(top[2] > 200 && top[0] < 60, "top should be red, got BGRA {top:?}");
        assert!(bottom[0] > 200 && bottom[2] < 60, "bottom should be blue, got BGRA {bottom:?}");
    }
}
