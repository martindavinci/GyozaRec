//! System-wide shortcuts through `global-hotkey`.

use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::HotKey;
use gyozarec::shortcuts::{Binding, Registrar, Unavailable};
use std::collections::HashMap;

/// Registers bindings one by one, so a taken combination only fails itself.
pub struct HotkeyRegistrar {
    manager: Option<GlobalHotKeyManager>,
    live: HashMap<u32, HotKey>,
}

impl HotkeyRegistrar {
    /// Without a manager (Windows refused it) every registration fails.
    pub fn new() -> HotkeyRegistrar {
        HotkeyRegistrar { manager: GlobalHotKeyManager::new().ok(), live: HashMap::new() }
    }
}

impl Registrar for HotkeyRegistrar {
    fn register(&mut self, b: &Binding) -> Result<u32, Unavailable> {
        let manager = self.manager.as_ref().ok_or(Unavailable)?;
        let hotkey: HotKey = b.keys.parse().map_err(|_| Unavailable)?;
        manager.register(hotkey).map_err(|_| Unavailable)?;
        self.live.insert(hotkey.id(), hotkey);
        Ok(hotkey.id())
    }

    fn unregister(&mut self, hotkey_id: u32) {
        if let (Some(manager), Some(hotkey)) = (&self.manager, self.live.remove(&hotkey_id)) {
            let _ = manager.unregister(hotkey);
        }
    }
}

/// Whether `global-hotkey` understands these keys.
pub fn parses(keys: &str) -> bool {
    keys.parse::<HotKey>().is_ok()
}

/// The name of a virtual key as `global-hotkey` spells it; modifiers and
/// keys GyozaRec does not offer give `None`.
fn vk_name(vk: u32) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FUNCTION: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    match vk {
        0x41..=0x5A => Some(LETTERS[(vk - 0x41) as usize]),
        0x30..=0x39 => Some(DIGITS[(vk - 0x30) as usize]),
        0x70..=0x87 => Some(FUNCTION[(vk - 0x70) as usize]),
        0x2C => Some("PrintScreen"),
        0x20 => Some("Space"),
        0x2D => Some("Insert"),
        0x2E => Some("Delete"),
        0x24 => Some("Home"),
        0x23 => Some("End"),
        0x21 => Some("PageUp"),
        0x22 => Some("PageDown"),
        0x25 => Some("Left"),
        0x26 => Some("Up"),
        0x27 => Some("Right"),
        0x28 => Some("Down"),
        _ => None,
    }
}

/// The key printed on the user's keyboard for a physical key: its scan code
/// is translated with the current layout (an AZERTY "A" stays "A").
pub fn key_name(key: &winit::keyboard::PhysicalKey) -> Option<&'static str> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VSC_TO_VK_EX, MapVirtualKeyW};
    use winit::keyboard::{KeyCode, PhysicalKey};
    use winit::platform::scancode::PhysicalKeyExtScancode;
    if *key == PhysicalKey::Code(KeyCode::PrintScreen) {
        return Some("PrintScreen");
    }
    let scancode = key.to_scancode()?;
    // SAFETY: pure translation call with the current keyboard layout.
    let vk = unsafe { MapVirtualKeyW(scancode, MAPVK_VSC_TO_VK_EX) };
    vk_name(vk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyozarec::shortcuts::{PRINT_SCREEN, SHORTCUTS};

    #[test]
    fn every_default_parses_as_a_hotkey() {
        for b in SHORTCUTS.iter().chain([&PRINT_SCREEN]) {
            assert!(parses(b.keys), "{}", b.keys);
        }
    }

    #[test]
    fn vk_table_maps_letters_digits_and_function_keys() {
        assert_eq!(vk_name(0x41), Some("A"));
        assert_eq!(vk_name(0x5A), Some("Z"));
        assert_eq!(vk_name(0x30), Some("0"));
        assert_eq!(vk_name(0x76), Some("F7"));
        assert_eq!(vk_name(0x87), Some("F24"));
        assert_eq!(vk_name(0x2C), Some("PrintScreen"));
        assert_eq!(vk_name(0x10), None, "Shift is a modifier");
        for vk in (0x20..=0x87).filter_map(vk_name) {
            assert!(parses(vk), "{vk} must parse");
        }
    }
}
