pub(super) fn sanitize_search_query(raw: &str) -> String {
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

pub(super) fn normalize_server_friendly_query(raw: &str) -> String {
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

pub(super) fn pick_fallback_display_query(visible_brand: &str, alt_query: &str) -> String {
    unique_queries(&[visible_brand.to_string(), alt_query.to_string()])
        .into_iter()
        .next()
        .unwrap_or_default()
}

pub(super) fn unique_queries(values: &[String]) -> Vec<String> {
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

pub(super) fn expand_phrase_prefixes(values: &[String]) -> Vec<String> {
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

pub(super) fn rank_queries(values: &[String]) -> Vec<String> {
    let mut unique = unique_queries(values);
    unique.sort_by(|left, right| {
        query_token_count(right)
            .cmp(&query_token_count(left))
            .then_with(|| right.len().cmp(&left.len()))
    });
    unique
}

fn trim_edge_quotes(raw: &str) -> String {
    raw.trim_matches(|ch| matches!(ch, '"' | '\'' | '`' | '“' | '”' | '‘' | '’'))
        .trim()
        .to_string()
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
