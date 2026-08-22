use thiserror::Error;

use super::super::isa95::CanonicalApparatusValidationError;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanonicalApparatusError {
    #[error("canonical apparatus was not found")]
    NotFound,
    #[error("canonical apparatus already exists")]
    AlreadyExists,
    #[error("canonical apparatus revision conflict")]
    RevisionConflict,
    #[error("canonical apparatus stable identity conflicts with the request")]
    IdentityConflict,
    #[error("retired apparatus configuration is immutable")]
    Retired,
    #[error("canonical apparatus draft is invalid: {0}")]
    InvalidRevision(#[from] CanonicalApparatusValidationError),
    #[error("AASX package is invalid")]
    InvalidAasx,
    #[error("stored canonical artifact failed its integrity check")]
    ArtifactIntegrity,
    #[error("canonical apparatus persistence failed")]
    Persistence,
    #[error("canonical apparatus cutover is blocked: {0}")]
    CutoverBlocked(String),
    #[error("system clock cannot produce canonical revision provenance")]
    Clock,
    #[error("canonical apparatus transaction fault was injected at {0}")]
    InjectedFault(&'static str),
}
