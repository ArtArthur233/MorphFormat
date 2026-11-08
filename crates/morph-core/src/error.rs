//! Error type of the engine.

use std::path::PathBuf;

/// Everything that can go wrong while detecting or converting a file.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The file is neither a known image nor a known video.
    #[error("unrecognised file type: {}", .0.display())]
    UnknownInput(PathBuf),
    /// The requested conversion is not available (yet).
    #[error("unsupported conversion: {0}")]
    Unsupported(String),
    /// A feature (`heic`, `video`) was not compiled into this build.
    #[error("{0} support not enabled in this build")]
    FeatureDisabled(&'static str),
    /// The user cancelled the conversion.
    #[error("conversion cancelled")]
    Cancelled,
}

/// Result alias using [`Error`].
pub type Result<T> = std::result::Result<T, Error>;
