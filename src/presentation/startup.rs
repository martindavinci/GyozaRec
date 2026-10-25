//! Command-line options.

use crate::instance::InstanceCommand;
use crate::quick::QuickAction;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartupOptions {
    /// `--capture`, `--capture-image`, `--capture-fullscreen`, `--record` or
    /// `--record-fullscreen`: start that capture right away.
    pub action: Option<QuickAction>,
    /// `--autostart`: launched at sign-in; start quietly in the tray.
    pub autostart: bool,
}

impl StartupOptions {
    /// `args` excludes the executable name; unknown arguments are ignored.
    pub fn parse(args: &[String]) -> StartupOptions {
        let action =
            args.iter().find_map(|a| a.strip_prefix("--").and_then(QuickAction::from_name));
        let autostart = args.iter().any(|a| a.eq_ignore_ascii_case("--autostart"));
        StartupOptions { action, autostart }
    }

    /// What a second launch sends to the running instance; a sign-in launch
    /// sends nothing (GyozaRec is already running).
    pub fn secondary_command(&self) -> Option<InstanceCommand> {
        match (self.action, self.autostart) {
            (Some(action), _) => Some(InstanceCommand::Quick(action)),
            (None, true) => None,
            (None, false) => Some(InstanceCommand::Activate),
        }
    }
}

/// The toolbar starts hidden only for a sign-in launch with a working tray icon.
pub fn startup_visible(autostart: bool, tray_active: bool) -> bool {
    !(autostart && tray_active)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn autostart_flag_parses() {
        assert!(StartupOptions::parse(&a(&["--autostart"])).autostart);
        assert!(!StartupOptions::parse(&a(&[])).autostart);
    }

    #[test]
    fn secondary_autostart_sends_nothing() {
        use crate::instance::InstanceCommand;
        use crate::quick::QuickAction;
        assert_eq!(StartupOptions::parse(&a(&["--autostart"])).secondary_command(), None);
        assert_eq!(
            StartupOptions::parse(&a(&[])).secondary_command(),
            Some(InstanceCommand::Activate)
        );
        assert_eq!(
            StartupOptions::parse(&a(&["--record"])).secondary_command(),
            Some(InstanceCommand::Quick(QuickAction::Record))
        );
    }

    #[test]
    fn startup_visibility_policy() {
        assert!(startup_visible(false, true));
        assert!(startup_visible(false, false));
        assert!(!startup_visible(true, true), "autostart with a tray icon starts hidden");
        assert!(startup_visible(true, false), "without a tray icon it must be reachable");
    }

    #[test]
    fn parse_reads_every_quick_flag() {
        use crate::quick::QuickAction::*;
        let action = |v: &[&str]| StartupOptions::parse(&a(v)).action;
        assert_eq!(action(&[]), None);
        assert_eq!(action(&["--capture"]), Some(Current));
        assert_eq!(action(&["--CAPTURE"]), Some(Current));
        assert_eq!(action(&["--foo", "--capture-image"]), Some(Image));
        assert_eq!(action(&["--capture-fullscreen"]), Some(ImageFullScreen));
        assert_eq!(action(&["--record"]), Some(Record));
        assert_eq!(action(&["--record-fullscreen"]), Some(RecordFullScreen));
        assert_eq!(action(&["capture"]), None);
    }
}
