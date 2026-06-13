use crate::core::werka::models::WerkaAiSearchSuggestion;
use crate::core::werka::ports::{WerkaAiSearchError, WerkaAiSearchImage};
use crate::core::werka::service::WerkaService;

impl WerkaService {
    pub fn ai_search_configured(&self) -> bool {
        self.ai_search.is_some()
    }

    pub async fn ai_search_suggestion(
        &self,
        image: WerkaAiSearchImage,
    ) -> Result<WerkaAiSearchSuggestion, WerkaAiSearchError> {
        let Some(search) = &self.ai_search else {
            return Err(WerkaAiSearchError::not_configured());
        };
        search.infer_suggestion(image).await
    }
}
