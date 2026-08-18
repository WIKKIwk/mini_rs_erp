use std::collections::BTreeSet;

use super::materials::{
    ApparatusMaterialRequirementGroup, ApparatusMaterialRule, ApparatusMaterialRuleUpsert,
    RawMaterialAssignment,
};
use super::{ProductionMapError, queue_state};

pub(super) fn normalize_rule(
    input: ApparatusMaterialRuleUpsert,
) -> Result<ApparatusMaterialRule, ProductionMapError> {
    let apparatus = input.apparatus.trim().to_string();
    if apparatus.is_empty() {
        return Err(ProductionMapError::RawMaterialInvalidInput);
    }
    let item_groups = normalize_group_names(input.item_groups);
    let requirement_groups = normalize_requirement_groups(input.requirement_groups);
    if item_groups.is_empty() {
        return Err(ProductionMapError::RawMaterialInvalidInput);
    }
    Ok(ApparatusMaterialRule {
        apparatus,
        requires_material: input.requires_material,
        start_policy: input.start_policy,
        item_groups,
        requirement_groups,
    })
}

pub(super) fn rule_matches(
    rule: &ApparatusMaterialRule,
    apparatus: &str,
    item_group_path: &[String],
) -> bool {
    material_rule_apparatus_matches(&rule.apparatus, apparatus)
        && (item_groups_match(&rule.item_groups, item_group_path)
            || rule
                .requirement_groups
                .iter()
                .any(|group| item_groups_match(&group.item_groups, item_group_path)))
}

fn material_rule_apparatus_matches(rule_apparatus: &str, apparatus: &str) -> bool {
    let rule_apparatus = rule_apparatus.trim();
    let apparatus = apparatus.trim();
    if rule_apparatus.is_empty() || apparatus.is_empty() {
        return false;
    }
    if rule_apparatus.eq_ignore_ascii_case(apparatus) {
        return true;
    }

    let rule_base = queue_state::warehouse_base_title(rule_apparatus);
    let apparatus_base = queue_state::warehouse_base_title(apparatus);
    let rule_has_instance = !rule_base.eq_ignore_ascii_case(rule_apparatus);
    let apparatus_has_instance = !apparatus_base.eq_ignore_ascii_case(apparatus);
    if rule_has_instance && apparatus_has_instance {
        return false;
    }

    queue_state::apparatus_titles_match(rule_apparatus, apparatus)
}

pub(super) fn material_requirements_met(
    rule: &ApparatusMaterialRule,
    assignments: &[RawMaterialAssignment],
) -> bool {
    material_requirement_match_count(rule, assignments) == material_requirement_slot_count(rule)
}

pub(super) fn material_requirement_slot_count(rule: &ApparatusMaterialRule) -> usize {
    effective_requirement_groups(rule)
        .iter()
        .map(|group| group.min_required_count.max(1))
        .sum()
}

pub(super) fn material_requirement_match_count(
    rule: &ApparatusMaterialRule,
    assignments: &[RawMaterialAssignment],
) -> usize {
    let slots = effective_requirement_groups(rule)
        .into_iter()
        .flat_map(|group| {
            (0..group.min_required_count.max(1)).map(move |_| group.item_groups.clone())
        })
        .collect::<Vec<_>>();
    let mut matched_slots = vec![None; slots.len()];
    for assignment_index in 0..assignments.len() {
        let mut visited = vec![false; slots.len()];
        try_match_material_assignment(
            assignment_index,
            assignments,
            &slots,
            &mut matched_slots,
            &mut visited,
        );
    }
    matched_slots.iter().filter(|slot| slot.is_some()).count()
}

pub(super) fn effective_requirement_groups(
    rule: &ApparatusMaterialRule,
) -> Vec<ApparatusMaterialRequirementGroup> {
    if !rule.requirement_groups.is_empty() {
        return rule.requirement_groups.clone();
    }
    rule.item_groups
        .iter()
        .map(|item_group| ApparatusMaterialRequirementGroup {
            name: item_group.clone(),
            item_groups: vec![item_group.clone()],
            min_required_count: 1,
        })
        .collect()
}

fn try_match_material_assignment(
    assignment_index: usize,
    assignments: &[RawMaterialAssignment],
    slots: &[Vec<String>],
    matched_slots: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for (slot_index, item_groups) in slots.iter().enumerate() {
        if visited[slot_index]
            || !item_groups.iter().any(|item_group| {
                item_group.eq_ignore_ascii_case(assignments[assignment_index].item_group.trim())
            })
        {
            continue;
        }
        visited[slot_index] = true;
        if matched_slots[slot_index].is_none()
            || try_match_material_assignment(
                matched_slots[slot_index].unwrap_or_default(),
                assignments,
                slots,
                matched_slots,
                visited,
            )
        {
            matched_slots[slot_index] = Some(assignment_index);
            return true;
        }
    }
    false
}

fn item_groups_match(groups: &[String], item_group_path: &[String]) -> bool {
    groups.iter().any(|group| {
        item_group_path
            .iter()
            .any(|candidate| group.trim().eq_ignore_ascii_case(candidate.trim()))
    })
}

fn normalize_requirement_groups(
    groups: Vec<ApparatusMaterialRequirementGroup>,
) -> Vec<ApparatusMaterialRequirementGroup> {
    groups
        .into_iter()
        .filter_map(|group| {
            let item_groups = normalize_group_names(group.item_groups);
            if item_groups.is_empty() {
                return None;
            }
            Some(ApparatusMaterialRequirementGroup {
                name: group.name.trim().to_string(),
                item_groups,
                min_required_count: group.min_required_count.max(1),
            })
        })
        .collect()
}

fn normalize_group_names(groups: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    groups
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.to_lowercase()))
        .collect()
}

pub(super) fn default_min_required_count() -> usize {
    1
}

pub(super) fn normalize_group_path(item_group: &str, item_group_path: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    std::iter::once(item_group.to_string())
        .chain(item_group_path)
        .map(|group| group.trim().to_string())
        .filter(|group| !group.is_empty())
        .filter(|group| seen.insert(group.to_lowercase()))
        .collect()
}

pub(super) fn normalize_barcode(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

pub(super) fn normalized_barcodes(value: &str) -> BTreeSet<String> {
    value
        .split(',')
        .map(normalize_barcode)
        .filter(|item| !item.is_empty())
        .collect()
}

pub(super) fn same_barcode(left: &str, right: &str) -> bool {
    normalize_barcode(left) == normalize_barcode(right)
}

pub(super) fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
