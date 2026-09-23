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
        let mut next = self.clone();
        next.order_ids = order_ids.clone();
        Ok(SequenceMoveResult {
            order_ids,
            version: next.version(),
            adjusted,
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
        self.notify_live();
        Ok(result)
    }
}
