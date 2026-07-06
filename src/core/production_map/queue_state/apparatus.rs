use super::super::pechat;

pub fn apparatus_matches_assigned(apparatus: &str, assigned: &[String]) -> bool {
    let apparatus = apparatus.trim();
    if apparatus.is_empty() {
        return false;
    }
    assigned
        .iter()
        .any(|item| apparatus_titles_match(apparatus, item.trim()))
}

pub fn apparatus_titles_match(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == right {
        return true;
    }
    if pechat::apparatus_node_matches_from(left, right)
        || pechat::apparatus_node_matches_from(right, left)
    {
        return true;
    }
    warehouse_base_title(left).eq_ignore_ascii_case(warehouse_base_title(right))
}

pub fn next_stage_title_matches_apparatus(next_stage: &str, apparatus: &str) -> bool {
    if apparatus_titles_match(next_stage, apparatus) {
        return true;
    }
    let next_stage_key = normalized_warehouse_key(next_stage);
    let apparatus_key = normalized_warehouse_key(apparatus);
    stage_label_matches_numbered_apparatus(&next_stage_key, &apparatus_key)
}

pub fn apparatus_search_key(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return String::new();
    }
    if let Some(color_count) = pechat::pechat_color_count(title) {
        return format!("pechat:{color_count}");
    }
    warehouse_base_title(title)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalized_warehouse_key(title: &str) -> String {
    warehouse_base_title(title)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn stage_label_matches_numbered_apparatus(stage_key: &str, apparatus_key: &str) -> bool {
    if stage_key.is_empty() || apparatus_key.is_empty() || apparatus_key == stage_key {
        return false;
    }
    let Some(suffix) = apparatus_key.strip_prefix(stage_key) else {
        return false;
    };
    let suffix = suffix.trim();
    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
}

/// Strips trailing instance suffixes such as ` - A` from warehouse titles.
pub fn warehouse_base_title(title: &str) -> &str {
    let title = title.trim();
    if let Some(idx) = title.rfind(" - ") {
        let suffix = title[idx + 3..].trim();
        if !suffix.is_empty()
            && suffix.len() <= 16
            && suffix
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        {
            return title[..idx].trim();
        }
    }
    title
}

/// Maps a warehouse title to the persisted sequence/state key when suffixes differ.
pub fn resolve_apparatus_storage_key(apparatus: &str, known_keys: &[String]) -> String {
    let apparatus = apparatus.trim();
    if apparatus.is_empty() {
        return String::new();
    }
    if known_keys.iter().any(|key| key.trim() == apparatus) {
        return apparatus.to_string();
    }
    for key in known_keys {
        if apparatus_titles_match(apparatus, key) {
            return key.trim().to_string();
        }
    }
    apparatus.to_string()
}
