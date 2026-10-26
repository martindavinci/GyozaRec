//! Windows backend.

mod audio;
mod autostart;
mod clipboard;
mod dialog;
mod grab;
#[cfg(feature = "github-updates")]
mod http;
mod locale;
mod monitors;
mod mp4;
mod random;
mod recording;
mod sound;
mod video_reader;
mod window_snapshot;

pub use audio::LoopbackStarter;
pub use autostart::RunKeyStore;
pub use clipboard::Win32Clipboard;
pub use dialog::{SaveChoice, ask_save_changes, show_error_box};
pub use grab::GdiGrabber;
#[cfg(feature = "github-updates")]
pub use http::GitHubReleases;
pub use locale::{cjk_font_path, system_ui_languages};
pub use monitors::{Win32Cursor, Win32Monitors};
pub use mp4::{Mp4Sink, probe_mp4};
pub use random::random_bytes;
pub use recording::{
    DefaultSinkFactory, ExplorerRevealer, allow_foreground_for_others, allow_tiny_window,
    exclude_from_capture, layered_color_key, make_color_keyed, reveal_in_explorer,
    show_without_activating,
};
pub use sound::SystemSound;
pub use video_reader::MfVideoReader;
pub use window_snapshot::Win32WindowList;

#[cfg(test)]
mod tests;
