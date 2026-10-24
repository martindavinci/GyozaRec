//! Windows backend.

mod grab;
mod monitors;
mod window_snapshot;

pub use grab::GdiGrabber;
pub use monitors::{Win32Cursor, Win32Monitors};
pub use window_snapshot::Win32WindowList;

#[cfg(test)]
mod tests;
