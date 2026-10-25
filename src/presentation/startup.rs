//! Command-line options.

use crate::quick::QuickAction;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartupOptions {
    /// `--capture`, `--capture-image`, `--capture-fullscreen`, `--record` or
    /// `--record-fullscreen`: start that capture right away.
    pub action: Option<QuickAction>,
}

impl StartupOptions {
    /// `args` excludes the executable name; unknown arguments are ignored.
    pub fn parse(args: &[String]) -> StartupOptions {
        let action =
            args.iter().find_map(|a| a.strip_prefix("--").and_then(QuickAction::from_name));
        StartupOptions { action }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
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
