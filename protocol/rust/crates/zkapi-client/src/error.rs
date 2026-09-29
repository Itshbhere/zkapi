//! Client error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("no active note")]
    NoActiveNote,
    #[error("note already exists")]
    NoteAlreadyExists,
    #[error("insufficient balance: need {needed}, have {available}")]
    InsufficientBalance { needed: u128, available: u128 },
    #[error("stale root")]
    StaleRoot,
    #[error("server error: {0}")]
    ServerError(String),
    #[error("invalid server response: {0}")]
    InvalidResponse(String),
    #[error("pending request exists, must recover first")]
    PendingRequest,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("proof generation failed: {0}")]
    ProofGeneration(String),
    #[error("verification failed: {0}")]
    VerificationFailed(String),
}
