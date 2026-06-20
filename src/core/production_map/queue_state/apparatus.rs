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
