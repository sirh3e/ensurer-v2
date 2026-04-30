use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommonError {
    #[error("invalid UUID: {0}")]
    InvalidId(#[from] uuid::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
