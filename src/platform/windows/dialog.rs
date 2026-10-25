use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IDNO, IDYES, MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_SETFOREGROUND, MB_YESNOCANCEL,
    MessageBoxW,
};
use windows::core::HSTRING;

/// The answer to "save the changes?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

/// Asks Yes / No / Cancel over the window `owner` (an HWND); blocks until answered.
pub fn ask_save_changes(owner: isize, question: &str) -> SaveChoice {
    // SAFETY: both strings outlive the call; `owner` is a live window of this thread.
    let answer = unsafe {
        MessageBoxW(
            Some(HWND(owner as *mut _)),
            &HSTRING::from(question),
            &HSTRING::from("GyozaRec"),
            MB_YESNOCANCEL | MB_ICONQUESTION,
        )
    };
    match answer {
        IDYES => SaveChoice::Save,
        IDNO => SaveChoice::Discard,
        _ => SaveChoice::Cancel,
    }
}

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
