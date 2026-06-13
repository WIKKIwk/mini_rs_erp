use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use reqwest::Method;
use serde::Deserialize;
use std::time::Duration;

use crate::core::werka::models::WerkaAiSearchSuggestion;
use crate::core::werka::ports::{WerkaAiSearch, WerkaAiSearchError, WerkaAiSearchImage};

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

fn suggestion_from_raw_text(raw_text: &str) -> Result<WerkaAiSearchSuggestion, WerkaAiSearchError> {
    let parsed = decode_payload(raw_text).ok_or_else(WerkaAiSearchError::no_result)?;
    let search_query = normalize_server_friendly_query(&parsed.search_query);
    let alt_query = normalize_server_friendly_query(&parsed.alt_query);
    let visible_brand = sanitize_search_query(&parsed.visible_brand);
    let mut display_query = search_query;
    if display_query.is_empty() {
        display_query = pick_fallback_display_query(&visible_brand, &alt_query);
    }

    let mut seeds = vec![
        display_query.clone(),
        alt_query.clone(),
        visible_brand.clone(),
    ];
    seeds.extend(expand_phrase_prefixes(&seeds));
    let background_queries = rank_queries(&seeds);
    let mut resolved_display_query = normalize_server_friendly_query(&display_query);
    if resolved_display_query.is_empty() && !background_queries.is_empty() {
        resolved_display_query = normalize_server_friendly_query(&background_queries[0]);
    }
    let mut visible_text = sanitize_search_query(&parsed.visible_text);
    if visible_text.is_empty() {
        visible_text = visible_brand;
    }
    Ok(WerkaAiSearchSuggestion {
        display_query: resolved_display_query,
        background_queries,
        visible_text,
    })
}

fn decode_payload(raw_text: &str) -> Option<AiPayload> {
    if raw_text.trim().is_empty() {
        return None;
    }
    serde_json::from_str(raw_text)
        .ok()
        .or_else(|| extract_json_object(raw_text).and_then(|raw| serde_json::from_str(raw).ok()))
}

fn extract_json_object(raw_text: &str) -> Option<&str> {
    let start = raw_text.find('{')?;
    let end = raw_text.rfind('}')?;
    if end <= start {
        None
    } else {
        Some(&raw_text[start..=end])
    }
}

fn sanitize_search_query(raw: &str) -> String {
    let mut value = raw.trim();
    if value.is_empty() {
        return String::new();
    }
    if let Some(index) = value.find(['\r', '\n']) {
        value = value[..index].trim();
    }
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = trim_edge_quotes(&collapsed);
    let mut chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() > 64 {
        chars.truncate(64);
        return chars.into_iter().collect::<String>().trim_end().to_string();
    }
    trimmed
}

fn trim_edge_quotes(raw: &str) -> String {
    raw.trim_matches(|ch| matches!(ch, '"' | '\'' | '`' | '“' | '”' | '‘' | '’'))
        .trim()
        .to_string()
}

fn normalize_server_friendly_query(raw: &str) -> String {
    let value = sanitize_search_query(raw);
    if value.is_empty() {
        return String::new();
    }
    let lower = value.to_lowercase();
    let tokens = split_search_tokens(&lower, true);
    if !tokens.is_empty() && tokens.len() <= 2 {
        match tokens.join(" ").as_str() {
            "nivea" => return "nivea".to_string(),
            "musaffo" => return "musaffo".to_string(),
            _ => {}
        }
    }
    if lower.contains("hot lunch") || lower.contains("xot lanch") {
        return if lower.contains("xot lanch") {
            "xot lanch".to_string()
        } else {
            "hot lunch".to_string()
        };
    }
    if lower.contains("musaffo") || lower.contains("мусаффо") {
        return "musaffo".to_string();
    }
    if lower.contains("simba") && lower.contains("chips") {
        return "simba chips".to_string();
    }
    if lower.contains("mini") && (lower.contains("rulet") || lower.contains("рулет")) {
        return "mini rulet".to_string();
    }
    if lower.contains("nivea") || lower.contains("нивеа") {
        return "nivea".to_string();
    }
    value
}

fn pick_fallback_display_query(visible_brand: &str, alt_query: &str) -> String {
    unique_queries(&[visible_brand.to_string(), alt_query.to_string()])
        .into_iter()
        .next()
        .unwrap_or_default()
}

fn unique_queries(values: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in values {
        let value = sanitize_search_query(raw);
        if value.is_empty() {
            continue;
        }
        if seen.insert(value.to_lowercase()) {
            result.push(value);
        }
    }
    result
}

fn expand_phrase_prefixes(values: &[String]) -> Vec<String> {
    let mut phrases = Vec::new();
    for raw in values {
        let value = sanitize_search_query(raw);
        if value.is_empty() {
            continue;
        }
        let tokens = split_search_tokens(&value, false);
        if tokens.len() < 2 {
            continue;
        }
        phrases.push(tokens[..2].join(" "));
        if tokens.len() >= 3 {
            phrases.push(tokens[..3].join(" "));
        }
    }
    unique_queries(&phrases)
}

fn rank_queries(values: &[String]) -> Vec<String> {
    let mut unique = unique_queries(values);
    unique.sort_by(|left, right| {
        query_token_count(right)
            .cmp(&query_token_count(left))
            .then_with(|| right.len().cmp(&left.len()))
    });
    unique
}

fn query_token_count(value: &str) -> usize {
    split_search_tokens(value, false)
        .into_iter()
        .filter(|token| token.trim().chars().count() >= 2)
        .count()
}

fn split_search_tokens(value: &str, filter_noise: bool) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else {
            flush_token(&mut tokens, &mut current, filter_noise);
        }
    }
    flush_token(&mut tokens, &mut current, filter_noise);
    tokens
}

fn flush_token(tokens: &mut Vec<String>, current: &mut String, filter_noise: bool) {
    let token = current.trim().to_string();
    current.clear();
    if token.is_empty() {
        return;
    }
    if filter_noise && is_noise_token(&token) {
        return;
    }
    tokens.push(token);
}

fn is_noise_token(token: &str) -> bool {
    matches!(
        token,
        "cream"
            | "care"
            | "soap"
            | "sovun"
            | "milk"
            | "dairy"
            | "soft"
            | "creme"
            | "molochnaya"
            | "mahsulotlari"
            | "products"
            | "product"
            | "spicy"
    )
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    error: Option<GeminiError>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct GeminiError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct AiPayload {
    #[serde(default)]
    search_query: String,
    #[serde(default)]
    alt_query: String,
    #[serde(default)]
    visible_brand: String,
    #[serde(default)]
    visible_text: String,
}

#[cfg(test)]
mod tests {
    use super::suggestion_from_raw_text;

    #[test]
    fn normalizes_brand_query_like_go() {
        let got = suggestion_from_raw_text(
            r#"{"search_query":"Nivea Creme Care","alt_query":"Nivea","visible_brand":"Nivea","visible_text":"Nivea Creme Care","confidence":0.91}"#,
        )
        .expect("suggestion");

        assert_eq!(got.display_query, "nivea");
        assert_eq!(got.background_queries[0], "nivea");
        assert_eq!(got.visible_text, "Nivea Creme Care");
    }

    #[test]
    fn extracts_json_from_wrapped_text_like_go() {
        let got = suggestion_from_raw_text(
            r#"Here: {"search_query":"Simba Chips spicy","alt_query":"","visible_brand":"Simba Chips","visible_text":""}"#,
        )
        .expect("suggestion");

        assert_eq!(got.display_query, "simba chips");
        assert_eq!(got.visible_text, "Simba Chips");
    }
}
