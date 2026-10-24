//! Where settings and captures live, for installed and portable builds.

use std::path::{Path, PathBuf};

/// A file with this name next to the executable switches to portable mode.
pub const PORTABLE_MARKER: &str = "portable";

#[derive(Clone, Debug, PartialEq)]
pub struct AppPaths {
    pub settings_file: PathBuf,
    pub capture_dir: PathBuf,
    pub portable: bool,
}

impl AppPaths {
    pub fn detect(exe_dir: &Path) -> AppPaths {
        if exe_dir.join(PORTABLE_MARKER).is_file() {
            return AppPaths {
                settings_file: exe_dir.join("data").join("settings.json"),
                capture_dir: exe_dir.join("Captures"),
                portable: true,
            };
        }
        let config = dirs::config_dir().unwrap_or_else(|| exe_dir.to_path_buf());
        let pictures = dirs::picture_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
            .unwrap_or_else(|| exe_dir.to_path_buf());
        AppPaths {
            settings_file: config.join("GyozaRec").join("settings.json"),
            capture_dir: pictures.join("GyozaRec"),
            portable: false,
        }
    }

    /// Paths for the running executable.
    pub fn current() -> AppPaths {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        AppPaths::detect(&exe_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn marker_makes_paths_portable() {
        let d = tempdir().unwrap();
        fs::write(d.path().join(PORTABLE_MARKER), b"").unwrap();
        let p = AppPaths::detect(d.path());
        assert!(p.portable);
        assert_eq!(p.settings_file, d.path().join("data").join("settings.json"));
        assert_eq!(p.capture_dir, d.path().join("Captures"));
    }

    #[test]
    fn without_marker_paths_are_per_user() {
        let d = tempdir().unwrap();
        let p = AppPaths::detect(d.path());
        assert!(!p.portable);
        assert!(!p.settings_file.starts_with(d.path()));
        assert!(p.settings_file.ends_with(Path::new("GyozaRec").join("settings.json")));
        assert!(p.capture_dir.ends_with("GyozaRec"));
    }
}
