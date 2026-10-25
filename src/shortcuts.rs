//! Global shortcuts, the notification area's close policy and how to quit
//! while a capture is running. Platform registration lives in the app.

use crate::quick::QuickAction;
use crate::settings::AppSettings;
use std::collections::HashMap;

/// A built-in shortcut: stable id, default keys, action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultBinding {
    pub id: &'static str,
    pub keys: &'static str,
    pub action: QuickAction,
}

/// Defaults. F12 is avoided: Windows reserves it for the debugger.
pub const SHORTCUTS: [DefaultBinding; 4] = [
    DefaultBinding { id: "image", keys: "Ctrl+Shift+F7", action: QuickAction::Image },
    DefaultBinding {
        id: "image-full",
        keys: "Ctrl+Shift+F8",
        action: QuickAction::ImageFullScreen,
    },
    DefaultBinding { id: "record", keys: "Ctrl+Shift+F9", action: QuickAction::Record },
    DefaultBinding {
        id: "record-full",
        keys: "Ctrl+Shift+F10",
        action: QuickAction::RecordFullScreen,
    },
];

pub const PRINT_SCREEN: DefaultBinding =
    DefaultBinding { id: "print-screen", keys: "PrintScreen", action: QuickAction::Image };

/// One system-wide key combination as it should be registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub id: &'static str,
    /// Accelerator text in `normalize` form, e.g. `Ctrl+Shift+F7`.
    pub keys: String,
    pub action: QuickAction,
}

/// Default keys for a binding id.
pub fn default_keys(id: &str) -> Option<&'static str> {
    SHORTCUTS.iter().chain([&PRINT_SCREEN]).find(|d| d.id == id).map(|d| d.keys)
}

/// The keys a binding uses with these settings (override or default).
pub fn keys_for(s: &AppSettings, id: &str) -> Option<String> {
    s.shortcut_keys.get(id).cloned().or_else(|| default_keys(id).map(str::to_string))
}

/// The bindings to register: defaults with the user's keys, minus the ones
/// switched off; Print Screen only when its option is on.
pub fn effective(s: &AppSettings) -> Vec<Binding> {
    let on = |d: &DefaultBinding| !s.shortcuts_disabled.contains(d.id);
    let binding = |d: &DefaultBinding| {
        let keys = keys_for(s, d.id).unwrap_or_else(|| d.keys.to_string());
        Binding { id: d.id, keys: normalize(&keys).unwrap_or(keys), action: d.action }
    };
    let mut out: Vec<Binding> = if s.shortcuts_enabled {
        SHORTCUTS.iter().filter(|d| on(d)).map(binding).collect()
    } else {
        Vec::new()
    };
    if s.print_screen_captures && on(&PRINT_SCREEN) {
        out.push(binding(&PRINT_SCREEN));
    }
    out
}

const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Super"];
/// Main keys with a fixed spelling; single characters are upper-cased.
const NAMED_KEYS: [&str; 12] = [
    "PrintScreen",
    "Space",
    "Insert",
    "Delete",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Up",
    "Down",
    "Left",
    "Right",
];

fn modifier(token: &str) -> Option<&'static str> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some("Ctrl"),
        "alt" | "option" => Some("Alt"),
        "shift" => Some("Shift"),
        "super" | "win" | "windows" | "meta" | "cmd" | "command" => Some("Super"),
        _ => None,
    }
}

fn main_key(token: &str) -> String {
    if token.chars().count() == 1 {
        return token.to_uppercase();
    }
    let upper = token.to_ascii_uppercase();
    if let Some(n) = upper.strip_prefix('F').filter(|n| n.parse::<u8>().is_ok()) {
        return format!("F{n}");
    }
    NAMED_KEYS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(token))
        .map_or_else(|| token.to_string(), |k| k.to_string())
}

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n))
}

/// Canonical form: modifiers in `Ctrl+Alt+Shift+Super` order, then the key.
pub fn normalize(keys: &str) -> Option<String> {
    let mut mods = Vec::new();
    let mut key = None;
    for token in keys.split('+').map(str::trim) {
        if token.is_empty() || key.is_some() {
            return None; // empty token, or something after the main key
        }
        match modifier(token) {
            Some(m) => mods.push(m),
            None => key = Some(main_key(token)),
        }
    }
    let key = key?;
    let mut parts: Vec<String> =
        MODIFIERS.iter().filter(|m| mods.contains(m)).map(|m| m.to_string()).collect();
    parts.push(key);
    Some(parts.join("+"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    NoModifier,
    Duplicate,
    Reserved,
    Unparsable,
}

/// Checks a new combination against the other bindings' keys; `parses`
/// asks the hotkey library whether it understands the keys.
pub fn validate(
    keys: &str,
    others: &[(&str, String)],
    parses: impl Fn(&str) -> bool,
) -> Result<String, KeyError> {
    let norm = normalize(keys).ok_or(KeyError::Unparsable)?;
    let key = norm.rsplit('+').next().unwrap_or_default();
    let has_modifier = norm.contains('+');
    if key == "F12" {
        return Err(KeyError::Reserved);
    }
    if !has_modifier && !is_function_key(key) && key != "PrintScreen" {
        return Err(KeyError::NoModifier);
    }
    if !parses(&norm) {
        return Err(KeyError::Unparsable);
    }
    if others.iter().any(|(_, other)| normalize(other).as_deref() == Some(norm.as_str())) {
        return Err(KeyError::Duplicate);
    }
    Ok(norm)
}

/// Ids whose keys are the same as another id's, in input order.
pub fn conflicts<'a>(keys: &[(&'a str, String)]) -> Vec<&'a str> {
    let norm: Vec<Option<String>> = keys.iter().map(|(_, k)| normalize(k)).collect();
    keys.iter()
        .enumerate()
        .filter(|(i, _)| {
            norm[*i].is_some() && norm.iter().enumerate().any(|(j, n)| j != *i && *n == norm[*i])
        })
        .map(|(_, (id, _))| *id)
        .collect()
}

/// Settings back to the default shortcuts.
pub fn reset(s: &mut AppSettings) {
    s.shortcut_keys.clear();
    s.shortcuts_disabled.clear();
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
    /// Binding id → (keys, registration result).
    live: HashMap<&'static str, (String, Result<u32, Unavailable>)>,
    actions: HashMap<u32, QuickAction>,
}

impl Shortcuts {
    /// Moves to `wanted`: unchanged bindings stay registered, removed ones are
    /// unregistered, changed or new ones are registered on their own.
    /// Returns the results of the changed or new ones.
    pub fn apply(
        &mut self,
        wanted: Vec<Binding>,
        reg: &mut dyn Registrar,
    ) -> Vec<(&'static str, Result<(), Unavailable>)> {
        // Unregister what is gone or changed; failed ones are retried below.
        let stale: Vec<&'static str> = self
            .live
            .iter()
            .filter(|(id, (keys, result))| {
                result.is_err() || !wanted.iter().any(|b| b.id == **id && b.keys == *keys)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            if let Some((_, Ok(hotkey))) = self.live.remove(id) {
                reg.unregister(hotkey);
                self.actions.remove(&hotkey);
            }
        }
        let mut changed = Vec::new();
        for b in wanted {
            if self.live.contains_key(b.id) {
                continue;
            }
            let result = reg.register(&b);
            if let Ok(hotkey) = result {
                self.actions.insert(hotkey, b.action);
            }
            changed.push((b.id, result.map(|_| ())));
            self.live.insert(b.id, (b.keys, result));
        }
        changed
    }

    /// `None` when the binding is not wanted.
    pub fn status(&self, binding_id: &str) -> Option<Result<(), Unavailable>> {
        self.live.get(binding_id).map(|(_, r)| r.map(|_| ()))
    }

    /// Only ids registered right now resolve.
    pub fn action_for(&self, hotkey_id: u32) -> Option<QuickAction> {
        self.actions.get(&hotkey_id).copied()
    }
}

/// After Windows refused some changed bindings, puts their previous keys
/// back into `new` (taken from `old`), and so for any other edited binding
/// that took keys handed back this way. Returns whether anything changed.
pub fn rollback(new: &mut AppSettings, old: &AppSettings, refused: &[&str]) -> bool {
    let mut changed = false;
    let mut undo: Vec<String> = refused.iter().map(|id| id.to_string()).collect();
    while let Some(id) = undo.pop() {
        let previous = old.shortcut_keys.get(&id).cloned();
        if new.shortcut_keys.get(&id) == previous.as_ref() {
            continue;
        }
        match previous {
            Some(keys) => new.shortcut_keys.insert(id.clone(), keys),
            None => new.shortcut_keys.remove(&id),
        };
        changed = true;
        let restored = keys_for(new, &id).and_then(|k| normalize(&k));
        for d in SHORTCUTS.iter().chain([&PRINT_SCREEN]).filter(|d| d.id != id) {
            if keys_for(new, d.id).and_then(|k| normalize(&k)) == restored {
                undo.push(d.id.to_string());
            }
        }
    }
    changed
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

/// The stage to cancel: a recording wins over a selection, and a selection
/// that has already ended does not count.
pub fn stage(busy: bool, selecting: bool, recording: bool) -> Stage {
    match (busy, recording, selecting) {
        (false, ..) => Stage::Idle,
        (true, true, _) => Stage::Recording,
        (true, false, true) => Stage::Selecting,
        (true, false, false) => Stage::Capturing,
    }
}

/// UI requests a capture can still send after the user asked to quit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incoming {
    Selection,
    Recording,
    Other,
}

/// While quitting, a delayed capture must not open a selection or start a
/// recording; everything else (e.g. the final result) is still handled.
pub fn accepts(exit_pending: bool, incoming: Incoming) -> bool {
    !exit_pending || incoming == Incoming::Other
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
        live: HashMap<u32, String>,
        calls: usize,
    }

    impl Registrar for FakeRegistrar {
        fn register(&mut self, b: &Binding) -> Result<u32, Unavailable> {
            self.calls += 1;
            if self.failing.contains(&b.keys.as_str()) {
                return Err(Unavailable);
            }
            self.next += 1;
            self.live.insert(self.next, b.keys.clone());
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
    fn effective_follows_settings() {
        assert!(effective(&with(false, false)).is_empty());
        assert_eq!(effective(&with(true, false)).len(), 4);
        let both = effective(&with(true, true));
        assert_eq!(both.len(), 5);
        assert_eq!(both[4].id, PRINT_SCREEN.id);
        assert_eq!(effective(&with(false, true)).len(), 1);
    }

    #[test]
    fn effective_applies_overrides_and_disabled() {
        let mut s = with(true, true);
        s.shortcut_keys.insert("record".into(), "Ctrl+Alt+R".into());
        s.shortcuts_disabled.insert("image-full".into());
        s.shortcuts_disabled.insert("print-screen".into());
        let e = effective(&s);
        let ids: Vec<&str> = e.iter().map(|b| b.id).collect();
        assert_eq!(ids, ["image", "record", "record-full"]);
        assert_eq!(e[1].keys, "Ctrl+Alt+R");
        assert_eq!(e[1].action, QuickAction::Record);
    }

    #[test]
    fn defaults_avoid_f12() {
        assert!(SHORTCUTS.iter().all(|b| !b.keys.contains("F12")));
    }

    #[test]
    fn normalize_orders_modifiers() {
        assert_eq!(normalize("shift+ctrl+a").as_deref(), Some("Ctrl+Shift+A"));
        assert_eq!(normalize("Win + Alt + F7").as_deref(), Some("Alt+Super+F7"));
        assert_eq!(normalize("printscreen").as_deref(), Some("PrintScreen"));
        assert_eq!(normalize("Ctrl+Shift"), None, "no main key");
        assert_eq!(normalize("Ctrl+A+B"), None, "two main keys");
        assert_eq!(normalize(""), None);
    }

    #[test]
    fn validate_rejects_no_modifier_duplicate_f12_and_unparsable() {
        let others = [("image", "Ctrl+Shift+F7".to_string())];
        let ok = |_: &str| true;
        assert_eq!(validate("alt+ctrl+r", &others, ok), Ok("Ctrl+Alt+R".into()));
        assert_eq!(validate("F7", &[], ok), Ok("F7".into()));
        assert_eq!(validate("PrintScreen", &[], ok), Ok("PrintScreen".into()));
        assert_eq!(validate("R", &others, ok), Err(KeyError::NoModifier));
        assert_eq!(validate("shift+ctrl+f7", &others, ok), Err(KeyError::Duplicate));
        assert_eq!(validate("Ctrl+F12", &others, ok), Err(KeyError::Reserved));
        assert_eq!(validate("Ctrl+Nope", &others, |_| false), Err(KeyError::Unparsable));
        assert_eq!(validate("Ctrl+", &others, ok), Err(KeyError::Unparsable));
    }

    #[test]
    fn registration_failure_marks_only_that_binding() {
        let mut reg = FakeRegistrar { failing: vec!["Ctrl+Shift+F9"], ..Default::default() };
        let mut s = Shortcuts::default();
        s.apply(effective(&with(true, true)), &mut reg);
        assert_eq!(s.status("record"), Some(Err(Unavailable)));
        for id in ["image", "image-full", "record-full", "print-screen"] {
            assert_eq!(s.status(id), Some(Ok(())), "{id}");
        }
        assert_eq!(reg.live.len(), 4);
    }

    #[test]
    fn diff_apply_keeps_unchanged_registrations() {
        let mut reg = FakeRegistrar::default();
        let mut s = Shortcuts::default();
        s.apply(effective(&with(true, false)), &mut reg);
        assert_eq!(reg.calls, 4);
        let mut changed = with(true, false);
        changed.shortcut_keys.insert("record".into(), "Ctrl+Alt+R".into());
        let results = s.apply(effective(&changed), &mut reg);
        assert_eq!(reg.calls, 5, "only the changed binding registers again");
        assert_eq!(results, vec![("record", Ok(()))]);
        assert_eq!(reg.live.len(), 4);
        assert!(reg.live.values().any(|k| k == "Ctrl+Alt+R"));
        assert!(!reg.live.values().any(|k| k == "Ctrl+Shift+F9"));
    }

    #[test]
    fn shortcuts_reapply_unregisters_removed() {
        let mut reg = FakeRegistrar::default();
        let mut s = Shortcuts::default();
        s.apply(effective(&with(true, true)), &mut reg);
        s.apply(effective(&with(false, false)), &mut reg);
        assert!(reg.live.is_empty());
        assert_eq!(s.status("image"), None);
    }

    #[test]
    fn refused_change_rolls_back_to_previous_keys() {
        let mut reg = FakeRegistrar { failing: vec!["Ctrl+Alt+R"], ..Default::default() };
        let mut s = Shortcuts::default();
        let old = with(true, false);
        s.apply(effective(&old), &mut reg);
        let mut new = old.clone();
        new.shortcut_keys.insert("record".into(), "Ctrl+Alt+R".into());
        let results = s.apply(effective(&new), &mut reg);
        let refused: Vec<&str> =
            results.iter().filter(|(_, r)| r.is_err()).map(|(id, _)| *id).collect();
        assert_eq!(refused, ["record"]);
        assert!(rollback(&mut new, &old, &refused));
        assert!(!new.shortcut_keys.contains_key("record"));
        s.apply(effective(&new), &mut reg);
        assert_eq!(s.status("record"), Some(Ok(())));
        assert!(reg.live.values().any(|k| k == "Ctrl+Shift+F9"));
    }

    /// Image moved to keys another app holds, Record took Image's old keys:
    /// giving Image its keys back means Record must give them up too.
    #[test]
    fn rollback_also_undoes_edits_that_took_the_restored_keys() {
        let old = with(true, false);
        let mut new = old.clone();
        new.shortcut_keys.insert("image".into(), "Ctrl+Alt+I".into());
        new.shortcut_keys.insert("record".into(), "Ctrl+Shift+F7".into());
        new.shortcut_keys.insert("record-full".into(), "Ctrl+Alt+X".into());
        assert!(rollback(&mut new, &old, &["image"]));
        assert!(!new.shortcut_keys.contains_key("image"));
        assert!(!new.shortcut_keys.contains_key("record"));
        assert_eq!(keys_for(&new, "record-full").as_deref(), Some("Ctrl+Alt+X"), "unrelated edit");
        let keys: Vec<String> = effective(&new).into_iter().map(|b| b.keys).collect();
        let unique: std::collections::BTreeSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "{keys:?}");
    }

    #[test]
    fn conflicts_lists_every_id_sharing_keys() {
        let keys = vec![
            ("image", "Ctrl+Shift+F7".to_string()),
            ("record", "shift+ctrl+f7".to_string()),
            ("record-full", "Ctrl+Shift+F10".to_string()),
        ];
        assert_eq!(conflicts(&keys), ["image", "record"]);
        assert!(conflicts(&keys[2..]).is_empty());
    }

    #[test]
    fn reset_clears_overrides_and_disabled() {
        let mut s = with(true, true);
        s.shortcut_keys.insert("record".into(), "Ctrl+Alt+R".into());
        s.shortcuts_disabled.insert("image".into());
        reset(&mut s);
        assert!(s.shortcut_keys.is_empty() && s.shortcuts_disabled.is_empty());
        assert_eq!(keys_for(&s, "record").as_deref(), Some("Ctrl+Shift+F9"));
    }

    #[test]
    fn released_or_unknown_ids_are_ignored() {
        let mut reg = FakeRegistrar::default();
        let mut s = Shortcuts::default();
        s.apply(effective(&with(true, true)), &mut reg);
        let image = reg.live.iter().find(|(_, k)| *k == "Ctrl+Shift+F7").map(|(id, _)| *id);
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
    fn stage_prefers_recording_and_ignores_finished_selection() {
        assert_eq!(stage(false, true, true), Stage::Idle);
        assert_eq!(stage(true, true, true), Stage::Recording);
        assert_eq!(stage(true, true, false), Stage::Selecting);
        assert_eq!(stage(true, false, false), Stage::Capturing);
    }

    #[test]
    fn nothing_new_starts_while_quitting() {
        assert!(!accepts(true, Incoming::Selection));
        assert!(!accepts(true, Incoming::Recording));
        assert!(accepts(true, Incoming::Other));
        assert!(accepts(false, Incoming::Selection));
        assert!(accepts(false, Incoming::Recording));
    }

    #[test]
    fn exit_plan_per_stage() {
        assert_eq!(exit_plan(Stage::Idle), ExitPlan::ExitNow);
        assert_eq!(exit_plan(Stage::Capturing), ExitPlan::WaitForCapture);
        assert_eq!(exit_plan(Stage::Selecting), ExitPlan::CancelSelection);
        assert_eq!(exit_plan(Stage::Recording), ExitPlan::CancelRecording);
    }
}
