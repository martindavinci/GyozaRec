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

#[cfg(test)]
mod tests {
    use gyozarec::shortcuts::{PRINT_SCREEN, SHORTCUTS};

    #[test]
    fn every_binding_parses_as_a_hotkey() {
        for b in SHORTCUTS.iter().chain([&PRINT_SCREEN]) {
            assert!(b.keys.parse::<global_hotkey::hotkey::HotKey>().is_ok(), "{}", b.keys);
        }
    }
}
