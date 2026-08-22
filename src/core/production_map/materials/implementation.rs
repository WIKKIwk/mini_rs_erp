use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::core::apparatus_standard::{
    ApparatusId, MaterialExecutionPolicy, RuntimeApparatusConfiguration,
};
use crate::core::qolip::QolipOrderStartPreparation;

use super::materials_support::*;
use super::queue_state;
use super::{
    ApparatusQueueActionResult, OrderControlState, PreparedApparatusQueueAction,
    ProductionMapError, ProductionMapSaved, ProductionMapService, QueueActionActor,
    QueueProgressInput, chain,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawMaterialStartPolicy {
    #[default]
    StateAll,
    RequirementGroups,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRequirementGroup {
    pub name: String,
    pub item_groups: Vec<String>,
    #[serde(default = "default_min_required_count")]
    pub min_required_count: u16,
}

fn default_min_required_count() -> u16 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRule {
    pub apparatus_id: ApparatusId,
    /// Historical/display snapshot only. Never use this for live identity.
    #[serde(default)]
    pub apparatus: String,
    #[serde(default)]
    pub requires_material: bool,
    #[serde(default)]
    pub start_policy: RawMaterialStartPolicy,
    #[serde(default)]
    pub item_groups: Vec<String>,
    #[serde(default)]
    pub requirement_groups: Vec<ApparatusMaterialRequirementGroup>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialRuleUpsert {
    pub apparatus: String,
    #[serde(default)]
    pub requires_material: bool,
    #[serde(default)]
    pub start_policy: RawMaterialStartPolicy,
    #[serde(default)]
    pub item_groups: Vec<String>,
    #[serde(default)]
    pub requirement_groups: Vec<ApparatusMaterialRequirementGroup>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMaterialAssignmentInput {
    pub order_id: String,
    pub barcode: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub item_group: String,
    #[serde(default)]
    pub item_group_path: Vec<String>,
    #[serde(default)]
    pub apparatus: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMaterialAssignmentDeleteInput {
    pub order_id: String,
    pub barcode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMaterialAssignment {
    pub order_id: String,
    pub apparatus_id: ApparatusId,
    /// Historical/display snapshot only. Never use this for live identity.
    #[serde(default)]
    pub apparatus: String,
    pub barcode: String,
    pub item_code: String,
    pub item_name: String,
    pub item_group: String,
    pub assigned_by_role: String,
    pub assigned_by_ref: String,
    pub assigned_by_display_name: String,
    pub assigned_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawMaterialStartRequirements {
    pub policy: RawMaterialStartPolicy,
    pub requires_material: bool,
    pub material_scan_required: bool,
    pub requirement_groups: Vec<ApparatusMaterialRequirementGroup>,
    pub assigned_barcodes: Vec<String>,
    pub staged_barcodes: Vec<String>,
    pub eligible_barcodes: Vec<String>,
    pub required_scan_count: usize,
    pub matched_scan_count: usize,
    pub assignments_satisfied: bool,
    pub scan_satisfied: bool,
}

/// Validation produced by the trusted admin Qolip boundary for one queue
/// start. Core queue callers cannot manufacture the private identity fields;
/// callers without this token remain fail-closed for Qolip-protected starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedQolipStartValidation {
    apparatus_id: ApparatusId,
    order_id: String,
    qolip_codes: Vec<String>,
}

impl TrustedQolipStartValidation {
    pub(crate) fn from_preparations(
        apparatus_id: &ApparatusId,
        order_id: &str,
        preparations: &[QolipOrderStartPreparation],
    ) -> Option<Self> {
        let mut qolip_codes = preparations
            .iter()
            .map(|preparation| preparation.spec.qolip_code.trim().to_lowercase())
            .filter(|code| !code.is_empty())
            .collect::<Vec<_>>();
        qolip_codes.sort();
        qolip_codes.dedup();
        let order_id = order_id.trim();
        if order_id.is_empty() || qolip_codes.is_empty() {
            return None;
        }
        Some(Self {
            apparatus_id: apparatus_id.clone(),
            order_id: order_id.to_string(),
            qolip_codes,
        })
    }

    pub(crate) fn matches(&self, apparatus_id: &ApparatusId, order_id: &str) -> bool {
        self.apparatus_id == *apparatus_id
            && self.order_id.eq_ignore_ascii_case(order_id.trim())
            && !self.qolip_codes.is_empty()
    }
}

pub struct MaterialScanProgressAction<'a> {
    pub apparatus: &'a str,
    pub order_id: &'a str,
    pub action: queue_state::ApparatusQueueAction,
    pub assigned_apparatus: &'a [String],
    pub actor: QueueActionActor,
    pub material_barcode: &'a str,
    pub state_material_barcodes: &'a [String],
    pub progress: QueueProgressInput,
    pub qolip_validation: Option<TrustedQolipStartValidation>,
}

include!("implementation_impl_parts/part_01.rs");
include!("implementation_impl_parts/part_02.rs");

fn material_rule_record(canonical: &RuntimeApparatusConfiguration) -> ApparatusMaterialRule {
    let (requires_material, start_policy, item_groups, requirement_groups) =
        match &canonical.material.policy {
            MaterialExecutionPolicy::NotRequired { item_group_ids } => (
                false,
                RawMaterialStartPolicy::StateAll,
                item_group_ids.clone(),
                Vec::new(),
            ),
            MaterialExecutionPolicy::AllRequired { item_group_ids } => (
                true,
                RawMaterialStartPolicy::StateAll,
                item_group_ids.clone(),
                Vec::new(),
            ),
            MaterialExecutionPolicy::RequirementSets { sets } => (
                true,
                RawMaterialStartPolicy::RequirementGroups,
                Vec::new(),
                sets.iter()
                    .map(|set| ApparatusMaterialRequirementGroup {
                        name: set.requirement_id.clone(),
                        item_groups: set.item_group_ids.clone(),
                        min_required_count: set.minimum_required_count,
                    })
                    .collect(),
            ),
        };
    ApparatusMaterialRule {
        apparatus_id: canonical.runtime.apparatus_id.clone(),
        apparatus: canonical.runtime.display.display_name.clone(),
        requires_material,
        start_policy,
        item_groups,
        requirement_groups,
    }
}

pub(crate) fn live_material_rule(
    canonical: &RuntimeApparatusConfiguration,
) -> Option<ApparatusMaterialRule> {
    match &canonical.material.policy {
        MaterialExecutionPolicy::NotRequired { item_group_ids } if item_group_ids.is_empty() => {
            None
        }
        _ => Some(material_rule_record(canonical)),
    }
}

pub(super) fn build_raw_material_start_requirements(
    rule: Option<&ApparatusMaterialRule>,
    assignments: &[RawMaterialAssignment],
    state_material_barcodes: &[String],
    material_barcodes: &str,
) -> RawMaterialStartRequirements {
    let policy = rule.map(|rule| rule.start_policy).unwrap_or_default();
    let requires_material = rule.is_some_and(|rule| rule.requires_material);
    let assigned = assignments
        .iter()
        .map(|assignment| normalize_barcode(&assignment.barcode))
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>();
    let staged = state_material_barcodes
        .iter()
        .map(|barcode| normalize_barcode(barcode))
        .filter(|barcode| assigned.contains(barcode))
        .collect::<BTreeSet<_>>();
    let scanned = normalized_barcodes(material_barcodes);
    let requirement_groups = rule.map(effective_requirement_groups).unwrap_or_default();
    let assignments_satisfied = if assignments.is_empty() {
        !requires_material
    } else {
        !requires_material
            || policy != RawMaterialStartPolicy::RequirementGroups
            || rule.is_some_and(|rule| material_requirements_met(rule, assignments))
    };
    let (eligible, required_scan_count, matched_scan_count, scan_satisfied) = match policy {
        RawMaterialStartPolicy::StateAll => {
            let matched = scanned.intersection(&staged).count();
            (
                staged.clone(),
                staged.len(),
                matched,
                assignments.is_empty() && !requires_material
                    || !assignments.is_empty()
                        && !scanned.is_empty()
                        && scanned.is_subset(&assigned)
                        && scanned == staged,
            )
        }
        RawMaterialStartPolicy::RequirementGroups => {
            let scanned_assignments = assignments
                .iter()
                .filter(|assignment| scanned.contains(&normalize_barcode(&assignment.barcode)))
                .cloned()
                .collect::<Vec<_>>();
            let required = rule
                .map(material_requirement_slot_count)
                .unwrap_or_default();
            let matched = rule
                .map(|rule| material_requirement_match_count(rule, &scanned_assignments))
                .unwrap_or_default();
            (
                assigned.clone(),
                required,
                matched,
                assignments.is_empty() && !requires_material
                    || !assignments.is_empty()
                        && scanned.is_subset(&assigned)
                        && required > 0
                        && matched == required,
            )
        }
    };
    RawMaterialStartRequirements {
        policy,
        requires_material,
        material_scan_required: requires_material || !assignments.is_empty(),
        requirement_groups,
        assigned_barcodes: assigned.into_iter().collect(),
        staged_barcodes: staged.into_iter().collect(),
        eligible_barcodes: eligible.into_iter().collect(),
        required_scan_count,
        matched_scan_count,
        assignments_satisfied,
        scan_satisfied,
    }
}
