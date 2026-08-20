use crate::core::apparatus_standard::ApparatusId;

/// Parse the only value that may identify a live apparatus in queue state.
///
/// The public compatibility names below are retained because queue state is a
/// shared transport surface, but they intentionally no longer inspect titles,
/// warehouse labels, aliases, or instance suffixes.
pub fn canonical_apparatus_id(value: &str) -> Option<ApparatusId> {
    ApparatusId::new(value.trim().to_string()).ok()
}

pub fn apparatus_ids_match(left: &str, right: &str) -> bool {
    match (canonical_apparatus_id(left), canonical_apparatus_id(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

pub fn apparatus_matches_assigned(apparatus: &str, assigned: &[String]) -> bool {
    assigned
        .iter()
        .any(|item| apparatus_ids_match(apparatus, item))
}

pub fn next_stage_apparatus_matches(next_stage: &str, apparatus: &str) -> bool {
    apparatus_ids_match(next_stage, apparatus)
}

/// Return the canonical ID for persisted/search-key consumers.
pub fn apparatus_search_key(value: &str) -> String {
    canonical_apparatus_id(value)
        .map(|id| id.as_str().to_string())
        .unwrap_or_default()
}

/// Resolve only an exact canonical ID. Legacy title/warehouse keys are not
/// aliases and therefore fail closed by returning an empty key.
pub fn resolve_apparatus_storage_key(apparatus: &str, known_keys: &[String]) -> String {
    let Some(id) = canonical_apparatus_id(apparatus) else {
        return String::new();
    };
    known_keys
        .iter()
        .find(|key| apparatus_ids_match(key, id.as_str()))
        .map(|key| key.trim().to_string())
        .unwrap_or_else(|| id.as_str().to_string())
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
