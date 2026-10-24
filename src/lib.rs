mod error;
pub mod geometry;
pub mod platform;

pub use error::{Error, Result};

#[cfg(test)]
mod tests {
    #[test]
    fn error_displays_message() {
        assert_eq!(crate::Error::Message("x".into()).to_string(), "x");
    }
}
