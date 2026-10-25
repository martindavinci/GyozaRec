//! Tells whether GitHub has a newer release than this copy. Built only with
//! the `github-updates` feature: copies from other stores update there.

/// A release number, `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// Accepts `v0.5.0` and `0.5.0`.
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.strip_prefix('v').unwrap_or(s).split('.');
        let mut next = || parts.next()?.parse::<u32>().ok();
        let version = Version(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }

    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is x.y.z")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    /// The release's page on GitHub.
    pub page: String,
}

pub const RELEASES_HOST: &str = "api.github.com";
pub const RELEASES_PATH: &str = "/repos/martindavinci/GyozaRec/releases/latest";
/// Seconds between two checks.
const DAY: i64 = 86_400;

/// The latest release from GitHub's answer; drafts, pre-releases and pages
/// outside GyozaRec's releases are ignored.
pub fn parse_latest(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let flag = |name: &str| v.get(name).and_then(|f| f.as_bool()).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return None;
    }
    let version = Version::parse(v.get("tag_name")?.as_str()?)?;
    let page = v.get("html_url")?.as_str()?.to_string();
    let note = crate::presentation::UpdateNote { version: version.to_string(), page };
    let page = note.safe_page()?.to_string();
    Some(Release { version, page })
}

pub fn newer(latest: &Release, current: Version) -> bool {
    latest.version > current
}

/// A check is due when there was none, a day has passed, or the clock went back.
pub fn due(last_check: Option<i64>, now: i64) -> bool {
    last_check.is_none_or(|last| now - last >= DAY || last > now)
}

/// Where the latest release is read from.
pub trait ReleaseSource: Send {
    fn latest_json(&self) -> crate::Result<String>;
}

/// The newer release, if any; any failure counts as none.
pub fn check(source: &dyn ReleaseSource, current: Version) -> Option<Release> {
    let release = parse_latest(&source.latest_json().ok()?)?;
    newer(&release, current).then_some(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST: &str = r#"{
        "url": "https://api.github.com/repos/martindavinci/GyozaRec/releases/1",
        "html_url": "https://github.com/martindavinci/GyozaRec/releases/tag/v0.5.0",
        "tag_name": "v0.5.0",
        "draft": false,
        "prerelease": false,
        "assets": []
    }"#;

    struct Answer(Option<&'static str>);

    impl ReleaseSource for Answer {
        fn latest_json(&self) -> crate::Result<String> {
            self.0.map(str::to_string).ok_or_else(|| crate::Error::Message("offline".into()))
        }
    }

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(Version::parse("v0.5.0"), Some(Version(0, 5, 0)));
        assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert!(Version::parse("v0.5.0").unwrap() > Version(0, 4, 0));
        for bad in ["v1", "x.y.z", "", "1.2.3.4", "1.2.-3"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn latest_release_is_read_from_github_json() {
        let r = parse_latest(LATEST).unwrap();
        assert_eq!(r.version, Version(0, 5, 0));
        assert_eq!(r.page, "https://github.com/martindavinci/GyozaRec/releases/tag/v0.5.0");
    }

    #[test]
    fn garbage_or_foreign_release_is_ignored() {
        assert_eq!(parse_latest("<html>rate limited</html>"), None);
        assert_eq!(parse_latest("{}"), None);
        assert_eq!(
            parse_latest(&LATEST.replace(r#""prerelease": false"#, r#""prerelease": true"#)),
            None
        );
        assert_eq!(parse_latest(&LATEST.replace(r#""draft": false"#, r#""draft": true"#)), None);
        let foreign = LATEST.replace(
            "https://github.com/martindavinci/GyozaRec/releases/tag/v0.5.0",
            "https://evil.example/x",
        );
        assert_eq!(parse_latest(&foreign), None);
        let sneaky = LATEST.replace(
            "github.com/martindavinci/GyozaRec/releases/tag",
            "github.com/martindavinci/GyozaRec/releases/../../other",
        );
        assert_eq!(parse_latest(&sneaky), None, "no way out of the releases folder");
    }

    #[test]
    fn check_is_due_once_a_day() {
        let now = 1_800_000_000;
        assert!(due(None, now));
        assert!(!due(Some(now - 3600), now));
        assert!(due(Some(now - 86_400), now));
        assert!(due(Some(now + 3600), now), "clock moved back");
    }

    #[test]
    fn failing_source_means_no_update() {
        assert_eq!(check(&Answer(None), Version(0, 4, 0)), None);
        assert_eq!(check(&Answer(Some("{oops")), Version(0, 4, 0)), None);
        assert_eq!(check(&Answer(Some(LATEST)), Version(0, 5, 0)), None, "same version");
        assert_eq!(
            check(&Answer(Some(LATEST)), Version(0, 4, 0)).unwrap().version,
            Version(0, 5, 0)
        );
    }
}
