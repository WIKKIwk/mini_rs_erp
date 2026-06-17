use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::ProductionMapError;
use super::pechat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusQueueOrderState {
    Pending,
    InProgress,
    Completed,
}

impl ApparatusQueueOrderState {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusQueueAction {
    Start,
    Complete,
}

pub fn next_queue_state(
    current: ApparatusQueueOrderState,
    action: ApparatusQueueAction,
) -> Result<ApparatusQueueOrderState, ProductionMapError> {
    match action {
        ApparatusQueueAction::Start => {
            if current == ApparatusQueueOrderState::Pending {
                Ok(ApparatusQueueOrderState::InProgress)
            } else {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
        }
        ApparatusQueueAction::Complete => {
            if current == ApparatusQueueOrderState::InProgress {
                Ok(ApparatusQueueOrderState::Completed)
            } else {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
        }
    }
}

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

pub fn effective_apparatus_sequence(
    stored_sequence: &[String],
    visible_order_ids: &[String],
) -> Vec<String> {
    let visible: BTreeSet<String> = visible_order_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
        .collect();
    if visible.is_empty() {
        return Vec::new();
    }
    if stored_sequence.is_empty() {
        return visible_order_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect();
    }
    let mut result = Vec::new();
    for id in stored_sequence {
        let id = id.trim();
        if id.is_empty() || !visible.contains(id) {
            continue;
        }
        if !result.iter().any(|existing| existing == id) {
            result.push(id.to_string());
        }
    }
    for id in visible_order_ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if !result.iter().any(|existing| existing == id) {
            result.push(id.to_string());
        }
    }
    result
}

pub fn first_actionable_order_id(
    sequence: &[String],
    states: &BTreeMap<String, ApparatusQueueOrderState>,
) -> Option<String> {
    for id in sequence {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if states
            .get(id)
            .copied()
            .unwrap_or(ApparatusQueueOrderState::Pending)
            == ApparatusQueueOrderState::InProgress
        {
            return Some(id.to_string());
        }
    }
    for id in sequence {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        match states
            .get(id)
            .copied()
            .unwrap_or(ApparatusQueueOrderState::Pending)
        {
            ApparatusQueueOrderState::Completed => continue,
            ApparatusQueueOrderState::InProgress => continue,
            ApparatusQueueOrderState::Pending => return Some(id.to_string()),
        }
    }
    None
}

pub fn apply_queue_action(
    sequence: &[String],
    states: &mut BTreeMap<String, ApparatusQueueOrderState>,
    order_id: &str,
    action: ApparatusQueueAction,
) -> Result<(), ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    let actionable = first_actionable_order_id(sequence, states)
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    if actionable != order_id {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let current = states
        .get(order_id)
        .copied()
        .unwrap_or(ApparatusQueueOrderState::Pending);
    states.insert(order_id.to_string(), next_queue_state(current, action)?);
    Ok(())
}

pub fn apply_unordered_queue_action(
    states: &mut BTreeMap<String, ApparatusQueueOrderState>,
    order_id: &str,
    action: ApparatusQueueAction,
) -> Result<(), ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if matches!(action, ApparatusQueueAction::Start)
        && states.iter().any(|(id, state)| {
            id.trim() != order_id && *state == ApparatusQueueOrderState::InProgress
        })
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let current = states
        .get(order_id)
        .copied()
        .unwrap_or(ApparatusQueueOrderState::Pending);
    states.insert(order_id.to_string(), next_queue_state(current, action)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn first_actionable_skips_completed_orders() {
        let sequence = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut states = BTreeMap::from([("a".to_string(), ApparatusQueueOrderState::Completed)]);
        assert_eq!(
            first_actionable_order_id(&sequence, &states).as_deref(),
            Some("b")
        );
        states.insert("b".to_string(), ApparatusQueueOrderState::InProgress);
        assert_eq!(
            first_actionable_order_id(&sequence, &states).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn first_actionable_prioritizes_in_progress_order() {
        let sequence = vec!["a".to_string(), "b".to_string()];
        let states = BTreeMap::from([("b".to_string(), ApparatusQueueOrderState::InProgress)]);
        assert_eq!(
            first_actionable_order_id(&sequence, &states).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn effective_sequence_uses_visible_order_when_store_empty() {
        let visible = vec!["zakaz-1236".to_string(), "zakaz-6687".to_string()];
        assert_eq!(effective_apparatus_sequence(&[], &visible), visible,);
    }

    #[test]
    fn effective_sequence_skips_missing_orders() {
        let stored = vec![
            "zakaz-old".to_string(),
            "zakaz-1236".to_string(),
            "zakaz-6687".to_string(),
        ];
        let visible = vec!["zakaz-1236".to_string(), "zakaz-6687".to_string()];
        assert_eq!(effective_apparatus_sequence(&stored, &visible), visible,);
    }

    #[test]
    fn start_and_complete_flow() {
        let sequence = vec!["a".to_string(), "b".to_string()];
        let mut states = BTreeMap::new();
        apply_queue_action(&sequence, &mut states, "b", ApparatusQueueAction::Start)
            .expect_err("only first pending order");
        apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Start)
            .expect("start first");
        assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::InProgress));
        apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Complete)
            .expect("complete first");
        assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::Completed));
        apply_queue_action(&sequence, &mut states, "b", ApparatusQueueAction::Start)
            .expect("start second");
    }

    #[test]
    fn unordered_action_allows_any_pending_order() {
        let mut states = BTreeMap::new();
        apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Start)
            .expect("free pick can start later order");
        assert_eq!(states.get("b"), Some(&ApparatusQueueOrderState::InProgress));
        apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Complete)
            .expect("free pick completes started order");
        assert_eq!(states.get("b"), Some(&ApparatusQueueOrderState::Completed));
        apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Start)
            .expect_err("completed order cannot restart");
    }

    #[test]
    fn unordered_action_blocks_second_start_while_order_in_progress() {
        let mut states = BTreeMap::new();
        apply_unordered_queue_action(&mut states, "a", ApparatusQueueAction::Start)
            .expect("start first order");
        let result = apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Start);
        assert_eq!(result, Err(ProductionMapError::QueueActionNotAllowed));
        assert_eq!(states.get("b"), None);
    }

    #[test]
    fn resolve_apparatus_storage_key_matches_pechat_suffixes() {
        let keys = vec![
            "7 ta rangli pechat".to_string(),
            "Godex aparat - DEMO".to_string(),
        ];
        assert_eq!(
            resolve_apparatus_storage_key("7 ta rangli pechat - A", &keys),
            "7 ta rangli pechat"
        );
    }

    #[test]
    fn apparatus_titles_match_warehouse_instance_suffixes() {
        assert!(apparatus_titles_match("Laminatsiya - A", "Laminatsiya"));
        assert!(apparatus_titles_match("Paket aparat - A", "Paket aparat"));
    }

    proptest! {
        #[test]
        fn effective_sequence_only_contains_visible_unique_ids(
            stored in proptest::collection::vec("[a-z]{1,8}", 0..24),
            visible_set in proptest::collection::btree_set("[a-z]{1,8}", 0..24),
        ) {
            let visible = visible_set.into_iter().collect::<Vec<_>>();
            let result = effective_apparatus_sequence(&stored, &visible);
            let visible_set = visible
                .iter()
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect::<BTreeSet<_>>();
            let result_set = result.iter().cloned().collect::<BTreeSet<_>>();
            prop_assert_eq!(result.len(), result_set.len());
            prop_assert!(result.iter().all(|id| visible_set.contains(id)));
            prop_assert!(visible_set.iter().all(|id| result_set.contains(id)));
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    fn symbolic_state(selector: u8) -> ApparatusQueueOrderState {
        let state = match selector % 3 {
            0 => ApparatusQueueOrderState::Pending,
            1 => ApparatusQueueOrderState::InProgress,
            _ => ApparatusQueueOrderState::Completed,
        };
        state
    }

    #[kani::proof]
    fn queue_state_transition_matches_policy() {
        let current = symbolic_state(kani::any::<u8>());
        let action = if kani::any::<bool>() {
            ApparatusQueueAction::Start
        } else {
            ApparatusQueueAction::Complete
        };
        let next = next_queue_state(current, action);
        match (current, action) {
            (ApparatusQueueOrderState::Pending, ApparatusQueueAction::Start) => {
                assert_eq!(next, Ok(ApparatusQueueOrderState::InProgress));
            }
            (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::Complete) => {
                assert_eq!(next, Ok(ApparatusQueueOrderState::Completed));
            }
            _ => {
                assert_eq!(next, Err(ProductionMapError::QueueActionNotAllowed));
            }
        }
    }
}
