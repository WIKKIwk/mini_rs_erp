//! Operation-based queue editing. The version describes one apparatus, not a
//! screen's filtered list. Persistence must validate and apply under one lock.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::*;
use crate::core::apparatus_standard::RuntimeApparatusConfiguration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceMove {
    pub apparatus: String,
    pub order_id: String,
    #[serde(default)]
    pub before_order_id: Option<String>,
    #[serde(default)]
    pub after_order_id: Option<String>,
    pub expected_version: String,
    pub idempotency_key: String,
}

impl SequenceMove {
    pub fn validate(&self) -> Result<(), ProductionMapError> {
        if !queue_state::is_canonical_apparatus_id(&self.apparatus)
            || self.order_id.trim().is_empty()
            || self.order_id != self.order_id.trim()
            || self.expected_version.len() != 64
            || !self.expected_version.bytes().all(|c| c.is_ascii_hexdigit())
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 200
            || (self.before_order_id.is_some() && self.after_order_id.is_some())
            || self
                .before_order_id
                .iter()
                .chain(&self.after_order_id)
                .any(|id| id.trim().is_empty() || id != id.trim() || id == &self.order_id)
        {
            return Err(ProductionMapError::QueueReorderInvalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceMoveResult {
    pub order_ids: Vec<String>,
    pub version: String,
    pub adjusted: bool,
    #[serde(default)]
    pub revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<serde_json::Value>,
}

pub fn compute_canonical_sequence_delta(
    old_sequence: &[String],
    new_sequence: &[String],
    moved_order_id: &str,
) -> Option<serde_json::Value> {
    if old_sequence == new_sequence {
        return None;
    }
    let new_idx = new_sequence.iter().position(|id| id == moved_order_id)?;
    let old_idx = old_sequence.iter().position(|id| id == moved_order_id);
    if old_idx == Some(new_idx) {
        return None;
    }
    let before_id = if new_idx + 1 < new_sequence.len() {
        Some(new_sequence[new_idx + 1].clone())
    } else {
        None
    };
    let after_id = if new_idx > 0 {
        Some(new_sequence[new_idx - 1].clone())
    } else {
        None
    };
    Some(serde_json::json!({
        "type": "move",
        "id": moved_order_id,
        "before_id": before_id,
        "after_id": after_id,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct SequenceMoveState {
    pub apparatus: String,
    pub order_ids: Vec<String>,
    pub states: BTreeMap<String, String>,
    pub frozen: BTreeSet<String>,
}

impl SequenceMoveState {
    pub fn from_data(
        canonical: &RuntimeApparatusConfiguration,
        maps: &[ProductionMapDefinition],
        stored: &[String],
        raw_states: &BTreeMap<String, String>,
        frozen: &BTreeSet<String>,
        preflight_orders: &BTreeSet<String>,
    ) -> Self {
        let visible = apparatus::selected_order_ids_for_apparatus(maps, canonical);
        let frozen = visible
            .iter()
            .filter(|id| {
                frozen.contains(*id) || raw_states.get(*id).is_some_and(|state| state == "frozen")
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        let order_ids =
            queue_state::effective_apparatus_sequence_excluding(stored, &visible, &frozen);
        let states = order_ids
            .iter()
            .map(|id| {
                let state = raw_states.get(id).map(String::as_str).unwrap_or("pending");
                let state = if preflight_orders.contains(id) {
                    "print_preflight"
                } else if state == "paused" && pechat::is_pechat_apparatus(canonical) {
                    "in_progress"
                } else {
                    state
                };
                (id.clone(), state.to_string())
            })
            .collect();
        Self {
            apparatus: canonical.runtime.apparatus_id.to_string(),
            order_ids,
            states,
            frozen,
        }
    }

    pub fn version(&self) -> String {
        // All collections have deterministic ordering. This fingerprints the
        // effective order/barriers of this apparatus, not the global revision.
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(self).expect("queue state is serializable"))
        )
    }

    pub fn apply(&self, command: &SequenceMove) -> Result<SequenceMoveResult, ProductionMapError> {
        command.validate()?;
        if command.apparatus != self.apparatus {
            return Err(ProductionMapError::QueueReorderInvalid);
        }
        if self.frozen.contains(&command.order_id) {
            return Err(ProductionMapError::QueueReorderFrozen);
        }
        if command.expected_version != self.version() {
            return Err(ProductionMapError::QueueReorderConflict);
        }
        let mut requested = self.order_ids.clone();
        let original = requested
            .iter()
            .position(|id| id == &command.order_id)
            .ok_or(ProductionMapError::QueueReorderConflict)?;
        requested.remove(original);
        let target = if let Some(id) = &command.before_order_id {
            requested
                .iter()
                .position(|candidate| candidate == id)
                .ok_or(ProductionMapError::QueueReorderConflict)?
        } else if let Some(id) = &command.after_order_id {
            requested
                .iter()
                .position(|candidate| candidate == id)
                .ok_or(ProductionMapError::QueueReorderConflict)?
                + 1
        } else {
            requested.len()
        };
        requested.insert(target, command.order_id.clone());
        let order_ids = service_queue_support::nearest_allowed_sequence(
            &self.order_ids,
            &requested,
            &command.order_id,
            &self.states,
            &self.frozen,
        )
        .map_err(|_| ProductionMapError::QueueReorderBlocked)?;
        let adjusted = order_ids != requested;
        let event = compute_canonical_sequence_delta(&self.order_ids, &order_ids, &command.order_id);
        let mut next = self.clone();
        next.order_ids = order_ids.clone();
        Ok(SequenceMoveResult {
            order_ids,
            version: next.version(),
            adjusted,
            revision: None,
            event,
        })
    }
}

impl ProductionMapService {
    pub async fn move_apparatus_sequence(
        &self,
        command: SequenceMove,
        actor: QueueActionActor,
    ) -> Result<SequenceMoveResult, ProductionMapError> {
        command.validate()?;
        let _guard = self.queue_action_guard().await;
        let canonical = self
            .resolve_canonical_apparatus_text(&command.apparatus)
            .await?;
        let result = self
            .store
            .move_apparatus_sequence(&canonical, &command, &actor)
            .await?;
        if let (Some(rev), Some(op)) = (result.revision, &result.event) {
            let base_rev = (rev - 1).max(0);
            self.notify_live_delta(ProductionMapLiveDelta {
                epoch: self.snapshot_epoch().to_string(),
                apparatus: canonical.runtime.apparatus_id.to_string(),
                base_revision: base_rev,
                revision: rev,
                ops: vec![op.clone()],
                version: result.version.clone(),
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod canonical_delta_tests {
    use super::*;

    fn apply_client_delta(initial: &[String], op: &serde_json::Value) -> Vec<String> {
        let mut result = initial.to_vec();
        let id = op.get("id").and_then(|v| v.as_str()).unwrap();
        result.retain(|x| x != id);
        let after_id = op.get("after_id").and_then(|v| v.as_str());
        let before_id = op.get("before_id").and_then(|v| v.as_str());
        if let Some(after) = after_id {
            if let Some(idx) = result.iter().position(|x| x == after) {
                result.insert(idx + 1, id.to_string());
                return result;
            }
        }
        if let Some(before) = before_id {
            if let Some(idx) = result.iter().position(|x| x == before) {
                result.insert(idx, id.to_string());
                return result;
            }
        }
        result.insert(0, id.to_string());
        result
    }

    #[test]
    fn canonical_delta_invariant_apply_matches_new_sequence() {
        let old_seq = vec!["A".to_string(), "B".to_string(), "C".to_string(), "D".to_string()];

        // Move D to head
        let new_seq = vec!["D".to_string(), "A".to_string(), "B".to_string(), "C".to_string()];
        let delta = compute_canonical_sequence_delta(&old_seq, &new_seq, "D").unwrap();
        assert_eq!(apply_client_delta(&old_seq, &delta), new_seq);

        // Move A to tail
        let new_seq = vec!["B".to_string(), "C".to_string(), "D".to_string(), "A".to_string()];
        let delta = compute_canonical_sequence_delta(&old_seq, &new_seq, "A").unwrap();
        assert_eq!(apply_client_delta(&old_seq, &delta), new_seq);

        // Move D between A and B
        let new_seq = vec!["A".to_string(), "D".to_string(), "B".to_string(), "C".to_string()];
        let delta = compute_canonical_sequence_delta(&old_seq, &new_seq, "D").unwrap();
        assert_eq!(apply_client_delta(&old_seq, &delta), new_seq);

        // No change
        assert!(compute_canonical_sequence_delta(&old_seq, &old_seq, "A").is_none());
    }

    #[test]
    fn canonical_delta_when_clamped_by_active_order_never_violates_barrier() {
        let old_seq = vec![
            "zakaz-0003".to_string(),
            "zakaz-0018".to_string(),
            "zakaz-0020".to_string(),
        ];
        let clamped_new = vec![
            "zakaz-0003".to_string(),
            "zakaz-0020".to_string(),
            "zakaz-0018".to_string(),
        ];
        let delta = compute_canonical_sequence_delta(&old_seq, &clamped_new, "zakaz-0020").unwrap();

        assert_eq!(delta["after_id"], "zakaz-0003");
        assert_eq!(delta["before_id"], "zakaz-0018");
        let applied = apply_client_delta(&old_seq, &delta);
        assert_eq!(applied, clamped_new);
        assert_eq!(applied[0], "zakaz-0003");

        let delta_none = compute_canonical_sequence_delta(&old_seq, &old_seq, "zakaz-0018");
        assert!(delta_none.is_none());
    }
}
