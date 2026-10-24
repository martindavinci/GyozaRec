use crate::capture::SoundPlayer;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::core::w;

/// Plays the system "asterisk" sound.
pub struct SystemSound;

impl SoundPlayer for SystemSound {
    fn play_capture(&self) {
        // SAFETY: static wide string alias, asynchronous playback, no module handle.
        unsafe {
            let _ = PlaySoundW(w!("SystemAsterisk"), None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT);
        }
    }
}
