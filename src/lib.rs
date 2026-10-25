pub mod audio;
pub mod autostart;
pub mod bridge;
pub mod capture;
pub mod clock;
pub mod convert;
pub mod crash;
pub mod editor;
mod error;
pub mod geometry;
pub mod i18n;
pub mod instance;
pub mod output;
pub mod paths;
pub mod pixels;
pub mod platform;
pub mod presentation;
pub mod quick;
pub mod recording;
pub mod render;
pub mod settings;
pub mod shortcuts;
pub mod sounds;
#[cfg(test)]
pub(crate) mod testing;
pub mod uikit;
#[cfg(feature = "github-updates")]
pub mod update;

pub use error::{Error, Failure, Result, UserError};

#[cfg(test)]
mod tests {
    #[test]
    fn error_displays_message() {
        assert_eq!(crate::Error::Message("x".into()).to_string(), "x");
    }
}
