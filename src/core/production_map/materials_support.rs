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
        item_groups,
        requirement_groups,
    })
}

pub(super) fn rule_matches(
    rule: &ApparatusMaterialRule,
    apparatus: &str,
    item_group_path: &[String],
) -> bool {
    queue_state::apparatus_titles_match(&rule.apparatus, apparatus)
        && (item_groups_match(&rule.item_groups, item_group_path)
            || rule
                .requirement_groups
                .iter()
                .any(|group| item_groups_match(&group.item_groups, item_group_path)))
}

pub(super) fn material_requirements_met(
    rule: &ApparatusMaterialRule,
    assignments: &[RawMaterialAssignment],
) -> bool {
    effective_requirement_groups(rule).into_iter().all(|group| {
        let required_count = group.min_required_count.max(1);
        let matched_count = assignments
            .iter()
            .filter(|assignment| {
                group
                    .item_groups
                    .iter()
                    .any(|item_group| item_group.eq_ignore_ascii_case(assignment.item_group.trim()))
            })
            .count();
        matched_count >= required_count
    })
}

fn effective_requirement_groups(
    rule: &ApparatusMaterialRule,
) -> Vec<ApparatusMaterialRequirementGroup> {
    if !rule.requirement_groups.is_empty() {
        return rule.requirement_groups.clone();
    }
    vec![ApparatusMaterialRequirementGroup {
        name: String::new(),
        item_groups: rule.item_groups.clone(),
        min_required_count: 1,
    }]
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
