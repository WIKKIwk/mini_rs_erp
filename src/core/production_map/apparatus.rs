use std::collections::{BTreeMap, BTreeSet};

use super::types::{ProductionMapDefinition, ProductionMapNodeKind};
use super::{chain, pechat, queue_state};

pub(super) fn visible_order_ids_for_apparatus(
    maps: &[ProductionMapDefinition],
    apparatus: &str,
) -> Vec<String> {
    maps.iter()
        .filter(|map| {
            !flexo_order_blocked_for_color_pechat(map, apparatus)
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
    if order_id.is_empty() || order_id.starts_with("template-") {
        return false;
    }
    !map.code.trim().is_empty()
        || !map.order_number.trim().is_empty()
        || order_id.starts_with("zakaz-")
}

pub(super) fn move_allowed(map: &ProductionMapDefinition, from: &str, to: &str) -> bool {
    let from_is_laminatsiya = is_laminatsiya_title(from);
    let to_is_laminatsiya = is_laminatsiya_title(to);
    if from_is_laminatsiya || to_is_laminatsiya {
        return from_is_laminatsiya
            && to_is_laminatsiya
            && alternative_assigned_group_contains_target(map, from, to);
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
