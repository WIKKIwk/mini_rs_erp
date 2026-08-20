//! Canonical, configuration-only apparatus contract.
//!
//! This module deliberately does not contain order, queue, WIP, downtime,
//! reservation, or live execution state. Those records refer to this domain
//! by [`ApparatusId`] and remain separate contracts.

pub mod aasx;
pub mod canonical_aasx;
pub mod isa95;
pub mod projector;
pub mod service;

pub use isa95::{
    AasIdentity, ApparatusCapacity, ApparatusDisplay, ApparatusLifecycle,
    ApparatusOperationalPolicies, CanonicalApparatusDraft, CanonicalApparatusRevision,
    CapacityAvailability, EquipmentCapability, EquipmentCapabilityCode, EquipmentClassId,
    EquipmentHierarchyScope, ExecutionOperation, ExecutionProfile, FactoryMapPlacement,
    HierarchyLevelId, LifecycleState, MaterialExecutionPolicy, MaterialRequirementSet,
    PhysicalAssetId, ProcessTechnology, QueueDiscipline, RevisionMetadata, RevisionSource,
    ToolingExecutionPolicy, TrainingProfile, VirtualTaskPolicy, WorkingWindowV1,
    CANONICAL_APPARATUS_SCHEMA_VERSION,
};
pub use canonical_aasx::{
    CanonicalAasxArtifact, CanonicalAasxExportError, CanonicalAasxImportError,
    CanonicalizedAasxUpload, canonicalize_uploaded_aasx, export_canonical_aasx,
    parse_canonical_aasx,
};
pub use projector::{
    AdminApparatusSummary, ApparatusCapacityProjection, ApparatusMaterialProjection,
    ApparatusProjectionSet, ApparatusQueueProjection, AasxSha256, RuntimeApparatusProjection,
    project_apparatus_revision,
};
pub use service::{
    CanonicalApparatusError, CanonicalApparatusPatch, CanonicalApparatusService,
    CanonicalCommandMetadata, CommittedCanonicalApparatus, StoredCanonicalAasx,
};

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

pub const IDTA_RELEASE: &str = "26-01";
pub const AAS_METAMODEL_VERSION: &str = "3.2.0";
pub const AASX_PART_5_VERSION: &str = "IDTA-01005 v3.2";
pub const AASX_PACKAGE_FORMAT: &str = "Open Packaging Conventions";
pub const AASX_MEDIA_TYPE: &str = "application/asset-administration-shell-package";

/// Project-owned semantic target. It is not an IDTA-issued semantic ID.
pub const AAS_APPARATUS_SUBMODEL_SEMANTIC_ID: &str =
    "urn:mini-rs-erp:semantic-id:submodel:apparatus:1";
pub const AAS_APPARATUS_SUBMODEL_ID_PREFIX: &str = "urn:mini-rs-erp:submodel:apparatus:";

const APPARATUS_ID_PREFIX: &str = "apparatus:";
const MAX_ID_LENGTH: usize = 128;
const MAX_DISPLAY_NAME_LENGTH: usize = 256;
const MAX_DESCRIPTION_LENGTH: usize = 2_000;
const MAX_REFERENCE_LENGTH: usize = 128;
const MAX_AAS_SUBMODEL_ID_LENGTH: usize = 256;
pub const COLOR_STATIONS_MIN: u8 = 7;
pub const COLOR_STATIONS_MAX: u8 = 9;
pub const CAPACITY_SLOTS_MAX: u16 = 64;
pub const EFFICIENCY_PERCENT_MAX: u16 = 200;
pub const CAPABILITY_LEVEL_MAX: u16 = 100;

/// Stable catalog identity. The value is private and has no mutating API.
///
/// The canonical shape is `apparatus:<namespace>:<opaque-key>`. In
/// particular, the legacy `apparatus:<display-name>` shape is not accepted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApparatusId(String);

impl ApparatusId {
    pub fn new(value: impl Into<String>) -> Result<Self, ApparatusValidationError> {
        let value = value.into();
        validate_id_shape(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn rejects_title_identity(&self, display_name: &str) -> bool {
        let title = display_name.trim().to_ascii_lowercase();
        let legacy = format!("{APPARATUS_ID_PREFIX}{title}");
        if self.0.eq_ignore_ascii_case(&legacy) {
            return true;
        }
        let title_slug = title_identity_key(&title);
        self.0
            .rsplit(':')
            .next()
            .is_some_and(|key| title_identity_key(key) == title_slug)
    }
}

/// Compare identity candidates independent of punctuation separators. This
/// closes the `flexo-pechat` versus `flexo_pechat` bypass without treating an
/// opaque key such as `asset-005` as title-derived.
fn title_identity_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl AsRef<str> for ApparatusId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ApparatusId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ApparatusId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ApparatusId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusIdentity {
    pub id: ApparatusId,
    pub display: ApparatusDisplayMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusDisplayMetadata {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Optional UI ordering hint. It is non-semantic and never an identity,
    /// routing, or topology field.
    #[serde(default)]
    pub catalog_order: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusFamily {
    Pechat,
    Laminatsiya,
    Rezka,
    Paket,
    Kley,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusKind {
    ColorPechat,
    Flexo,
    Laminatsiya,
    ExtruderLaminatsiya,
    Rezka,
    Paket,
    HolodniyKley,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusClassification {
    pub family: ApparatusFamily,
    pub kind: ApparatusKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_stations: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCode {
    Print,
    Pechat,
    Flexo,
    Laminate,
    Cut,
    Package,
    Glue,
    Apparatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityProfile {
    pub code: CapabilityCode,
    #[serde(default = "default_capability_level")]
    pub level: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to_unix: Option<i64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuePolicy {
    StrictSequence,
    FreePick,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawMaterialStartPolicy {
    #[default]
    StateAll,
    RequirementGroups,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialRequirementGroup {
    pub name: String,
    pub item_groups: Vec<String>,
    #[serde(default = "default_min_required_count")]
    pub min_required_count: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialPolicy {
    #[serde(default)]
    pub requires_material: bool,
    #[serde(default)]
    pub start_policy: RawMaterialStartPolicy,
    #[serde(default)]
    pub item_groups: Vec<String>,
    #[serde(default)]
    pub requirement_groups: Vec<MaterialRequirementGroup>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolingPolicy {
    #[default]
    QolipScanNotRequired,
    QolipScanRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationalPolicies {
    pub queue: QueuePolicy,
    #[serde(default)]
    pub material: MaterialPolicy,
    #[serde(default)]
    pub tooling: ToolingPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingWindow {
    /// ISO weekday: Monday = 1, Sunday = 7.
    pub weekday: u8,
    /// Minutes after midnight, inclusive.
    pub start_minute: u16,
    /// Minutes after midnight, exclusive.
    pub end_minute: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacityConfiguration {
    #[serde(default = "default_capacity_slots")]
    pub capacity_slots: u16,
    #[serde(default)]
    pub setup_minutes: u32,
    #[serde(default)]
    pub cleanup_minutes: u32,
    #[serde(default = "default_efficiency_percent")]
    pub efficiency_percent: u16,
    #[serde(default = "default_finite_capacity")]
    pub finite_capacity: bool,
    #[serde(default)]
    pub working_windows: Vec<WorkingWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementReference {
    pub factory_map_object_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingReference {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    Default,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: CatalogSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versioning {
    #[serde(default = "default_revision")]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AasPackageMetadata {
    #[serde(default = "default_aas_submodel_id")]
    pub submodel_id: String,
    #[serde(default = "default_semantic_id")]
    pub semantic_id: String,
    #[serde(default = "default_idta_release")]
    pub idta_release: String,
    #[serde(default = "default_aas_metamodel_version")]
    pub aas_metamodel_version: String,
    #[serde(default = "default_aasx_part_5_version")]
    pub aasx_part_5_version: String,
    #[serde(default = "default_aasx_package_format")]
    pub package_format: String,
    #[serde(default = "default_aasx_media_type")]
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalApparatus {
    pub identity: ApparatusIdentity,
    pub classification: ApparatusClassification,
    pub capabilities: Vec<CapabilityCode>,
    #[serde(default)]
    pub capability_profiles: Vec<CapabilityProfile>,
    pub policies: OperationalPolicies,
    pub capacity: CapacityConfiguration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementReference>,
    pub training: TrainingReference,
    pub provenance: Provenance,
    pub versioning: Versioning,
    pub aas: AasPackageMetadata,
}

impl CanonicalApparatus {
    pub fn validate(&self) -> Result<(), ApparatusValidationError> {
        validate_display(&self.identity.display)?;
        if self
            .identity
            .id
            .rejects_title_identity(&self.identity.display.display_name)
        {
            return Err(ApparatusValidationError::TitleDerivedId);
        }
        validate_classification(&self.classification)?;
        validate_capabilities(&self.capabilities, &self.capability_profiles)?;
        validate_policies(&self.classification, &self.policies)?;
        validate_capacity(&self.capacity)?;
        if let Some(placement) = &self.placement {
            validate_reference("factory_map_object_id", &placement.factory_map_object_id)?;
        }
        if self.versioning.revision == 0 {
            return Err(ApparatusValidationError::RevisionRequired);
        }
        if let Some(source_ref) = self.provenance.source_ref.as_deref() {
            validate_reference("source_ref", source_ref)?;
        }
        validate_aas(&self.aas, &self.identity.id)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApparatusValidationError {
    #[error("apparatus id is empty")]
    EmptyId,
    #[error("apparatus id must use the canonical apparatus:<namespace>:<opaque-key> shape")]
    InvalidIdShape,
    #[error("apparatus id contains whitespace or control characters")]
    InvalidIdCharacters,
    #[error("apparatus id is title-derived")]
    TitleDerivedId,
    #[error("display name is required")]
    EmptyDisplayName,
    #[error("display metadata contains invalid characters or is too long")]
    InvalidDisplayMetadata,
    #[error("classification kind does not belong to its family")]
    ClassificationConflict,
    #[error("color stations are invalid")]
    InvalidColorStations,
    #[error("capabilities must be non-empty and unique")]
    InvalidCapabilities,
    #[error("capability profile is invalid")]
    InvalidCapabilityProfile,
    #[error("material policy conflicts with its start policy")]
    MaterialPolicyConflict,
    #[error("free-pick queue policy is not supported for pechat apparatus")]
    QueuePolicyConflict,
    #[error("tooling scan is supported only for supported pechat apparatus")]
    ToolingPolicyConflict,
    #[error("capacity configuration is invalid")]
    InvalidCapacity,
    #[error("invalid reference: {0}")]
    InvalidReference(&'static str),
    #[error("version revision must be positive")]
    RevisionRequired,
    #[error("AAS package metadata does not match the pinned contract")]
    InvalidAasMetadata,
}

fn validate_id_shape(value: &str) -> Result<(), ApparatusValidationError> {
    if value.trim().is_empty() {
        return Err(ApparatusValidationError::EmptyId);
    }
    if value != value.trim()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(ApparatusValidationError::InvalidIdCharacters);
    }
    if value.len() > MAX_ID_LENGTH || !value.starts_with(APPARATUS_ID_PREFIX) {
        return Err(ApparatusValidationError::InvalidIdShape);
    }
    let segments = value[APPARATUS_ID_PREFIX.len()..]
        .split(':')
        .collect::<Vec<_>>();
    if segments.len() != 2 || segments.iter().any(|segment| segment.is_empty()) {
        return Err(ApparatusValidationError::InvalidIdShape);
    }
    if segments.iter().any(|segment| {
        segment.chars().any(|character| {
            !(character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.'))
        })
    }) {
        return Err(ApparatusValidationError::InvalidIdShape);
    }
    Ok(())
}

fn validate_display(display: &ApparatusDisplayMetadata) -> Result<(), ApparatusValidationError> {
    let name = display.display_name.trim();
    if name.is_empty() {
        return Err(ApparatusValidationError::EmptyDisplayName);
    }
    if name.chars().count() > MAX_DISPLAY_NAME_LENGTH
        || display.description.chars().count() > MAX_DESCRIPTION_LENGTH
        || name.chars().any(char::is_control)
        || display.description.chars().any(char::is_control)
    {
        return Err(ApparatusValidationError::InvalidDisplayMetadata);
    }
    Ok(())
}

fn validate_classification(
    classification: &ApparatusClassification,
) -> Result<(), ApparatusValidationError> {
    let valid_kind = match classification.family {
        ApparatusFamily::Pechat => matches!(
            classification.kind,
            ApparatusKind::ColorPechat | ApparatusKind::Flexo
        ),
        ApparatusFamily::Laminatsiya => matches!(
            classification.kind,
            ApparatusKind::Laminatsiya | ApparatusKind::ExtruderLaminatsiya
        ),
        ApparatusFamily::Rezka => classification.kind == ApparatusKind::Rezka,
        ApparatusFamily::Paket => classification.kind == ApparatusKind::Paket,
        ApparatusFamily::Kley => classification.kind == ApparatusKind::HolodniyKley,
        ApparatusFamily::Other => classification.kind == ApparatusKind::Other,
    };
    if !valid_kind {
        return Err(ApparatusValidationError::ClassificationConflict);
    }
    match (classification.kind, classification.color_stations) {
        (ApparatusKind::ColorPechat, Some(stations))
            if (COLOR_STATIONS_MIN..=COLOR_STATIONS_MAX).contains(&stations) =>
        {
            Ok(())
        }
        (ApparatusKind::ColorPechat, _) => Err(ApparatusValidationError::InvalidColorStations),
        (_, None) => Ok(()),
        _ => Err(ApparatusValidationError::InvalidColorStations),
    }
}

fn validate_capabilities(
    capabilities: &[CapabilityCode],
    profiles: &[CapabilityProfile],
) -> Result<(), ApparatusValidationError> {
    if capabilities.is_empty() {
        return Err(ApparatusValidationError::InvalidCapabilities);
    }
    let unique = capabilities
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if unique.len() != capabilities.len()
        || profiles.iter().any(|profile| {
            profile.level == 0
                || profile.level > CAPABILITY_LEVEL_MAX
                || profile
                    .valid_from_unix
                    .zip(profile.valid_to_unix)
                    .is_some_and(|(start, end)| end <= start)
                || !capabilities.contains(&profile.code)
                || profiles.iter().any(|other| {
                    other.code == profile.code
                        && other.valid_from_unix == profile.valid_from_unix
                        && !std::ptr::eq(profile, other)
                })
        })
    {
        return Err(ApparatusValidationError::InvalidCapabilityProfile);
    }
    Ok(())
}

fn validate_policies(
    classification: &ApparatusClassification,
    policies: &OperationalPolicies,
) -> Result<(), ApparatusValidationError> {
    if classification.family == ApparatusFamily::Pechat && policies.queue == QueuePolicy::FreePick {
        return Err(ApparatusValidationError::QueuePolicyConflict);
    }
    let material = &policies.material;
    if !material.requires_material {
        if material.start_policy != RawMaterialStartPolicy::StateAll
            || !material.item_groups.is_empty()
            || !material.requirement_groups.is_empty()
        {
            return Err(ApparatusValidationError::MaterialPolicyConflict);
        }
    } else {
        match material.start_policy {
            RawMaterialStartPolicy::StateAll
                if material.item_groups.is_empty() || !material.requirement_groups.is_empty() =>
            {
                return Err(ApparatusValidationError::MaterialPolicyConflict);
            }
            RawMaterialStartPolicy::RequirementGroups
                if material.requirement_groups.is_empty() || !material.item_groups.is_empty() =>
            {
                return Err(ApparatusValidationError::MaterialPolicyConflict);
            }
            _ => {}
        }
        if material.requirement_groups.iter().any(|group| {
            group.name.trim().is_empty()
                || group.item_groups.is_empty()
                || group.min_required_count == 0
        }) {
            return Err(ApparatusValidationError::MaterialPolicyConflict);
        }
    }
    let qolip_supported = classification.family == ApparatusFamily::Pechat
        && (classification.kind == ApparatusKind::Flexo
            || (classification.kind == ApparatusKind::ColorPechat
                && classification
                    .color_stations
                    .is_some_and(|stations| (7..=9).contains(&stations))));
    if policies.tooling == ToolingPolicy::QolipScanRequired && !qolip_supported {
        return Err(ApparatusValidationError::ToolingPolicyConflict);
    }
    Ok(())
}

fn validate_capacity(capacity: &CapacityConfiguration) -> Result<(), ApparatusValidationError> {
    if capacity.capacity_slots == 0
        || capacity.capacity_slots > CAPACITY_SLOTS_MAX
        || capacity.efficiency_percent == 0
        || capacity.efficiency_percent > EFFICIENCY_PERCENT_MAX
        || capacity.working_windows.iter().any(|window| {
            window.weekday == 0
                || window.weekday > 7
                || window.start_minute >= window.end_minute
                || window.end_minute > 24 * 60
        })
    {
        return Err(ApparatusValidationError::InvalidCapacity);
    }
    Ok(())
}

fn validate_reference(name: &'static str, value: &str) -> Result<(), ApparatusValidationError> {
    if value.trim().is_empty()
        || value.chars().count() > MAX_REFERENCE_LENGTH
        || value.chars().any(char::is_control)
    {
        return Err(ApparatusValidationError::InvalidReference(name));
    }
    Ok(())
}

fn validate_aas(
    aas: &AasPackageMetadata,
    apparatus_id: &ApparatusId,
) -> Result<(), ApparatusValidationError> {
    if aas.submodel_id.trim().is_empty()
        || aas.submodel_id != aas.submodel_id.trim()
        || aas.submodel_id.chars().count() > MAX_AAS_SUBMODEL_ID_LENGTH
        || aas.submodel_id.chars().any(char::is_control)
    {
        return Err(ApparatusValidationError::InvalidReference("submodel_id"));
    }
    if aas.submodel_id != aas_submodel_id_for_apparatus(apparatus_id)
        || aas.semantic_id != AAS_APPARATUS_SUBMODEL_SEMANTIC_ID
        || aas.idta_release != IDTA_RELEASE
        || aas.aas_metamodel_version != AAS_METAMODEL_VERSION
        || aas.aasx_part_5_version != AASX_PART_5_VERSION
        || aas.package_format != AASX_PACKAGE_FORMAT
        || aas.media_type != AASX_MEDIA_TYPE
    {
        return Err(ApparatusValidationError::InvalidAasMetadata);
    }
    Ok(())
}

fn aas_submodel_id_for_apparatus(apparatus_id: &ApparatusId) -> String {
    let suffix = apparatus_id
        .as_str()
        .strip_prefix(APPARATUS_ID_PREFIX)
        .expect("ApparatusId must use the canonical apparatus prefix");
    format!("{AAS_APPARATUS_SUBMODEL_ID_PREFIX}{suffix}")
}

pub fn default_aas_package_metadata() -> AasPackageMetadata {
    AasPackageMetadata {
        submodel_id: format!("{AAS_APPARATUS_SUBMODEL_ID_PREFIX}canonical"),
        semantic_id: AAS_APPARATUS_SUBMODEL_SEMANTIC_ID.to_string(),
        idta_release: IDTA_RELEASE.to_string(),
        aas_metamodel_version: AAS_METAMODEL_VERSION.to_string(),
        aasx_part_5_version: AASX_PART_5_VERSION.to_string(),
        package_format: AASX_PACKAGE_FORMAT.to_string(),
        media_type: AASX_MEDIA_TYPE.to_string(),
    }
}

/// Returns the pinned AAS metadata for one canonical apparatus identity.
///
/// The submodel identifier is derived from the opaque apparatus ID, never
/// from display metadata, so two apparatus identities cannot share the
/// canonical submodel reference.
pub fn aas_package_metadata_for_apparatus(
    apparatus_id: &ApparatusId,
) -> AasPackageMetadata {
    let mut metadata = default_aas_package_metadata();
    metadata.submodel_id = aas_submodel_id_for_apparatus(apparatus_id);
    metadata
}

fn default_capacity_slots() -> u16 {
    1
}

fn default_efficiency_percent() -> u16 {
    100
}

fn default_finite_capacity() -> bool {
    true
}

fn default_capability_level() -> u16 {
    1
}

fn default_min_required_count() -> u16 {
    1
}

fn default_enabled() -> bool {
    true
}

fn default_revision() -> u64 {
    1
}

fn default_aas_submodel_id() -> String {
    format!("{AAS_APPARATUS_SUBMODEL_ID_PREFIX}canonical")
}

fn default_semantic_id() -> String {
    AAS_APPARATUS_SUBMODEL_SEMANTIC_ID.to_string()
}

fn default_idta_release() -> String {
    IDTA_RELEASE.to_string()
}

fn default_aas_metamodel_version() -> String {
    AAS_METAMODEL_VERSION.to_string()
}

fn default_aasx_part_5_version() -> String {
    AASX_PART_5_VERSION.to_string()
}

fn default_aasx_package_format() -> String {
    AASX_PACKAGE_FORMAT.to_string()
}

fn default_aasx_media_type() -> String {
    AASX_MEDIA_TYPE.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_apparatus() -> CanonicalApparatus {
        let id = ApparatusId::new("apparatus:catalog:7-color-001").unwrap();
        CanonicalApparatus {
            identity: ApparatusIdentity {
                id: id.clone(),
                display: ApparatusDisplayMetadata {
                    display_name: "7 ta rangli bosma aparat".to_string(),
                    description: String::new(),
                    catalog_order: 1,
                },
            },
            classification: ApparatusClassification {
                family: ApparatusFamily::Pechat,
                kind: ApparatusKind::ColorPechat,
                color_stations: Some(7),
            },
            capabilities: vec![CapabilityCode::Print, CapabilityCode::Pechat],
            capability_profiles: vec![CapabilityProfile {
                code: CapabilityCode::Print,
                level: 1,
                valid_from_unix: None,
                valid_to_unix: None,
                enabled: true,
            }],
            policies: OperationalPolicies {
                queue: QueuePolicy::StrictSequence,
                material: MaterialPolicy::default(),
                tooling: ToolingPolicy::QolipScanRequired,
            },
            capacity: CapacityConfiguration {
                capacity_slots: 1,
                setup_minutes: 0,
                cleanup_minutes: 0,
                efficiency_percent: 100,
                finite_capacity: true,
                working_windows: Vec::new(),
            },
            placement: None,
            training: TrainingReference { enabled: true },
            provenance: Provenance {
                source: CatalogSource::Default,
                source_ref: None,
            },
            versioning: Versioning { revision: 1 },
            aas: aas_package_metadata_for_apparatus(&id),
        }
    }

    #[test]
    fn id_is_opaque_and_rejects_legacy_title_shape() {
        assert_eq!(
            ApparatusId::new("apparatus:catalog:stable-001")
                .unwrap()
                .as_str(),
            "apparatus:catalog:stable-001"
        );
        assert_eq!(
            ApparatusId::new("apparatus:laminatsiya_1"),
            Err(ApparatusValidationError::InvalidIdShape)
        );
        assert_eq!(
            ApparatusId::new(" "),
            Err(ApparatusValidationError::EmptyId)
        );
    }

    #[test]
    fn aas_submodel_identity_is_unique_and_bound_to_apparatus_id() {
        let first = valid_apparatus();
        let mut second = valid_apparatus();
        second.identity.id = ApparatusId::new("apparatus:catalog:7-color-002").unwrap();
        second.aas = aas_package_metadata_for_apparatus(&second.identity.id);

        first.validate().unwrap();
        second.validate().unwrap();
        assert_ne!(first.aas.submodel_id, second.aas.submodel_id);
        assert_eq!(
            first.aas.submodel_id,
            "urn:mini-rs-erp:submodel:apparatus:catalog:7-color-001"
        );

        let mut mismatched = first;
        mismatched.aas = second.aas;
        assert_eq!(
            mismatched.validate(),
            Err(ApparatusValidationError::InvalidAasMetadata)
        );
    }

    #[test]
    fn canonical_validation_rejects_title_derived_identity_and_policy_conflicts() {
        let mut apparatus = valid_apparatus();
        apparatus.identity.id =
            ApparatusId::new("apparatus:catalog:7_ta_rangli_bosma_aparat").unwrap();
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::TitleDerivedId)
        );

        apparatus.identity.id = ApparatusId::new("apparatus:catalog:flexo-pechat").unwrap();
        apparatus.identity.display.display_name = "Flexo pechat".to_string();
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::TitleDerivedId)
        );

        let mut apparatus = valid_apparatus();
        apparatus.policies.tooling = ToolingPolicy::QolipScanNotRequired;
        apparatus.policies.material = MaterialPolicy {
            requires_material: false,
            start_policy: RawMaterialStartPolicy::RequirementGroups,
            item_groups: Vec::new(),
            requirement_groups: Vec::new(),
        };
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::MaterialPolicyConflict)
        );
    }

    #[test]
    fn pechat_free_pick_is_rejected_for_color_and_flexo() {
        let mut apparatus = valid_apparatus();
        apparatus.policies.queue = QueuePolicy::FreePick;
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::QueuePolicyConflict)
        );

        apparatus.identity.id = ApparatusId::new("apparatus:catalog:flexo-free-pick").unwrap();
        apparatus.identity.display.display_name = "Flexo pechat".to_string();
        apparatus.classification = ApparatusClassification {
            family: ApparatusFamily::Pechat,
            kind: ApparatusKind::Flexo,
            color_stations: None,
        };
        apparatus.capabilities = vec![
            CapabilityCode::Print,
            CapabilityCode::Pechat,
            CapabilityCode::Flexo,
        ];
        apparatus.policies.tooling = ToolingPolicy::QolipScanNotRequired;
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::QueuePolicyConflict)
        );
    }

    #[test]
    fn color_pechat_requires_evidence_backed_station_boundaries() {
        let mut apparatus = valid_apparatus();
        for stations in [7, 9] {
            apparatus.classification.color_stations = Some(stations);
            assert_eq!(apparatus.validate(), Ok(()));
        }
        for stations in [6, 10] {
            apparatus.classification.color_stations = Some(stations);
            assert_eq!(
                apparatus.validate(),
                Err(ApparatusValidationError::InvalidColorStations)
            );
        }
        apparatus.classification.color_stations = None;
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::InvalidColorStations)
        );
    }

    #[test]
    fn flexo_qolip_tooling_matches_pechat_behavior() {
        let mut apparatus = valid_apparatus();
        apparatus.identity.id = ApparatusId::new("apparatus:catalog:flexo-001").unwrap();
        apparatus.identity.display.display_name = "Flexo pechat".to_string();
        apparatus.classification = ApparatusClassification {
            family: ApparatusFamily::Pechat,
            kind: ApparatusKind::Flexo,
            color_stations: None,
        };
        apparatus.capabilities = vec![
            CapabilityCode::Print,
            CapabilityCode::Pechat,
            CapabilityCode::Flexo,
        ];
        assert_eq!(apparatus.validate(), Ok(()));

        apparatus.classification = ApparatusClassification {
            family: ApparatusFamily::Laminatsiya,
            kind: ApparatusKind::Laminatsiya,
            color_stations: None,
        };
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::ToolingPolicyConflict)
        );
    }

    #[test]
    fn canonical_types_round_trip_through_serde() {
        let apparatus = valid_apparatus();
        apparatus.validate().unwrap();
        let json = serde_json::to_string(&apparatus).unwrap();
        let decoded: CanonicalApparatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, apparatus);
    }

    #[test]
    fn capacity_and_aas_metadata_are_validated() {
        let mut apparatus = valid_apparatus();
        apparatus.capacity.working_windows.push(WorkingWindow {
            weekday: 1,
            start_minute: 900,
            end_minute: 800,
        });
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::InvalidCapacity)
        );

        let mut apparatus = valid_apparatus();
        apparatus.capacity.working_windows.clear();
        apparatus.aas.aas_metamodel_version = "3.1.0".to_string();
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::InvalidAasMetadata)
        );

        let mut apparatus = valid_apparatus();
        apparatus.provenance.source_ref = Some(" ".to_string());
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::InvalidReference("source_ref"))
        );

        let mut apparatus = valid_apparatus();
        apparatus.aas.submodel_id = " ".to_string();
        assert_eq!(
            apparatus.validate(),
            Err(ApparatusValidationError::InvalidReference("submodel_id"))
        );
    }
}
