use super::super::ports::RezkaPortError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RezkaServiceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("epc generation failed")]
    EpcGenerationFailed,
    #[error("store write failed: {0}")]
    StoreWrite(String),
    #[error("print failed: {0}")]
    PrintFailed(String),
    #[error("submit failed: {0}")]
    SubmitFailed(String),
}

impl RezkaServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::NotConfigured(_) => "rezka_not_configured",
            Self::EpcGenerationFailed => "epc_generation_failed",
            Self::StoreWrite(_) => "store_write_failed",
            Self::PrintFailed(_) => "print_failed",
            Self::SubmitFailed(_) => "submit_failed",
        }
    }
}

impl From<RezkaPortError> for RezkaServiceError {
    fn from(value: RezkaPortError) -> Self {
        Self::StoreWrite(value.message())
    }
}
