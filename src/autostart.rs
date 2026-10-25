//! Start with Windows: what to do with the per-user "run at sign-in" entry.

use std::path::Path;

/// The entry's command: the quoted executable, started hidden in the tray.
pub fn command_line(exe: &Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartAction {
    Write,
    Remove,
    Nothing,
}

/// While enabled the entry is rewritten whenever it differs, so a portable
/// copy that was moved points to its new place after its next launch.
pub fn action(wanted: bool, current: Option<&str>, expected: &str) -> AutostartAction {
    match (wanted, current) {
        (true, Some(value)) if value == expected => AutostartAction::Nothing,
        (true, _) => AutostartAction::Write,
        (false, Some(_)) => AutostartAction::Remove,
        (false, None) => AutostartAction::Nothing,
    }
}

/// Where the entry lives (the registry on Windows).
pub trait AutostartStore {
    fn read(&self) -> Option<String>;
    fn write(&self, value: &str) -> bool;
    fn remove(&self) -> bool;
}

/// Brings the entry in line with the setting; `false` if the change failed.
pub fn sync(store: &dyn AutostartStore, wanted: bool, exe: &Path) -> bool {
    let expected = command_line(exe);
    match action(wanted, store.read().as_deref(), &expected) {
        AutostartAction::Write => store.write(&expected),
        AutostartAction::Remove => store.remove(),
        AutostartAction::Nothing => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn command_line_quotes_the_path_and_starts_hidden() {
        let exe = Path::new(r"C:\Program Files\GyozaRec\gyozarec.exe");
        assert_eq!(command_line(exe), r#""C:\Program Files\GyozaRec\gyozarec.exe" --autostart"#);
    }

    #[test]
    fn autostart_action_table() {
        let expected = r#""C:\new\gyozarec.exe" --autostart"#;
        let moved = r#""C:\old\gyozarec.exe" --autostart"#;
        assert_eq!(action(true, None, expected), AutostartAction::Write);
        assert_eq!(action(true, Some(moved), expected), AutostartAction::Write);
        assert_eq!(action(true, Some(expected), expected), AutostartAction::Nothing);
        assert_eq!(action(false, Some(expected), expected), AutostartAction::Remove);
        assert_eq!(action(false, Some(moved), expected), AutostartAction::Remove);
        assert_eq!(action(false, None, expected), AutostartAction::Nothing);
    }
}
