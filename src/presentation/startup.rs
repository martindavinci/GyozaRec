//! Command-line options.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartupOptions {
    /// `--capture`: start a capture right away.
    pub capture_now: bool,
}

impl StartupOptions {
    /// `args` excludes the executable name; unknown arguments are ignored.
    pub fn parse(args: &[String]) -> StartupOptions {
        StartupOptions { capture_now: args.iter().any(|a| a.eq_ignore_ascii_case("--capture")) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_detects_capture_flag() {
        assert!(!StartupOptions::parse(&a(&[])).capture_now);
        assert!(StartupOptions::parse(&a(&["--capture"])).capture_now);
        assert!(StartupOptions::parse(&a(&["--CAPTURE"])).capture_now);
        assert!(StartupOptions::parse(&a(&["--foo", "--capture"])).capture_now);
        assert!(!StartupOptions::parse(&a(&["capture"])).capture_now);
    }
}
