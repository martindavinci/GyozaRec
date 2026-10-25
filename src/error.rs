/// Errors produced by GyozaRec.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    GifEncode(#[from] gif::EncodingError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[cfg(windows)]
    #[error(transparent)]
    Win32(#[from] windows::core::Error),
    #[error(transparent)]
    User(#[from] UserError),
    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Failures GyozaRec explains to the user in their language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UserError {
    #[error("the capture area is empty")]
    EmptyArea,
    #[error("no monitor found")]
    NoMonitor,
    #[error("no frames were recorded")]
    NoFrames,
    #[error("invalid video size {width}×{height} (sides must be even)")]
    InvalidVideoSize { width: i32, height: i32 },
    #[error("recording failed")]
    RecordingFailed,
    #[error("the capture stopped unexpectedly")]
    CaptureFailed,
}

/// What went wrong, as carried to the UI: known failures are translated
/// there; anything else keeps its (usually OS-localized) message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    User(UserError),
    Other(String),
}

impl From<&Error> for Failure {
    fn from(err: &Error) -> Self {
        match err {
            Error::User(user) => Failure::User(*user),
            other => Failure::Other(other.to_string()),
        }
    }
}

impl From<Error> for Failure {
    fn from(err: Error) -> Self {
        Failure::from(&err)
    }
}

impl From<UserError> for Failure {
    fn from(err: UserError) -> Self {
        Failure::User(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_errors_become_translatable_failures() {
        let err = Error::from(UserError::NoFrames);
        assert_eq!(Failure::from(&err), Failure::User(UserError::NoFrames));
    }

    #[test]
    fn other_errors_keep_their_message() {
        let err = Error::Message("boom".into());
        assert_eq!(Failure::from(&err), Failure::Other("boom".into()));
    }
}
