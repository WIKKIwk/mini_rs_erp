use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::core::apparatus_standard::{ApparatusId, RuntimeApparatusConfiguration};
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapNode, ProductionMapNodeKind,
};

/// Manual graph edits can connect interchangeable machines without supplying
/// alternative metadata. Persist that meaning once, at the validated save
/// boundary, so queue controls, WIP scans and shared completion use one group.
/// Existing explicit groups/assignments are authoritative and are not merged.
pub(super) fn normalize_topology_alternatives(
    map: &mut ProductionMapDefinition,
    configurations: &BTreeMap<ApparatusId, Arc<RuntimeApparatusConfiguration>>,
) {
    let mut candidates = map
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            node.kind == ProductionMapNodeKind::Apparatus
                && node.alternative_group_id.is_empty()
                && node.alternative_assigned_apparatus_id.is_empty()
        })
        .filter_map(|(index, node)| {
            let canonical = configurations.get(&node.base_apparatus_id()?)?;
            let incoming = map
                .edges
                .iter()
                .filter(|edge| edge.to == node.id)
                .map(|edge| (edge.from.clone(), edge.branch.clone()))
                .collect::<BTreeSet<_>>();
            let outgoing = map
                .edges
                .iter()
                .filter(|edge| edge.from == node.id)
                .map(|edge| (edge.to.clone(), edge.branch.clone()))
                .collect::<BTreeSet<_>>();
            // Disconnected nodes do not prove a shared production occurrence.
            (!incoming.is_empty() && !outgoing.is_empty())
                .then_some((index, canonical, incoming, outgoing))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| map.nodes[a.0].id.cmp(&map.nodes[b.0].id));

    let mut used = BTreeSet::<usize>::new();
    let mut groups = Vec::new();
    for (position, (index, canonical, incoming, outgoing)) in candidates.iter().enumerate() {
        if used.contains(index) {
            continue;
        }
        let mut members = vec![*index];
        let mut apparatuses = BTreeSet::from([canonical.runtime.apparatus_id.clone()]);
        for (peer, other, peer_incoming, peer_outgoing) in candidates.iter().skip(position + 1) {
            if !used.contains(peer)
                && incoming == peer_incoming
                && outgoing == peer_outgoing
                && canonical.runtime.equipment_class_id == other.runtime.equipment_class_id
                && canonical.runtime.execution_profile == other.runtime.execution_profile
                && canonical.runtime.capabilities == other.runtime.capabilities
                && same_work_parameters(&map.nodes[*index], &map.nodes[*peer])
                && apparatuses.insert(other.runtime.apparatus_id.clone())
            {
                members.push(*peer);
            }
        }
        if members.len() > 1 {
            used.extend(members.iter().copied());
            groups.push(members);
        }
    }

    let mut occupied = map
        .nodes
        .iter()
        .map(|node| node.alternative_group_id.clone())
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>();
    for members in groups {
        // Node identity, not edge ordering or a mutable display name, makes the
        // generated group stable. IDs are scoped to this map, as explicit IDs are.
        let base = format!("topology_alt:{}", map.nodes[members[0]].id);
        let mut group = base.clone();
        let mut suffix = 1;
        while !occupied.insert(group.clone()) {
            group = format!("{base}:{suffix}");
            suffix += 1;
        }
        for index in members {
            map.nodes[index].alternative_group_id = group.clone();
        }
    }
}

fn same_work_parameters(left: &ProductionMapNode, right: &ProductionMapNode) -> bool {
    left.formula == right.formula
        && left.item_code == right.item_code
        && left.qty_formula == right.qty_formula
        && left.from_location == right.from_location
        && left.to_location == right.to_location
        && left.rezka_kadr_count == right.rezka_kadr_count
        && left.rezka_frame_groups == right.rezka_frame_groups
        && left.rezka_label_length == right.rezka_label_length
}
