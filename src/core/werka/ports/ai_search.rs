use async_trait::async_trait;

use crate::core::werka::models::WerkaAiSearchSuggestion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WerkaAiSearchImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WerkaAiSearchError {
    pub code: &'static str,
    pub message: String,
}

impl WerkaAiSearchError {
    pub fn not_configured() -> Self {
        Self {
            code: "not_configured",
            message: "werka ai search is not configured".to_string(),
        }
    }

    pub fn invalid_image(message: &str) -> Self {
        Self {
            code: "invalid_image",
            message: message.to_string(),
        }
    }

    pub fn no_result() -> Self {
        Self {
            code: "no_result",
            message: "no ai search suggestion".to_string(),
        }
    }

    pub fn upstream(message: impl Into<String>) -> Self {
        Self {
            code: "upstream_failed",
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait WerkaAiSearch: Send + Sync {
    async fn infer_suggestion(
        &self,
        image: WerkaAiSearchImage,
    ) -> Result<WerkaAiSearchSuggestion, WerkaAiSearchError>;
}
