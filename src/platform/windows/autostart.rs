//! The per-user "run at sign-in" entry in the registry.

use crate::autostart::AutostartStore;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, w};

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

/// A value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
pub struct RunKeyStore {
    pub value_name: &'static str,
}

impl AutostartStore for RunKeyStore {
    fn read(&self) -> Option<String> {
        let name = HSTRING::from(self.value_name);
        let mut buffer = vec![0u16; 1024];
        let mut bytes = (buffer.len() * 2) as u32;
        // SAFETY: the buffer and its byte length describe valid writable memory.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                &name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if status.is_err() {
            return None;
        }
        let len = (bytes as usize / 2).saturating_sub(1).min(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    fn write(&self, value: &str) -> bool {
        let name = HSTRING::from(self.value_name);
        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `data` is a NUL-terminated UTF-16 string alive for the call.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                &name,
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                (data.len() * 2) as u32,
            )
        };
        status.is_ok()
    }

    fn remove(&self) -> bool {
        let name = HSTRING::from(self.value_name);
        // SAFETY: deletes one named value of the current user's Run key.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, &name).is_ok() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses its own value name, never the real `GyozaRec` entry.
    #[test]
    #[ignore = "desktop"]
    fn run_key_roundtrip() {
        let store = RunKeyStore { value_name: "GyozaRec-test" };
        let _ = store.remove();
        assert_eq!(store.read(), None);
        assert!(store.write(r#""C:\x y\gyozarec.exe" --autostart"#));
        assert_eq!(store.read().as_deref(), Some(r#""C:\x y\gyozarec.exe" --autostart"#));
        assert!(store.remove());
        assert_eq!(store.read(), None);
    }
}
