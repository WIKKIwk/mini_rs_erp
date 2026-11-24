use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::core::apparatus_standard::{
    ApparatusId, MaterialExecutionPolicy, RuntimeApparatusConfiguration,
};
use crate::core::qolip::QolipOrderStartPreparation;

use super::materials_support::*;
use super::queue_state;
use super::service_maps::compile_saved_maps;
use super::{
    ApparatusQueueActionResult, OrderControlState, OrderRunSession, OrderRunStatus, PreparedApparatusQueueAction,
    ProductionMapError, ProductionMapSaved, ProductionMapService, ProductionQrRawMaterial,
    ProductionQrSessionResources, QueueActionActor, QueueProgressInput, chain,
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
    pub material_barcodes: &'a [String],
    pub state_material_barcodes: &'a [String],
    pub progress: QueueProgressInput,
    pub qolip_validation: Option<TrustedQolipStartValidation>,
}

include!("implementation_parts/raw_material_assignment_orders.rs");

include!("implementation_parts/prepare_apparatus_queue_action_with_material_scan_and_progress.rs");

include!("implementation_parts/build_raw_material_start_requirements_refs.rs");
