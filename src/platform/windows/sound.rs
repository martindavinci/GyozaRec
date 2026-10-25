use crate::capture::SoundPlayer;
use crate::sounds::{Cue, wav};
use std::sync::OnceLock;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::{PCWSTR, w};

/// The system "asterisk" sound for a saved capture, and GyozaRec's own tones
/// for recording events.
pub struct SystemSound;

/// Played asynchronously from memory, so the bytes must outlive the call.
fn cue_bytes(cue: Cue) -> Option<&'static [u8]> {
    static START: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    static PAUSE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    static RESUME: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    static STOP: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let cell = match cue {
        Cue::Saved => return None,
        Cue::Start => &START,
        Cue::Pause => &PAUSE,
        Cue::Resume => &RESUME,
        Cue::Stop => &STOP,
    };
    cell.get_or_init(|| wav(cue)).as_deref()
}

impl SoundPlayer for SystemSound {
    fn play(&self, cue: Cue) {
        // SAFETY: a static alias, or a WAV image that lives for the whole process.
        unsafe {
            match cue_bytes(cue) {
                None => {
                    let _ = PlaySoundW(
                        w!("SystemAsterisk"),
                        None,
                        SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
                    );
                }
                Some(bytes) => {
                    let image = PCWSTR(bytes.as_ptr().cast());
                    let _ = PlaySoundW(image, None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
                }
            }
        }
    }
}
