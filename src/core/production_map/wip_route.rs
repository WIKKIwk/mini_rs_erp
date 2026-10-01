//! Effective input routing is a read-only projection. Printed QR identity and
//! the producing batch's original node IDs remain historical facts.
use super::{
    OrderProgressBatch, OrderProgressBatchWipStatus, ProductionMapDefinition, ProductionMapError,
    ProductionMapNode, ProductionMapNodeKind, chain,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WipInputRoute {
    pub source_stage_node_id: String,
    pub stage_node_id: String,
    pub consumer_apparatus_ids: Vec<String>,
    pub map_fingerprint: String,
    pub remapped: bool,
}

pub(crate) fn production_map_fingerprint(map: &ProductionMapDefinition) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(map).expect("serializable map"))
    )
}

pub(crate) fn normalized_route_batch(batch: &OrderProgressBatch) -> OrderProgressBatch {
    let mut batch = batch.clone();
    super::service_progress_support::restore_self_consumed_wip(&mut batch);
    batch.refresh_status_detail();
    batch
}

pub(crate) fn actionable_wip(batch: &OrderProgressBatch) -> bool {
    batch.wip_status == OrderProgressBatchWipStatus::Waiting
        && batch.used_by_apparatus.trim().is_empty()
        && batch.used_by_session_id.trim().is_empty()
        && batch.processed_by_apparatus.trim().is_empty()
        && batch.processed_by_session_id.trim().is_empty()
        && batch.action.records_progress_output()
}

fn field<'a>(batch: &'a OrderProgressBatch, key: &str) -> &'a str {
    batch
        .payload_json
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
}

fn node<'a>(map: &'a ProductionMapDefinition, id: &str) -> Option<&'a ProductionMapNode> {
    map.nodes
        .iter()
        .find(|n| n.id.trim() == id && n.kind == ProductionMapNodeKind::Apparatus)
}

fn occurrence(node: &ProductionMapNode) -> String {
    if node.alternative_group_id.trim().is_empty() {
        format!("node:{}", node.id.trim())
    } else {
        format!("group:{}", node.alternative_group_id.trim())
    }
}

fn stage_apparatus<'a>(stages: &'a [chain::ChainStage], node_id: &str) -> Option<&'a str> {
    stages
        .iter()
        .find(|stage| stage.node_id.trim() == node_id.trim())
        .and_then(|stage| stage.apparatus_id.as_deref())
}

pub(crate) fn resolve_wip_input_route(
    map: &ProductionMapDefinition,
    batch: &OrderProgressBatch,
) -> Result<WipInputRoute, ProductionMapError> {
    use ProductionMapError::*;
    if batch.order_id.trim() != map.id.trim() || !batch.action.records_progress_output() {
        return Err(ProgressBatchNotAccepted);
    }
    let binding = batch
        .payload_json
        .get("wip_route_binding")
        .map(|value| {
            serde_json::from_value::<WipInputRoute>(value.clone())
                .map_err(|_| WipRouteDestinationUnresolved)
        })
        .transpose()?;
    let target_id = binding
        .as_ref()
        .map(|r| r.stage_node_id.as_str())
        .unwrap_or_else(|| field(batch, "next_stage_node_id"));
    let source_id = field(batch, "stage_node_id");
    let stages = chain::linear_work_stages(map);
    // Older records may have only canonical producer/consumer IDs. A unique
    // existing pair identifies their route even after consumption; it never
    // licenses recovery of a supplied but deleted concrete destination.
    let legacy_canonical_route = binding.is_none()
        && source_id.is_empty()
        && target_id.is_empty()
        && !batch.next_apparatus.trim().is_empty();
    if legacy_canonical_route
        && stages
            .iter()
            .filter(|stage| {
                stage
                    .apparatus_id
                    .as_deref()
                    .is_some_and(|id| super::types::apparatus_ids_match(id, &batch.next_apparatus))
            })
            .count()
            != 1
    {
        return Err(WipRouteAmbiguous);
    }
    let source = if !source_id.is_empty() {
        let source = node(map, source_id).ok_or(WipRouteSourceUnresolved)?;
        if !stage_apparatus(&stages, source_id)
            .is_some_and(|id| super::types::apparatus_ids_match(id, &batch.apparatus))
        {
            return Err(WipRouteSourceUnresolved);
        }
        source
    } else {
        // Legacy records with a valid explicit destination can use its unique
        // canonical predecessor. A deleted destination never licenses guessing
        // a missing historical producer from a machine title or type.
        if !target_id.is_empty() && node(map, target_id).is_none() {
            return Err(WipRouteSourceUnresolved);
        }
        if target_id.is_empty() && batch.next_apparatus.trim().is_empty() {
            return Err(WipRouteSourceUnresolved);
        }
        let possible_sources = if target_id.is_empty() {
            stages.clone()
        } else {
            chain::previous_work_stages_for_node(map, target_id)
        };
        let sources: BTreeSet<_> = possible_sources
            .into_iter()
            .filter(|s| {
                s.apparatus_id
                    .as_deref()
                    .is_some_and(|id| super::types::apparatus_ids_match(id, &batch.apparatus))
            })
            .map(|s| s.node_id)
            .collect();
        if sources.len() != 1 {
            return Err(WipRouteSourceUnresolved);
        }
        node(map, sources.first().expect("one source")).ok_or(WipRouteSourceUnresolved)?
    };
    if binding
        .as_ref()
        .is_some_and(|r| r.source_stage_node_id != source.id)
    {
        return Err(WipRouteSourceUnresolved);
    }
    let successors = chain::next_work_stages_for_node(map, &source.id);
    let mut groups: BTreeMap<String, Vec<&ProductionMapNode>> = BTreeMap::new();
    for stage in &successors {
        if let Some(next) = node(map, &stage.node_id) {
            // Maps may draw only representative edges for an alternative
            // group. The concrete peers are still the same logical successor.
            let candidates = groups.entry(occurrence(next)).or_default();
            for peer in &map.nodes {
                if chain::stage_node_ids_match_for_map(map, &next.id, &peer.id)
                    && stage_apparatus(&stages, &peer.id).is_some()
                    && !candidates.iter().any(|candidate| candidate.id == peer.id)
                {
                    candidates.push(peer);
                }
            }
        }
    }
    let (mut target, remapped) = if !target_id.is_empty() && node(map, target_id).is_some() {
        let target = node(map, target_id).expect("existing target");
        if !groups.contains_key(&occurrence(target)) {
            return Err(WipRouteDestinationUnresolved);
        }
        (target, binding.as_ref().is_some_and(|r| r.remapped))
    } else {
        if binding.is_some() || (!actionable_wip(batch) && !legacy_canonical_route) {
            return Err(WipRouteDestinationUnresolved);
        }
        if groups.len() > 1 {
            return Err(WipRouteAmbiguous);
        }
        let candidates = groups
            .values()
            .next()
            .ok_or(WipRouteDestinationUnresolved)?;
        // A stale explicit destination also needs its original canonical
        // apparatus to anchor the replacement occurrence within this group.
        if !target_id.is_empty() && batch.next_apparatus.trim().is_empty() {
            return Err(WipRouteDestinationUnresolved);
        }
        let matching: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|n| {
                batch.next_apparatus.trim().is_empty()
                    || stage_apparatus(&stages, &n.id).is_some_and(|id| {
                        super::types::apparatus_ids_match(id, &batch.next_apparatus)
                    })
            })
            .collect();
        let target = if batch.next_apparatus.trim().is_empty() {
            // Historical final-stage WIP with no explicit route follows a sole
            // newly appended logical stage; all its alternatives remain peers.
            candidates
                .first()
                .copied()
                .ok_or(WipRouteDestinationUnresolved)?
        } else {
            if matching.len() > 1 {
                return Err(WipRouteAmbiguous);
            }
            matching
                .first()
                .copied()
                .ok_or(WipRouteDestinationUnresolved)?
        };
        (target, !legacy_canonical_route)
    };
    let candidates = groups
        .get(&occurrence(target))
        .ok_or(WipRouteDestinationUnresolved)?;
    if binding.is_none() && batch.wip_status == OrderProgressBatchWipStatus::InUse {
        // Sessions created before route pins existed still own a concrete
        // consumer. Resolve that peer only inside the proven destination
        // occurrence; retaining the original representative cannot license
        // deleting the machine that is actually processing the roll.
        let used_by = if batch.used_by_apparatus.trim().is_empty() {
            batch.current_apparatus.trim()
        } else {
            batch.used_by_apparatus.trim()
        };
        let owned: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|candidate| {
                stage_apparatus(&stages, &candidate.id)
                    .is_some_and(|id| super::types::apparatus_ids_match(id, used_by))
            })
            .collect();
        if owned.len() > 1 {
            return Err(WipRouteAmbiguous);
        }
        target = owned
            .first()
            .copied()
            .ok_or(WipRouteDestinationUnresolved)?;
    }
    if binding.is_some()
        && !batch.used_by_apparatus.trim().is_empty()
        && !stage_apparatus(&stages, &target.id)
            .is_some_and(|id| super::types::apparatus_ids_match(id, &batch.used_by_apparatus))
    {
        return Err(WipRouteDestinationUnresolved);
    }
    if binding.is_none()
        && !batch.next_apparatus.trim().is_empty()
        && !candidates.iter().any(|n| {
            stage_apparatus(&stages, &n.id)
                .is_some_and(|id| super::types::apparatus_ids_match(id, &batch.next_apparatus))
        })
    {
        return Err(WipRouteDestinationUnresolved);
    }
    let consumer_apparatus_ids: Vec<_> = candidates
        .iter()
        .filter_map(|n| stage_apparatus(&stages, &n.id).map(str::to_string))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(WipInputRoute {
        source_stage_node_id: source.id.clone(),
        stage_node_id: target.id.clone(),
        consumer_apparatus_ids,
        map_fingerprint: production_map_fingerprint(map),
        remapped,
    })
}

/// Protect usable outstanding rolls, while permitting unrelated edits on maps
/// that already contain historical unresolved records. No batch is rewritten.
pub(crate) fn validate_map_wip_routes(
    previous: &ProductionMapDefinition,
    next: &ProductionMapDefinition,
    batches: &[OrderProgressBatch],
) -> Result<(), ProductionMapError> {
    for batch in batches {
        let batch = normalized_route_batch(batch);
        if !actionable_wip(&batch) && batch.wip_status != OrderProgressBatchWipStatus::InUse {
            continue;
        }
        if let Ok(previous_route) = resolve_wip_input_route(previous, &batch) {
            let next_route = resolve_wip_input_route(next, &batch)?;
            if batch.wip_status == OrderProgressBatchWipStatus::InUse {
                if previous_route.source_stage_node_id != next_route.source_stage_node_id {
                    return Err(ProductionMapError::WipRouteSourceUnresolved);
                }
                if previous_route.stage_node_id != next_route.stage_node_id {
                    return Err(ProductionMapError::WipRouteDestinationUnresolved);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn route_error_code(error: &ProductionMapError) -> &'static str {
    match error {
        ProductionMapError::WipRouteSourceUnresolved => "wip_route_source_unresolved",
        ProductionMapError::WipRouteDestinationUnresolved => "wip_route_destination_unresolved",
        ProductionMapError::WipRouteAmbiguous => "wip_route_ambiguous",
        ProductionMapError::WipRouteChanged => "wip_route_changed",
        _ => "progress_batch_not_accepted",
    }
}

#[cfg(test)]
#[path = "wip_route_tests.rs"]
mod tests;
