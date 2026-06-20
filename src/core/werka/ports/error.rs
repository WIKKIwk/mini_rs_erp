#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum WerkaPortError {
    #[error("lookup failed")]
    LookupFailed,
    #[error("database lookup failed: {0}")]
    Database(String),
    #[error("invalid input")]
    InvalidInput,
    #[error("not found")]
    NotFound,
    #[error("lookup unavailable")]
    LookupUnavailable,
    #[error("insufficient stock")]
    InsufficientStock,
    #[error("duplicate customer issue source")]
    DuplicateCustomerIssueSource,
    #[error("write failed: {0}")]
    WriteFailed(String),
}
