use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW};
use windows::core::HSTRING;

/// A blocking error box without an owner window, usable from any thread.
pub fn show_error_box(message: &str) {
    // SAFETY: both strings outlive the call; no owner window is involved.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from("GyozaRec"),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}
