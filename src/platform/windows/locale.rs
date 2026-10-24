use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::core::PWSTR;

/// The user's preferred Windows display languages, most preferred first
/// (e.g. `["it-IT", "en-US"]`). Empty if Windows cannot tell.
pub fn system_ui_languages() -> Vec<String> {
    let mut count = 0u32;
    let mut len = 0u32;
    // SAFETY: first call asks for the buffer length; the second fills a buffer
    // of exactly that many UTF-16 units.
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).is_err() {
            return Vec::new();
        }
        let mut buffer = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
        .is_err()
        {
            return Vec::new();
        }
        // Double-NUL-terminated list of NUL-separated tags.
        buffer
            .split(|&c| c == 0)
            .filter(|tag| !tag.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}
