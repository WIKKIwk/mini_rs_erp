use serde::Deserialize;

use crate::core::werka::models::WerkaAiSearchSuggestion;
use crate::core::werka::ports::WerkaAiSearchError;

use super::query::{
    expand_phrase_prefixes, normalize_server_friendly_query, pick_fallback_display_query,
    rank_queries, sanitize_search_query,
};

pub(super) fn suggestion_from_raw_text(
    raw_text: &str,
) -> Result<WerkaAiSearchSuggestion, WerkaAiSearchError> {
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
