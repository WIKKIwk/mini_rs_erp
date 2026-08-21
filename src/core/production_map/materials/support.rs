use std::collections::BTreeSet;

use crate::core::apparatus_standard::ApparatusId;

use super::ProductionMapError;
use super::materials::{
    ApparatusMaterialRequirementGroup, ApparatusMaterialRule, RawMaterialAssignment,
};
#[cfg(test)]
use super::materials::ApparatusMaterialRuleUpsert;

#[cfg(test)]
pub(super) fn normalize_rule(
    input: ApparatusMaterialRuleUpsert,
) -> Result<ApparatusMaterialRule, ProductionMapError> {
    let apparatus_id = parse_apparatus_id(&input.apparatus)?;
    let apparatus = apparatus_id.to_string();
    let item_groups = normalize_group_names(input.item_groups);
    let requirement_groups = normalize_requirement_groups(input.requirement_groups);
    let valid_policy = if !input.requires_material {
        input.start_policy == super::RawMaterialStartPolicy::StateAll
            && item_groups.is_empty()
            && requirement_groups.is_empty()
    } else {
        match input.start_policy {
            super::RawMaterialStartPolicy::StateAll => {
                !item_groups.is_empty() && requirement_groups.is_empty()
            }
            super::RawMaterialStartPolicy::RequirementGroups => {
                item_groups.is_empty() && !requirement_groups.is_empty()
            }
        }
    };
    if !valid_policy {
        return Err(ProductionMapError::RawMaterialInvalidInput);
    }
    Ok(ApparatusMaterialRule {
        apparatus_id,
        apparatus,
        requires_material: input.requires_material,
        start_policy: input.start_policy,
        item_groups,
        requirement_groups,
    })
}

pub(super) fn rule_matches(
    rule: &ApparatusMaterialRule,
    apparatus: &ApparatusId,
    item_group_path: &[String],
) -> bool {
    rule.apparatus_id.as_str() == apparatus.as_str()
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
    material_requirement_match_count(rule, assignments) == material_requirement_slot_count(rule)
}

pub(super) fn material_requirement_slot_count(rule: &ApparatusMaterialRule) -> usize {
    effective_requirement_groups(rule)
        .iter()
        .map(|group| usize::from(group.min_required_count.max(1)))
        .sum()
}

pub(super) fn material_requirement_match_count(
    rule: &ApparatusMaterialRule,
    assignments: &[RawMaterialAssignment],
) -> usize {
    let slots = effective_requirement_groups(rule)
        .into_iter()
        .flat_map(|group| {
            (0..usize::from(group.min_required_count.max(1)))
                .map(move |_| group.item_groups.clone())
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

#[cfg(test)]
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

#[cfg(test)]
fn normalize_group_names(groups: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    groups
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.to_lowercase()))
        .collect()
}

pub(super) fn parse_apparatus_id(value: &str) -> Result<ApparatusId, ProductionMapError> {
    ApparatusId::new(value.trim().to_string())
        .map_err(|_| ProductionMapError::RawMaterialInvalidInput)
}

pub(super) fn parse_optional_apparatus_id(
    value: &str,
) -> Result<Option<ApparatusId>, ProductionMapError> {
    if value.trim().is_empty() {
        Ok(None)
    } else {
        parse_apparatus_id(value).map(Some)
    }
}

pub(super) fn canonical_apparatuses_match(candidate: &str, expected: &ApparatusId) -> bool {
    parse_apparatus_id(candidate).is_ok_and(|candidate| candidate.as_str() == expected.as_str())
}

pub(super) fn assigned_apparatus_contains(expected: &ApparatusId, assigned: &[String]) -> bool {
    assigned
        .iter()
        .any(|candidate| canonical_apparatuses_match(candidate, expected))
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::core::production_map::RawMaterialStartPolicy;

    fn apparatus_id(value: &str) -> ApparatusId {
        ApparatusId::new(value).expect("valid apparatus id")
    }

    fn assignment(id: &str, barcode: &str, item_group: &str) -> RawMaterialAssignment {
        RawMaterialAssignment {
            order_id: "zakaz-test".to_string(),
            apparatus_id: apparatus_id(id),
            apparatus: id.to_string(),
            barcode: barcode.to_string(),
            item_code: item_group.to_string(),
            item_name: item_group.to_string(),
            item_group: item_group.to_string(),
            assigned_by_role: "admin".to_string(),
            assigned_by_ref: "admin".to_string(),
            assigned_by_display_name: "Admin".to_string(),
            assigned_at: "now".to_string(),
        }
    }

    #[test]
    fn apparatus_identity_is_exact_and_does_not_match_display_text() {
        let rule = ApparatusMaterialRule {
            apparatus_id: apparatus_id("apparatus:catalog:pechat-001"),
            apparatus: "7 ta rangli pechat - A".to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kraska".to_string()],
            requirement_groups: Vec::new(),
        };
        assert!(rule_matches(
            &rule,
            &apparatus_id("apparatus:catalog:pechat-001"),
            &["Kraska".to_string()]
        ));
        assert!(!rule_matches(
            &rule,
            &apparatus_id("apparatus:catalog:pechat-002"),
            &["Kraska".to_string()]
        ));
        assert!(!canonical_apparatuses_match(
            "7 ta rangli pechat - A",
            &rule.apparatus_id
        ));
    }

    #[test]
    fn requirement_groups_need_distinct_assignments_for_each_slot() {
        let rule = ApparatusMaterialRule {
            apparatus_id: apparatus_id("apparatus:catalog:flexo-001"),
            apparatus: "Flexo".to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::RequirementGroups,
            item_groups: Vec::new(),
            requirement_groups: vec![ApparatusMaterialRequirementGroup {
                name: "two inks".to_string(),
                item_groups: vec!["Kraska".to_string()],
                min_required_count: 2,
            }],
        };
        let one = vec![assignment("apparatus:catalog:flexo-001", "INK-1", "Kraska")];
        assert!(!material_requirements_met(&rule, &one));
        let two = vec![
            assignment("apparatus:catalog:flexo-001", "INK-1", "Kraska"),
            assignment("apparatus:catalog:flexo-001", "INK-2", "Kraska"),
        ];
        assert!(material_requirements_met(&rule, &two));
    }

    #[test]
    fn rule_normalization_rejects_mixed_canonical_policy_shapes() {
        let result = normalize_rule(ApparatusMaterialRuleUpsert {
            apparatus: "apparatus:catalog:flexo-001".to_string(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Kraska".to_string()],
            requirement_groups: vec![ApparatusMaterialRequirementGroup {
                name: "ink".to_string(),
                item_groups: vec!["Kraska".to_string()],
                min_required_count: 1,
            }],
        });
        assert_eq!(result, Err(ProductionMapError::RawMaterialInvalidInput));
    }
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
