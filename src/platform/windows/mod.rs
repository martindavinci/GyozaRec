//! Windows backend.

mod clipboard;
mod dialog;
mod grab;
mod locale;
mod monitors;
mod mp4;
mod recording;
mod sound;
mod window_snapshot;

pub use clipboard::Win32Clipboard;
pub use dialog::show_error_box;
pub use grab::GdiGrabber;
pub use locale::{cjk_font_path, system_ui_languages};
pub use monitors::{Win32Cursor, Win32Monitors};
pub use mp4::{Mp4Sink, probe_mp4};
pub use recording::{
    DefaultSinkFactory, ExplorerRevealer, allow_foreground_for_others, allow_tiny_window,
    exclude_from_capture, layered_color_key, make_color_keyed, reveal_in_explorer,
};
pub use sound::SystemSound;
pub use window_snapshot::Win32WindowList;

#[cfg(test)]
mod tests;
