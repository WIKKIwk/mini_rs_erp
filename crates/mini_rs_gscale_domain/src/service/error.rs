use super::super::models::ScaleDriverPrintResponse;
use super::super::ports::GscalePortError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GscaleServiceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("epc generation failed")]
    EpcGenerationFailed,
    #[error("store write failed: {0}")]
    StoreWrite(String),
    #[error("print failed: {detail}")]
    PrintFailed {
        detail: String,
        delete_error: Option<String>,
    },
    #[error("submit failed: {0}")]
    SubmitFailed(String),
}

impl GscaleServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::NotConfigured(_) => "gscale_not_configured",
            Self::EpcGenerationFailed => "epc_generation_failed",
            Self::StoreWrite(_) => "store_write_failed",
            Self::PrintFailed { .. } => "print_failed",
            Self::SubmitFailed(_) => "submit_failed",
        }
    }
}

pub(super) fn map_receipt_store_error(error: GscalePortError) -> GscaleServiceError {
    match error {
        GscalePortError::InvalidInput(detail) => GscaleServiceError::InvalidInput(detail),
        GscalePortError::NotConfigured(detail) => GscaleServiceError::NotConfigured(detail),
        GscalePortError::StoreWrite(detail) => GscaleServiceError::StoreWrite(detail),
        GscalePortError::Driver(detail) => GscaleServiceError::StoreWrite(detail),
    }
}

pub(super) fn print_done(print: &ScaleDriverPrintResponse) -> bool {
    print.ok && print.status.trim().eq_ignore_ascii_case("done")
}

pub(super) fn print_error_detail(print: &ScaleDriverPrintResponse) -> String {
    for value in [&print.detail, &print.error, &print.status] {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "print failed".to_string()
}

pub(super) fn clean_store_error(message: &str) -> String {
    message
        .trim()
        .strip_prefix("store write failed: ")
        .unwrap_or_else(|| message.trim())
        .to_string()
}
