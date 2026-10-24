pub mod bridge;
pub mod capture;
pub mod clock;
mod error;
pub mod geometry;
pub mod output;
pub mod paths;
pub mod pixels;
pub mod platform;
pub mod presentation;
pub mod render;
pub mod settings;
#[cfg(test)]
pub(crate) mod testing;
pub mod uikit;

pub use error::{Error, Result};

#[cfg(test)]
mod tests {
    #[test]
    fn error_displays_message() {
        assert_eq!(crate::Error::Message("x".into()).to_string(), "x");
    }
}
