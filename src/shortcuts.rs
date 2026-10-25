//! Global shortcuts, the notification area's close policy and how to quit
//! while a capture is running. Platform registration lives in the app.

use crate::quick::QuickAction;
use crate::settings::AppSettings;
use std::collections::HashMap;

/// One system-wide key combination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Stable id, also used to show its status in Options.
    pub id: &'static str,
    /// Accelerator text, e.g. `Ctrl+Shift+F7`.
    pub keys: &'static str,
    pub action: QuickAction,
}

/// Fixed bindings. F12 is avoided: Windows reserves it for the debugger.
pub const SHORTCUTS: [Binding; 4] = [
    Binding { id: "image", keys: "Ctrl+Shift+F7", action: QuickAction::Image },
    Binding { id: "image-full", keys: "Ctrl+Shift+F8", action: QuickAction::ImageFullScreen },
    Binding { id: "record", keys: "Ctrl+Shift+F9", action: QuickAction::Record },
    Binding { id: "record-full", keys: "Ctrl+Shift+F10", action: QuickAction::RecordFullScreen },
];

pub const PRINT_SCREEN: Binding =
    Binding { id: "print-screen", keys: "PrintScreen", action: QuickAction::Image };

/// The bindings the settings ask for.
pub fn wanted(s: &AppSettings) -> Vec<Binding> {
    let mut out = if s.shortcuts_enabled { SHORTCUTS.to_vec() } else { Vec::new() };
    if s.print_screen_captures {
        out.push(PRINT_SCREEN);
    }
    out
}

/// Registers system-wide hotkeys; implemented with `global-hotkey` in the app.
pub trait Registrar {
    /// The hotkey id events will carry, or `Err` if another app owns the keys.
    fn register(&mut self, b: &Binding) -> Result<u32, Unavailable>;
    fn unregister(&mut self, hotkey_id: u32);
}

/// The keys are taken by another app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable;

/// Registered bindings, keyed by binding id (two bindings may share an action).
#[derive(Default)]
pub struct Shortcuts {
    status: HashMap<&'static str, Result<u32, Unavailable>>,
    actions: HashMap<u32, QuickAction>,
}

impl Shortcuts {
    /// Replaces the registered set; each binding succeeds or fails on its own.
    pub fn apply(&mut self, wanted: Vec<Binding>, reg: &mut dyn Registrar) {
        for id in self.actions.keys() {
            reg.unregister(*id);
        }
        self.actions.clear();
        self.status.clear();
        for b in wanted {
            let result = reg.register(&b);
            if let Ok(id) = result {
                self.actions.insert(id, b.action);
            }
            self.status.insert(b.id, result);
        }
    }

    /// `None` when the binding is not wanted.
    pub fn status(&self, binding_id: &str) -> Option<Result<(), Unavailable>> {
        self.status.get(binding_id).map(|r| r.map(|_| ()))
    }

    /// Only ids registered right now resolve.
    pub fn action_for(&self, hotkey_id: u32) -> Option<QuickAction> {
        self.actions.get(&hotkey_id).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosePolicy {
    HideToTray,
    Exit,
}

/// Closing the toolbar only hides it while the notification icon really
/// exists, so GyozaRec can always be reached again.
pub fn close_policy(tray_active: bool) -> ClosePolicy {
    if tray_active { ClosePolicy::HideToTray } else { ClosePolicy::Exit }
}

/// Where a capture is when the user asks to quit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    /// Waiting for the delay, or saving an image.
    Capturing,
    Selecting,
    Recording,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitPlan {
    ExitNow,
    WaitForCapture,
    CancelSelection,
    CancelRecording,
}

pub fn exit_plan(stage: Stage) -> ExitPlan {
    match stage {
        Stage::Idle => ExitPlan::ExitNow,
        Stage::Capturing => ExitPlan::WaitForCapture,
        Stage::Selecting => ExitPlan::CancelSelection,
        Stage::Recording => ExitPlan::CancelRecording,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quick::QuickAction;
    use crate::settings::AppSettings;
    use std::collections::HashMap;

    /// Hands out ids like the real manager; fails on the listed keys.
    #[derive(Default)]
    struct FakeRegistrar {
        failing: Vec<&'static str>,
        next: u32,
        live: HashMap<u32, &'static str>,
    }

    impl Registrar for FakeRegistrar {
        fn register(&mut self, b: &Binding) -> Result<u32, Unavailable> {
            if self.failing.contains(&b.keys) {
                return Err(Unavailable);
            }
            self.next += 1;
            self.live.insert(self.next, b.keys);
            Ok(self.next)
        }

        fn unregister(&mut self, hotkey_id: u32) {
            self.live.remove(&hotkey_id);
        }
    }

    fn with(shortcuts: bool, print_screen: bool) -> AppSettings {
        AppSettings {
            shortcuts_enabled: shortcuts,
            print_screen_captures: print_screen,
            ..AppSettings::default()
        }
    }

    #[test]
    fn wanted_follows_settings() {
        assert!(wanted(&with(false, false)).is_empty());
        assert_eq!(wanted(&with(true, false)).len(), 4);
        let both = wanted(&with(true, true));
        assert_eq!(both.len(), 5);
        assert_eq!(both[4].id, PRINT_SCREEN.id);
        assert_eq!(wanted(&with(false, true)).len(), 1);
    }

    #[test]
    fn bindings_avoid_f12() {
        assert!(SHORTCUTS.iter().all(|b| !b.keys.contains("F12")));
    }

    #[test]
    fn registration_failure_marks_only_that_binding() {
        let mut reg = FakeRegistrar { failing: vec!["Ctrl+Shift+F9"], ..Default::default() };
        let mut s = Shortcuts::default();
        s.apply(wanted(&with(true, true)), &mut reg);
        assert_eq!(s.status("record"), Some(Err(Unavailable)));
        for id in ["image", "image-full", "record-full", "print-screen"] {
            assert_eq!(s.status(id), Some(Ok(())), "{id}");
        }
        assert_eq!(reg.live.len(), 4);
    }

    #[test]
    fn shortcuts_reapply_unregisters_previous() {
        let mut reg = FakeRegistrar::default();
        let mut s = Shortcuts::default();
        s.apply(wanted(&with(true, true)), &mut reg);
        s.apply(wanted(&with(false, false)), &mut reg);
        assert!(reg.live.is_empty());
        assert_eq!(s.status("image"), None);
    }

    #[test]
    fn released_or_unknown_ids_are_ignored() {
        let mut reg = FakeRegistrar::default();
        let mut s = Shortcuts::default();
        s.apply(wanted(&with(true, true)), &mut reg);
        let image = reg.live.iter().find(|(_, k)| **k == "Ctrl+Shift+F7").map(|(id, _)| *id);
        assert_eq!(s.action_for(image.unwrap()), Some(QuickAction::Image));
        assert_eq!(s.action_for(999), None);
        s.apply(Vec::new(), &mut reg);
        assert_eq!(s.action_for(image.unwrap()), None, "no longer registered");
    }

    #[test]
    fn close_policy_uses_actual_tray() {
        assert_eq!(close_policy(true), ClosePolicy::HideToTray);
        assert_eq!(close_policy(false), ClosePolicy::Exit);
    }

    #[test]
    fn exit_plan_per_stage() {
        assert_eq!(exit_plan(Stage::Idle), ExitPlan::ExitNow);
        assert_eq!(exit_plan(Stage::Capturing), ExitPlan::WaitForCapture);
        assert_eq!(exit_plan(Stage::Selecting), ExitPlan::CancelSelection);
        assert_eq!(exit_plan(Stage::Recording), ExitPlan::CancelRecording);
    }
}
