use std::collections::{BTreeMap, BTreeSet};

use super::types::{ProductionMapDefinition, ProductionMapNodeKind};
use super::{chain, queue_state};
use crate::core::apparatus_standard::{
    ApparatusId, EquipmentCapabilityCode, ExecutionOperation, RuntimeApparatusConfiguration,
};

pub(super) fn visible_order_ids_for_apparatus(
    maps: &[ProductionMapDefinition],
    apparatus: &str,
) -> Vec<String> {
    let Some(apparatus_id) = ApparatusId::new(apparatus.trim().to_string()).ok() else {
        return Vec::new();
    };
    let apparatus = apparatus_id.as_str();
    maps.iter()
        .filter(|map| {
            !is_template_map(map) && chain::map_has_work_stage_for_station(map, apparatus)
        })
        .map(|map| map.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

pub(super) fn visible_order_ids_by_apparatus(
    maps: &[ProductionMapDefinition],
) -> BTreeMap<String, Vec<String>> {
    let mut visible = BTreeMap::<String, Vec<String>>::new();
    for map in maps {
        let order_id = map.id.trim();
        if !is_visible_queue_order(map) {
            continue;
        }
        let mut seen_apparatus_ids = BTreeSet::<String>::new();
        for stage in chain::linear_work_stages(map) {
            let Some(apparatus_id) = stage.apparatus_id.as_deref() else {
                continue;
            };
            let Some(apparatus_id) = ApparatusId::new(apparatus_id.trim().to_string()).ok() else {
                continue;
            };
            if !seen_apparatus_ids.insert(apparatus_id.to_string()) {
                continue;
            }
            visible
                .entry(apparatus_id.to_string())
                .or_default()
                .push(order_id.to_string());
        }
    }
    visible
}

fn is_visible_queue_order(map: &ProductionMapDefinition) -> bool {
    let order_id = map.id.trim();
    if order_id.is_empty() || is_template_map(map) {
        return false;
    }
    !map.code.trim().is_empty()
        || !map.order_number.trim().is_empty()
        || order_id.starts_with("zakaz-")
}

fn is_template_map(map: &ProductionMapDefinition) -> bool {
    map.id.trim().starts_with("template-")
}

/// Queue-owned stage-specific helpers cannot classify an opaque ID without a
/// canonical apparatus lookup. They fail closed for operations that require a
/// specific family; callers use these only as conservative guards.
pub(super) fn is_laminatsiya_apparatus(apparatus: &RuntimeApparatusConfiguration) -> bool {
    apparatus.is_active()
        && apparatus.runtime.execution_profile.operation == ExecutionOperation::Laminate
        && apparatus.supports(EquipmentCapabilityCode::Laminate)
}

pub(super) fn is_rezka_apparatus(apparatus: &RuntimeApparatusConfiguration) -> bool {
    apparatus.is_active()
        && apparatus.runtime.execution_profile.operation == ExecutionOperation::Cut
        && apparatus.supports(EquipmentCapabilityCode::Cut)
}

pub(super) fn requires_qolip_scan(apparatus: &RuntimeApparatusConfiguration) -> bool {
    crate::core::production_map::pechat::requires_qolip_scan(apparatus)
}

#[cfg(test)]
fn unassigned_alternative_candidate_groups(
    map: &ProductionMapDefinition,
    from: &str,
    to: &str,
) -> BTreeSet<String> {
    let candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && node.alternative_assigned_apparatus_id.trim().is_empty()
                && queue_state::apparatus_ids_match(&node.apparatus_id, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    candidate_groups
        .into_iter()
        .filter(|group_id| {
            let group_nodes = map.nodes.iter().filter(|node| {
                node.kind == ProductionMapNodeKind::Apparatus
                    && node.alternative_group_id.trim() == group_id
            });
            let mut has_target = false;
            let mut all_unassigned = true;
            for node in group_nodes {
                if !node.alternative_assigned_apparatus_id.trim().is_empty() {
                    all_unassigned = false;
                }
                if queue_state::apparatus_ids_match(&node.apparatus_id, to) {
                    has_target = true;
                }
            }
            all_unassigned && has_target
        })
        .collect()
}

#[cfg(test)]
pub(super) fn reassign_alternative_apparatus_assignment(
    map: &mut ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    let to = to.trim();
    if to.is_empty() {
        return false;
    }
    let mut candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && queue_state::apparatus_ids_match(&node.alternative_assigned_apparatus_id, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    if candidate_groups.is_empty() {
        candidate_groups = unassigned_alternative_candidate_groups(map, from, to);
    }
    if candidate_groups.is_empty() {
        return false;
    }
    let target_title = map
        .nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && candidate_groups.contains(node.alternative_group_id.trim())
                && queue_state::apparatus_ids_match(&node.apparatus_id, to)
        })
        .map(|node| node.title.trim().to_string());
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_apparatus_id = to.to_string();
            if let Some(title) = &target_title {
                node.alternative_assigned_title = title.clone();
            }
            changed = true;
        }
    }
    changed
}

pub(super) fn claim_unassigned_alternative_apparatus_assignment(
    map: &mut ProductionMapDefinition,
    apparatus: &str,
) -> bool {
    let apparatus = apparatus.trim();
    if apparatus.is_empty() {
        return false;
    }
    let candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && node.alternative_assigned_apparatus_id.trim().is_empty()
                && queue_state::apparatus_ids_match(&node.apparatus_id, apparatus)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .filter(|group_id| {
            map.nodes.iter().all(|node| {
                node.kind != ProductionMapNodeKind::Apparatus
                    || node.alternative_group_id.trim() != group_id
                    || node.alternative_assigned_apparatus_id.trim().is_empty()
            })
        })
        .collect();
    if candidate_groups.is_empty() {
        return false;
    }
    let target_title = map
        .nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && candidate_groups.contains(node.alternative_group_id.trim())
                && queue_state::apparatus_ids_match(&node.apparatus_id, apparatus)
        })
        .map(|node| node.title.trim().to_string());
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_apparatus_id = apparatus.to_string();
            if let Some(title) = &target_title {
                node.alternative_assigned_title = title.clone();
            }
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_map(id: &str, code: &str, order_number: &str) -> ProductionMapDefinition {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "product_code": "TEST-PRODUCT",
            "title": "Test order",
            "code": code,
            "order_number": order_number,
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {
                    "id": "apparatus",
                    "kind": "apparatus",
                    "title": "Renamed display title",
                    "apparatus_id": "apparatus:catalog:press-008"
                },
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "apparatus"},
                {"from": "apparatus", "to": "end"}
            ]
        }))
        .expect("queue map fixture")
    }

    #[test]
    fn queue_execution_ignores_template_maps_for_real_orders() {
        let maps = vec![
            queue_map("template-zakaz-1234", "", ""),
            queue_map("zakaz-1234", "1234", "1234"),
        ];

        assert_eq!(
            visible_order_ids_for_apparatus(&maps, "apparatus:catalog:press-008"),
            vec!["zakaz-1234".to_string()]
        );
    }

    #[test]
    fn title_snapshot_does_not_identify_queue_apparatus() {
        let maps = vec![queue_map("zakaz-1234", "1234", "1234")];
        assert!(visible_order_ids_for_apparatus(&maps, "Renamed display title").is_empty());
    }

    #[test]
    fn alternative_assignment_updates_id_and_keeps_display_snapshot() {
        let mut map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": "zakaz-1234",
            "product_code": "TEST-PRODUCT",
            "title": "Test order",
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {
                    "id": "source",
                    "kind": "apparatus",
                    "title": "Source display",
                    "apparatus_id": "apparatus:catalog:press-007",
                    "alternative_group_id": "print-group"
                },
                {
                    "id": "target",
                    "kind": "apparatus",
                    "title": "Target display",
                    "apparatus_id": "apparatus:catalog:press-008",
                    "alternative_group_id": "print-group"
                },
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "source"},
                {"from": "source", "to": "end"}
            ]
        }))
        .expect("alternative queue map fixture");

        assert!(reassign_alternative_apparatus_assignment(
            &mut map,
            "apparatus:catalog:press-007",
            "apparatus:catalog:press-008"
        ));
        let source = map
            .nodes
            .iter()
            .find(|node| node.id == "source")
            .expect("source node");
        assert_eq!(
            source.alternative_assigned_apparatus_id,
            "apparatus:catalog:press-008"
        );
        assert_eq!(source.alternative_assigned_title, "Target display");
    }
}
