//! Windows backend.

mod clipboard;
mod grab;
mod monitors;
mod mp4;
mod sound;
mod window_snapshot;

pub use clipboard::Win32Clipboard;
pub use grab::GdiGrabber;
pub use monitors::{Win32Cursor, Win32Monitors};
pub use mp4::{Mp4Sink, probe_mp4};
pub use sound::SystemSound;
pub use window_snapshot::Win32WindowList;

#[cfg(test)]
mod tests;
