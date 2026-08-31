use std::collections::BTreeSet;

use thiserror::Error;

use super::model::{AAS_SHELL_ID_PREFIX, AasIdentity};
use super::{
    CANONICAL_APPARATUS_SCHEMA_VERSION, CanonicalApparatusRevision, CapacityAvailability,
    EquipmentCapabilityCode, ExecutionOperation, LifecycleState, MaterialExecutionPolicy,
    ProcessTechnology, ToolingExecutionPolicy, VirtualTaskPolicy,
};
use crate::{
    AAS_APPARATUS_SUBMODEL_ID_PREFIX, AAS_APPARATUS_SUBMODEL_SEMANTIC_ID, AAS_METAMODEL_VERSION,
    AASX_MEDIA_TYPE, AASX_PACKAGE_FORMAT, AASX_PART_5_VERSION, IDTA_RELEASE,
};

const MAX_DISPLAY_NAME_CHARS: usize = 256;
const MAX_DESCRIPTION_CHARS: usize = 2_000;
const MAX_REFERENCE_CHARS: usize = 256;
const MAX_CAPACITY_SLOTS: u16 = 64;
const MAX_EFFICIENCY_PERCENT: u16 = 200;
const MAX_CAPABILITY_LEVEL: u16 = 100;
const MAX_COLOR_STATIONS: u16 = 32;
const MAX_WEB_WIDTH_MM: u32 = 100_000;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanonicalApparatusValidationError {
    #[error("invalid canonical identifier: {0}")]
    InvalidIdentifier(&'static str),
    #[error("canonical schema version is not supported")]
    InvalidSchemaVersion,
    #[error("display metadata is invalid")]
    InvalidDisplay,
    #[error("equipment capabilities must be canonical, complete, and unique")]
    InvalidCapabilities,
    #[error("execution profile conflicts with explicit capabilities")]
    InvalidExecutionProfile,
    #[error("operational policy is incomplete or internally inconsistent")]
    InvalidPolicy,
    #[error("capacity configuration is invalid")]
    InvalidCapacity,
    #[error("lifecycle configuration is invalid")]
    InvalidLifecycle,
    #[error("immutable revision metadata is invalid")]
    InvalidRevisionMetadata,
    #[error("AAS identity does not match the pinned project profile")]
    InvalidAasIdentity,
    #[error("invalid canonical reference: {0}")]
    InvalidReference(&'static str),
}

impl CanonicalApparatusRevision {
    pub fn validate(&self) -> Result<(), CanonicalApparatusValidationError> {
        if self.schema_version != CANONICAL_APPARATUS_SCHEMA_VERSION {
            return Err(CanonicalApparatusValidationError::InvalidSchemaVersion);
        }
        validate_display(self)?;
        validate_capabilities(self)?;
        validate_execution_profile(self)?;
        validate_policies(self)?;
        validate_capacity(self)?;
        validate_lifecycle(self)?;
        validate_revision_metadata(self)?;
        validate_aas_identity(self)?;
        if let Some(placement) = &self.placement {
            validate_reference("factory_map_object_id", &placement.factory_map_object_id)?;
        }
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.lifecycle.state == LifecycleState::Active
    }

    pub fn supports(&self, code: EquipmentCapabilityCode) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.code == code)
    }
}

fn validate_display(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    let display = &revision.display;
    if display.display_name.is_empty()
        || display.display_name != display.display_name.trim()
        || display.display_name.chars().count() > MAX_DISPLAY_NAME_CHARS
        || display.description.chars().count() > MAX_DESCRIPTION_CHARS
        || display.display_name.chars().any(char::is_control)
        || display.description.chars().any(char::is_control)
    {
        return Err(CanonicalApparatusValidationError::InvalidDisplay);
    }
    Ok(())
}

fn validate_capabilities(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    if revision.capabilities.is_empty()
        || revision
            .capabilities
            .iter()
            .any(|capability| capability.level == 0 || capability.level > MAX_CAPABILITY_LEVEL)
    {
        return Err(CanonicalApparatusValidationError::InvalidCapabilities);
    }
    let codes = revision
        .capabilities
        .iter()
        .map(|capability| capability.code)
        .collect::<Vec<_>>();
    let unique = codes.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != codes.len() || !codes.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(CanonicalApparatusValidationError::InvalidCapabilities);
    }
    Ok(())
}

fn validate_execution_profile(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    let profile = &revision.execution_profile;
    let expected_capability = match profile.operation {
        ExecutionOperation::Print => EquipmentCapabilityCode::Print,
        ExecutionOperation::Laminate => EquipmentCapabilityCode::Laminate,
        ExecutionOperation::Cut => EquipmentCapabilityCode::Cut,
        ExecutionOperation::Package => EquipmentCapabilityCode::Package,
        ExecutionOperation::Glue => EquipmentCapabilityCode::Glue,
    };
    let technology_matches = matches!(
        (profile.operation, profile.technology),
        (
            ExecutionOperation::Print,
            ProcessTechnology::Rotogravure | ProcessTechnology::Flexographic
        ) | (
            ExecutionOperation::Laminate,
            ProcessTechnology::AdhesiveLamination | ProcessTechnology::ExtrusionLamination
        ) | (ExecutionOperation::Cut, ProcessTechnology::Slitting)
            | (ExecutionOperation::Package, ProcessTechnology::BagMaking)
            | (ExecutionOperation::Glue, ProcessTechnology::ColdGlue)
    );
    let color_stations_match = match profile.technology {
        ProcessTechnology::Rotogravure => profile
            .color_station_count
            .is_some_and(|value| value > 0 && value <= MAX_COLOR_STATIONS),
        ProcessTechnology::Flexographic => profile
            .color_station_count
            .is_none_or(|value| value > 0 && value <= MAX_COLOR_STATIONS),
        _ => profile.color_station_count.is_none(),
    };
    let virtual_tasks_match = profile.virtual_tasks == VirtualTaskPolicy::Disabled
        || revision.supports(EquipmentCapabilityCode::VirtualTask);
    let min_web_width_match = profile
        .min_web_width_mm
        .is_none_or(|value| value > 0 && value <= MAX_WEB_WIDTH_MM);
    let max_web_width_match = profile
        .max_web_width_mm
        .is_none_or(|value| value > 0 && value <= MAX_WEB_WIDTH_MM);
    let web_width_range_matches = profile
        .min_web_width_mm
        .zip(profile.max_web_width_mm)
        .is_none_or(|(minimum, maximum)| minimum <= maximum);
    if !revision.supports(expected_capability)
        || !technology_matches
        || !color_stations_match
        || !virtual_tasks_match
        || !min_web_width_match
        || !max_web_width_match
        || !web_width_range_matches
    {
        return Err(CanonicalApparatusValidationError::InvalidExecutionProfile);
    }
    Ok(())
}

fn validate_policies(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    match &revision.policies.material {
        MaterialExecutionPolicy::NotRequired { item_group_ids } => {
            if !item_group_ids.is_empty() {
                validate_sorted_unique_references(item_group_ids, "material_item_group")?;
            }
        }
        MaterialExecutionPolicy::AllRequired { item_group_ids } => {
            validate_sorted_unique_references(item_group_ids, "material_item_group")?;
        }
        MaterialExecutionPolicy::RequirementSets { sets } => {
            if sets.is_empty()
                || !sets
                    .windows(2)
                    .all(|pair| pair[0].requirement_id < pair[1].requirement_id)
            {
                return Err(CanonicalApparatusValidationError::InvalidPolicy);
            }
            for set in sets {
                validate_reference("material_requirement_id", &set.requirement_id)?;
                if set.minimum_required_count == 0
                    || usize::from(set.minimum_required_count) > set.item_group_ids.len()
                {
                    return Err(CanonicalApparatusValidationError::InvalidPolicy);
                }
                validate_sorted_unique_references(&set.item_group_ids, "material_item_group")?;
            }
        }
    }
    if let ToolingExecutionPolicy::QolipScanRequired { tooling_class_id } =
        &revision.policies.tooling
    {
        if !revision.supports(EquipmentCapabilityCode::Tooling) {
            return Err(CanonicalApparatusValidationError::InvalidPolicy);
        }
        validate_reference("tooling_class_id", tooling_class_id)?;
    }
    if revision.training.enabled != revision.supports(EquipmentCapabilityCode::Training)
        || (!revision.training.enabled
            && (revision.training.queue_enabled || revision.training.material_tracking_enabled))
    {
        return Err(CanonicalApparatusValidationError::InvalidPolicy);
    }
    Ok(())
}

fn validate_capacity(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    let capacity = &revision.capacity;
    if capacity.capacity_slots == 0
        || capacity.capacity_slots > MAX_CAPACITY_SLOTS
        || capacity.efficiency_percent == 0
        || capacity.efficiency_percent > MAX_EFFICIENCY_PERCENT
    {
        return Err(CanonicalApparatusValidationError::InvalidCapacity);
    }
    if let CapacityAvailability::Scheduled { working_windows } = &capacity.availability
        && (working_windows.is_empty()
            || !working_windows.windows(2).all(|pair| pair[0] < pair[1])
            || working_windows.iter().any(|window| {
                window.weekday == 0
                    || window.weekday > 7
                    || window.start_minute >= window.end_minute
                    || window.end_minute > 24 * 60
            }))
    {
        return Err(CanonicalApparatusValidationError::InvalidCapacity);
    }
    Ok(())
}

fn validate_lifecycle(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    match (
        revision.lifecycle.state,
        revision.lifecycle.retirement_reason.as_deref(),
    ) {
        (LifecycleState::Active, None) => Ok(()),
        (LifecycleState::Retired, Some(reason)) => validate_reference("retirement_reason", reason),
        _ => Err(CanonicalApparatusValidationError::InvalidLifecycle),
    }
}

fn validate_revision_metadata(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    let metadata = &revision.revision_metadata;
    if metadata.revision == 0 || metadata.committed_at_unix_ms <= 0 {
        return Err(CanonicalApparatusValidationError::InvalidRevisionMetadata);
    }
    validate_reference("actor_id", &metadata.actor_id)?;
    validate_reference("command_id", &metadata.command_id)?;
    if let Some(reference) = metadata.source_reference.as_deref() {
        validate_reference("source_reference", reference)?;
    }
    Ok(())
}

fn validate_aas_identity(
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusValidationError> {
    let suffix = revision
        .apparatus_id
        .as_str()
        .strip_prefix("apparatus:")
        .ok_or(CanonicalApparatusValidationError::InvalidAasIdentity)?;
    let expected = AasIdentity {
        shell_id: format!("{AAS_SHELL_ID_PREFIX}{suffix}"),
        submodel_id: format!("{AAS_APPARATUS_SUBMODEL_ID_PREFIX}{suffix}"),
        semantic_id: AAS_APPARATUS_SUBMODEL_SEMANTIC_ID.to_string(),
        idta_release: IDTA_RELEASE.to_string(),
        aas_metamodel_version: AAS_METAMODEL_VERSION.to_string(),
        aasx_part_5_version: AASX_PART_5_VERSION.to_string(),
        package_format: AASX_PACKAGE_FORMAT.to_string(),
        media_type: AASX_MEDIA_TYPE.to_string(),
    };
    if revision.aas_identity != expected {
        return Err(CanonicalApparatusValidationError::InvalidAasIdentity);
    }
    Ok(())
}

fn validate_sorted_unique_references(
    values: &[String],
    field: &'static str,
) -> Result<(), CanonicalApparatusValidationError> {
    if values.is_empty() || !values.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(CanonicalApparatusValidationError::InvalidPolicy);
    }
    for value in values {
        validate_reference(field, value)?;
    }
    Ok(())
}

fn validate_reference(
    field: &'static str,
    value: &str,
) -> Result<(), CanonicalApparatusValidationError> {
    if value.is_empty()
        || value != value.trim()
        || value.chars().count() > MAX_REFERENCE_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(CanonicalApparatusValidationError::InvalidReference(field));
    }
    Ok(())
}
