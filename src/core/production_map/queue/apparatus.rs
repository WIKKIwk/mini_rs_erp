use std::collections::{BTreeMap, BTreeSet};

use super::types::{ProductionMapDefinition, ProductionMapNodeKind};
use super::{chain, pechat, queue_state};

pub(super) fn visible_order_ids_for_apparatus(
    maps: &[ProductionMapDefinition],
    apparatus: &str,
) -> Vec<String> {
    maps.iter()
        .filter(|map| {
            !is_template_map(map)
                && !flexo_order_blocked_for_color_pechat(map, apparatus)
                && chain::map_has_work_stage_for_station(map, apparatus)
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
        let mut seen_titles = BTreeSet::<String>::new();
        for stage in chain::linear_work_stages(map) {
            let title = stage.station_title.trim();
            if title.is_empty()
                || flexo_order_blocked_for_color_pechat(map, title)
                || !seen_titles.insert(title.to_ascii_lowercase())
            {
                continue;
            }
            visible
                .entry(title.to_string())
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

pub(super) fn move_allowed(map: &ProductionMapDefinition, from: &str, to: &str) -> bool {
    let from_is_laminatsiya = is_laminatsiya_title(from);
    let to_is_laminatsiya = is_laminatsiya_title(to);
    if from_is_laminatsiya || to_is_laminatsiya {
        return from_is_laminatsiya
            && to_is_laminatsiya
            && (alternative_assigned_group_contains_target(map, from, to)
                || unassigned_alternative_group_contains_target(map, from, to));
    }
    if has_unassigned_alternative_candidate(map, from)
        && !unassigned_alternative_group_contains_target(map, from, to)
    {
        return false;
    }

    // A queue move is a change of the work-center assignment, not a way to
    // turn one operation into another. Keep known apparatus families
    // compatible before applying the more specific pechat rules below. This
    // also prevents a printing order from being silently moved to rezka or a
    // packaging station merely because its title has no colour count.
    if let (Some(from_family), Some(to_family)) =
        (known_apparatus_family(from), known_apparatus_family(to))
        && from_family != to_family
    {
        return false;
    }

    let from_is_flexo = pechat::is_flexo_apparatus(from);
    let to_is_flexo = pechat::is_flexo_apparatus(to);
    if from_is_flexo != to_is_flexo {
        return false;
    }
    if (from_is_flexo || to_is_flexo) && !is_flexo_order(map) {
        return false;
    }
    let Some(target_color) = pechat::pechat_color_count(to) else {
        return true;
    };
    if is_flexo_order(map) {
        return false;
    }
    let source_color = pechat::pechat_color_count(from).or_else(|| {
        pechat::order_pechat_color_count(
            map.nodes
                .iter()
                .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
                .map(|node| node.title.as_str()),
        )
    });
    pechat::pechat_can_move_order(target_color, map.roll_count, map.width_mm, source_color)
}

fn flexo_order_blocked_for_color_pechat(map: &ProductionMapDefinition, apparatus: &str) -> bool {
    is_flexo_order(map) && pechat::pechat_color_count(apparatus).is_some()
}

fn is_flexo_order(map: &ProductionMapDefinition) -> bool {
    let mut haystack = format!("{} {} {}", map.title, map.product_code, map.code).to_lowercase();
    for node in &map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus {
            if pechat::is_flexo_apparatus(&node.title) {
                return true;
            }
            continue;
        }
        haystack.push(' ');
        haystack.push_str(&node.title.to_lowercase());
        haystack.push(' ');
        haystack.push_str(&node.item_code.to_lowercase());
    }
    ["fleksa", "fleska", "flex", "flexe", "flexo"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
}

fn known_apparatus_family(title: &str) -> Option<&'static str> {
    let normalized = title.trim().to_ascii_lowercase();
    if pechat::is_pechat_apparatus(&normalized) {
        return Some("pechat");
    }
    if normalized.contains("laminatsiya") {
        return Some("laminatsiya");
    }
    if normalized.contains("rezka") {
        return Some("rezka");
    }
    if normalized.contains("paket") {
        return Some("paket");
    }
    if normalized.contains("kley") {
        return Some("kley");
    }
    None
}

pub(super) fn is_laminatsiya_title(title: &str) -> bool {
    title.trim().to_lowercase().contains("laminatsiya")
}

pub(super) fn is_rezka_title(title: &str) -> bool {
    title.trim().to_lowercase().contains("rezka")
}

fn alternative_assigned_group_contains_target(
    map: &ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    let candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && queue_state::apparatus_titles_match(&node.alternative_assigned_title, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    if candidate_groups.is_empty() {
        return true;
    }
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
            && queue_state::apparatus_titles_match(&node.title, to)
    })
}

fn has_unassigned_alternative_candidate(
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> bool {
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && !node.alternative_group_id.trim().is_empty()
            && node.alternative_assigned_title.trim().is_empty()
            && queue_state::apparatus_titles_match(&node.title, apparatus)
    })
}

fn unassigned_alternative_group_contains_target(
    map: &ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    !unassigned_alternative_candidate_groups(map, from, to).is_empty()
}

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
                && node.alternative_assigned_title.trim().is_empty()
                && queue_state::apparatus_titles_match(&node.title, from)
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
                if !node.alternative_assigned_title.trim().is_empty() {
                    all_unassigned = false;
                }
                if queue_state::apparatus_titles_match(&node.title, to) {
                    has_target = true;
                }
            }
            all_unassigned && has_target
        })
        .collect()
}

pub(super) fn reassign_apparatus_nodes(
    map: &mut ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    let to = to.trim();
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && queue_state::apparatus_titles_match(&node.title, from)
        {
            node.title = to.to_string();
            changed = true;
        }
    }
    changed
}

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
                && queue_state::apparatus_titles_match(&node.alternative_assigned_title, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    if candidate_groups.is_empty() {
        candidate_groups = unassigned_alternative_candidate_groups(map, from, to);
    }
    if candidate_groups.is_empty() {
        return false;
    }
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_title = to.to_string();
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
                && node.alternative_assigned_title.trim().is_empty()
                && queue_state::apparatus_titles_match(&node.title, apparatus)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .filter(|group_id| {
            map.nodes.iter().all(|node| {
                node.kind != ProductionMapNodeKind::Apparatus
                    || node.alternative_group_id.trim() != group_id
                    || node.alternative_assigned_title.trim().is_empty()
            })
        })
        .collect();
    if candidate_groups.is_empty() {
        return false;
    }
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_title = apparatus.to_string();
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
                    "title": "8 ta rangli pechat"
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
            visible_order_ids_for_apparatus(&maps, "8 ta rangli bosma aparat"),
            vec!["zakaz-1234".to_string()]
        );
    }
}
