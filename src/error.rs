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
    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, Error>;
