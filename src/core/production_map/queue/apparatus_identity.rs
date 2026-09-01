use crate::core::apparatus_standard::ApparatusId;

/// Parse the only value that may identify a live apparatus in queue state.
///
/// The public compatibility names below are retained because queue state is a
/// shared transport surface, but they intentionally no longer inspect titles,
/// warehouse labels, aliases, or instance suffixes.
pub fn is_canonical_apparatus_id(value: &str) -> bool {
    ApparatusId::is_valid(value.trim())
}

pub fn apparatus_ids_match(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    left == right && ApparatusId::is_valid(left)
}

pub fn apparatus_matches_assigned(apparatus: &str, assigned: &[String]) -> bool {
    let apparatus = apparatus.trim();
    ApparatusId::is_valid(apparatus)
        && assigned.iter().any(|item| item.trim() == apparatus)
}

pub fn next_stage_apparatus_matches(next_stage: &str, apparatus: &str) -> bool {
    apparatus_ids_match(next_stage, apparatus)
}

/// Return the canonical ID for persisted/search-key consumers.
pub fn apparatus_search_key(value: &str) -> String {
    let value = value.trim();
    if is_canonical_apparatus_id(value) {
        value.to_string()
    } else {
        String::new()
    }
}

/// Resolve only an exact canonical ID. Legacy title/warehouse keys are not
/// aliases and therefore fail closed by returning an empty key.
pub fn resolve_apparatus_storage_key(apparatus: &str, known_keys: &[String]) -> String {
    let apparatus = apparatus.trim();
    if !ApparatusId::is_valid(apparatus) {
        return String::new();
    }
    known_keys
        .iter()
        .find(|key| key.trim() == apparatus)
        .map(|key| key.trim().to_string())
        .unwrap_or_else(|| apparatus.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> String {
        value.to_string()
    }

    #[test]
    fn queue_identity_requires_canonical_ids() {
        let assigned = vec![id("apparatus:catalog:press-001")];
        assert!(apparatus_matches_assigned(
            "apparatus:catalog:press-001",
            &assigned
        ));
        assert!(!apparatus_matches_assigned("7 ta rangli pechat", &assigned));
        assert!(!apparatus_matches_assigned(
            "apparatus:catalog:press-002",
            &assigned
        ));
    }

    #[test]
    fn queue_identity_does_not_match_titles_or_instance_suffixes() {
        assert!(!apparatus_ids_match("Laminatsiya - A", "Laminatsiya"));
        assert!(apparatus_ids_match(
            " apparatus:catalog:lam-001 ",
            "apparatus:catalog:lam-001"
        ));
        assert!(!apparatus_ids_match(
            "apparatus:catalog:lam-001",
            "apparatus:catalog:lam-002"
        ));
        assert!(apparatus_ids_match(
            "apparatus:catalog:lam-001",
            "apparatus:catalog:lam-001"
        ));
    }

    #[test]
    fn storage_resolution_is_exact_and_fails_closed_for_legacy_keys() {
        let keys = vec![id("Laminatsiya - A"), id("apparatus:catalog:lam-001")];
        assert_eq!(
            resolve_apparatus_storage_key("apparatus:catalog:lam-001", &keys),
            "apparatus:catalog:lam-001"
        );
        assert_eq!(resolve_apparatus_storage_key("Laminatsiya", &keys), "");
    }

    #[test]
    fn search_key_is_canonical_id_only() {
        assert_eq!(
            apparatus_search_key("apparatus:catalog:press-001"),
            "apparatus:catalog:press-001"
        );
        assert_eq!(apparatus_search_key("8 ta rangli pechat - A"), "");
    }
}
