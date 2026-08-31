use serde::{Deserialize, Serialize};

use super::{
    CANONICAL_APPARATUS_SCHEMA_VERSION, CanonicalApparatusValidationError, EquipmentClassId,
    HierarchyLevelId, PhysicalAssetId,
};
use crate::{
    AAS_APPARATUS_SUBMODEL_ID_PREFIX, AAS_APPARATUS_SUBMODEL_SEMANTIC_ID, AAS_METAMODEL_VERSION,
    AASX_MEDIA_TYPE, AASX_PACKAGE_FORMAT, AASX_PART_5_VERSION, ApparatusId, IDTA_RELEASE,
};

pub const AAS_SHELL_ID_PREFIX: &str = "urn:mini-rs-erp:aas:apparatus:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusDisplay {
    pub display_name: String,
    pub description: String,
    pub catalog_order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentHierarchyScope {
    pub enterprise_id: HierarchyLevelId,
    pub site_id: HierarchyLevelId,
    pub area_id: HierarchyLevelId,
    pub work_center_id: HierarchyLevelId,
    pub work_unit_id: HierarchyLevelId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentCapabilityCode {
    Print,
    Laminate,
    Cut,
    Package,
    Glue,
    Tooling,
    VirtualTask,
    Training,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentCapability {
    pub code: EquipmentCapabilityCode,
    pub level: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOperation {
    Print,
    Laminate,
    Cut,
    Package,
    Glue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessTechnology {
    Rotogravure,
    Flexographic,
    AdhesiveLamination,
    ExtrusionLamination,
    Slitting,
    BagMaking,
    ColdGlue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualTaskPolicy {
    Disabled,
    InputBridge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProfile {
    pub operation: ExecutionOperation,
    pub technology: ProcessTechnology,
    pub color_station_count: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_web_width_mm: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_web_width_mm: Option<u32>,
    pub virtual_tasks: VirtualTaskPolicy,
    pub capability_compatible_reroute: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueDiscipline {
    StrictSequence,
    FreePick,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialRequirementSet {
    pub requirement_id: String,
    pub item_group_ids: Vec<String>,
    pub minimum_required_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaterialExecutionPolicy {
    NotRequired {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        item_group_ids: Vec<String>,
    },
    AllRequired {
        item_group_ids: Vec<String>,
    },
    RequirementSets {
        sets: Vec<MaterialRequirementSet>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolingExecutionPolicy {
    NotRequired,
    QolipScanRequired { tooling_class_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusOperationalPolicies {
    pub queue: QueueDiscipline,
    pub material: MaterialExecutionPolicy,
    pub tooling: ToolingExecutionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkingWindowV1 {
    pub weekday: u8,
    pub start_minute: u16,
    pub end_minute: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapacityAvailability {
    Always,
    Scheduled {
        working_windows: Vec<WorkingWindowV1>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusCapacity {
    pub capacity_slots: u16,
    pub setup_minutes: u32,
    pub cleanup_minutes: u32,
    pub efficiency_percent: u16,
    pub finite_capacity: bool,
    pub availability: CapacityAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactoryMapPlacement {
    pub factory_map_object_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainingProfile {
    pub enabled: bool,
    pub queue_enabled: bool,
    pub material_tracking_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Active,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApparatusLifecycle {
    pub state: LifecycleState,
    pub retirement_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionSource {
    Admin,
    AasxImport,
    LegacyMigration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionMetadata {
    pub revision: u64,
    pub committed_at_unix_ms: i64,
    pub actor_id: String,
    pub command_id: String,
    pub source: RevisionSource,
    pub source_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AasIdentity {
    pub shell_id: String,
    pub submodel_id: String,
    pub semantic_id: String,
    pub idta_release: String,
    pub aas_metamodel_version: String,
    pub aasx_part_5_version: String,
    pub package_format: String,
    pub media_type: String,
}

impl AasIdentity {
    pub fn for_apparatus(apparatus_id: &ApparatusId) -> Self {
        let suffix = apparatus_id
            .as_str()
            .strip_prefix("apparatus:")
            .expect("validated ApparatusId has the apparatus prefix");
        Self {
            shell_id: format!("{AAS_SHELL_ID_PREFIX}{suffix}"),
            submodel_id: format!("{AAS_APPARATUS_SUBMODEL_ID_PREFIX}{suffix}"),
            semantic_id: AAS_APPARATUS_SUBMODEL_SEMANTIC_ID.to_string(),
            idta_release: IDTA_RELEASE.to_string(),
            aas_metamodel_version: AAS_METAMODEL_VERSION.to_string(),
            aasx_part_5_version: AASX_PART_5_VERSION.to_string(),
            package_format: AASX_PACKAGE_FORMAT.to_string(),
            media_type: AASX_MEDIA_TYPE.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalApparatusDraft {
    pub display: ApparatusDisplay,
    pub equipment_class_id: EquipmentClassId,
    pub physical_asset_id: PhysicalAssetId,
    pub hierarchy: EquipmentHierarchyScope,
    pub capabilities: Vec<EquipmentCapability>,
    pub execution_profile: ExecutionProfile,
    pub policies: ApparatusOperationalPolicies,
    pub capacity: ApparatusCapacity,
    pub placement: Option<FactoryMapPlacement>,
    pub training: TrainingProfile,
    pub lifecycle: ApparatusLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalApparatusRevision {
    pub schema_version: u32,
    pub apparatus_id: ApparatusId,
    pub display: ApparatusDisplay,
    pub equipment_class_id: EquipmentClassId,
    pub physical_asset_id: PhysicalAssetId,
    pub hierarchy: EquipmentHierarchyScope,
    pub capabilities: Vec<EquipmentCapability>,
    pub execution_profile: ExecutionProfile,
    pub policies: ApparatusOperationalPolicies,
    pub capacity: ApparatusCapacity,
    pub placement: Option<FactoryMapPlacement>,
    pub training: TrainingProfile,
    pub lifecycle: ApparatusLifecycle,
    pub revision_metadata: RevisionMetadata,
    pub aas_identity: AasIdentity,
}

impl CanonicalApparatusRevision {
    pub fn from_draft(
        apparatus_id: ApparatusId,
        mut draft: CanonicalApparatusDraft,
        revision_metadata: RevisionMetadata,
    ) -> Result<Self, CanonicalApparatusValidationError> {
        draft.normalize();
        let revision = Self {
            schema_version: CANONICAL_APPARATUS_SCHEMA_VERSION,
            aas_identity: AasIdentity::for_apparatus(&apparatus_id),
            apparatus_id,
            display: draft.display,
            equipment_class_id: draft.equipment_class_id,
            physical_asset_id: draft.physical_asset_id,
            hierarchy: draft.hierarchy,
            capabilities: draft.capabilities,
            execution_profile: draft.execution_profile,
            policies: draft.policies,
            capacity: draft.capacity,
            placement: draft.placement,
            training: draft.training,
            lifecycle: draft.lifecycle,
            revision_metadata,
        };
        revision.validate()?;
        Ok(revision)
    }

    pub fn to_draft(&self) -> CanonicalApparatusDraft {
        CanonicalApparatusDraft {
            display: self.display.clone(),
            equipment_class_id: self.equipment_class_id.clone(),
            physical_asset_id: self.physical_asset_id.clone(),
            hierarchy: self.hierarchy.clone(),
            capabilities: self.capabilities.clone(),
            execution_profile: self.execution_profile.clone(),
            policies: self.policies.clone(),
            capacity: self.capacity.clone(),
            placement: self.placement.clone(),
            training: self.training.clone(),
            lifecycle: self.lifecycle.clone(),
        }
    }
}

impl CanonicalApparatusDraft {
    pub fn normalize(&mut self) {
        self.capabilities.sort_by_key(|capability| capability.code);
        match &mut self.policies.material {
            MaterialExecutionPolicy::NotRequired { item_group_ids } => item_group_ids.sort(),
            MaterialExecutionPolicy::AllRequired { item_group_ids } => item_group_ids.sort(),
            MaterialExecutionPolicy::RequirementSets { sets } => {
                for set in sets.iter_mut() {
                    set.item_group_ids.sort();
                }
                sets.sort_by(|left, right| left.requirement_id.cmp(&right.requirement_id));
            }
        }
        if let CapacityAvailability::Scheduled { working_windows } = &mut self.capacity.availability
        {
            working_windows.sort();
        }
    }
}
