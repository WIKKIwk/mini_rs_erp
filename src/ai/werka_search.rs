mod gemini;
mod query;
mod suggestion;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use reqwest::Method;
use std::time::Duration;

use crate::core::werka::models::WerkaAiSearchSuggestion;
use crate::core::werka::ports::{WerkaAiSearch, WerkaAiSearchError, WerkaAiSearchImage};

use self::gemini::GeminiResponse;
use self::suggestion::suggestion_from_raw_text;

const DEFAULT_MODEL: &str = "gemini-flash-lite-latest";
const PROMPT: &str = "Look at the package image and identify only the main brand or product family name that a warehouse worker should type for search. If there is a clear standalone brand, return only that brand. Do not include care words, category words, flavor, size, or descriptors such as cream, care, soap, sovun, milk, dairy, spicy. Prefer the shortest searchable family query and server-friendly retail spelling or transliteration. Examples: HOTLUNCH spicy chicken -> hot lunch, Musaffo -> musaffo, Simba Chips -> simba chips, Yashkino Mini-Rulet -> mini rulet, Nivea Creme Care -> nivea. Return strict JSON with keys search_query, alt_query, visible_brand, visible_text, confidence.";

pub struct WerkaAiSearchService {
    client: reqwest::Client,
    api_key: String,
    endpoint: String,
}

impl WerkaAiSearchService {
    pub fn new(api_key: &str, model: &str, timeout: Duration) -> Self {
        let model = if model.trim().is_empty() {
            DEFAULT_MODEL
        } else {
            model.trim()
        };
        let timeout = if timeout.is_zero() {
            Duration::from_secs(15)
        } else {
            timeout
        };
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client");
        Self {
            client,
            api_key: api_key.trim().to_string(),
            endpoint: format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
            ),
        }
    }
}

#[async_trait]
impl WerkaAiSearch for WerkaAiSearchService {
    async fn infer_suggestion(
        &self,
        image: WerkaAiSearchImage,
    ) -> Result<WerkaAiSearchSuggestion, WerkaAiSearchError> {
        if self.api_key.trim().is_empty() {
            return Err(WerkaAiSearchError::not_configured());
        }
        if image.bytes.is_empty() {
            return Err(WerkaAiSearchError::invalid_image("image is required"));
        }
        let mime_type = if image.mime_type.trim().is_empty() {
            "image/jpeg".to_string()
        } else {
            image.mime_type.trim().to_string()
        };
        let payload = serde_json::json!({
            "contents": [{
                "parts": [
                    { "text": PROMPT },
                    {
                        "inline_data": {
                            "mime_type": mime_type,
                            "data": general_purpose::STANDARD.encode(&image.bytes),
                        }
                    }
                ]
            }],
            "generationConfig": {
                "temperature": 0,
                "responseMimeType": "application/json",
            }
        });
        let response = self
            .client
            .request(
                Method::POST,
                format!("{}?key={}", self.endpoint, self.api_key),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|error| WerkaAiSearchError::upstream(error.to_string()))?;
        let status = response.status();
        let gemini: GeminiResponse = response
            .json()
            .await
            .map_err(|_| WerkaAiSearchError::upstream("invalid ai response"))?;
        if !status.is_success() {
            let message = gemini
                .error
                .map(|error| error.message.trim().to_string())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "AI search request failed".to_string());
            return Err(WerkaAiSearchError::upstream(message));
        }
        let raw_text = gemini
            .candidates
            .first()
            .and_then(|candidate| candidate.content.parts.first())
            .map(|part| part.text.trim())
            .filter(|text| !text.is_empty())
            .ok_or_else(WerkaAiSearchError::no_result)?;
        suggestion_from_raw_text(raw_text)
    }
}
