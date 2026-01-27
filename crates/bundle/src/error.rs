use thiserror::Error;

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to serialize: {0}")]
    Serialize(serde_json::Error),

    #[error("Failed to deserialize: {0}")]
    Deserialize(serde_json::Error),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Token not found: {0}")]
    TokenNotFound(String),

    #[error("Invalid bundle format: {0}")]
    InvalidFormat(String),
}
