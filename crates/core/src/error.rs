use thiserror::Error;

/// Core error type for Heisensim.
#[derive(Error, Debug)]
pub enum HeisensimError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Intercept error: {0}")]
    InterceptError(String),

    #[error("Replay error: {0}")]
    ReplayError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}
