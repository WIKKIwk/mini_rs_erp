//! Canonical apparatus contract and deterministic AASX representation.

pub mod aasx;
pub mod canonical_aasx;
mod identity;
pub mod isa95;
pub mod projector;

pub use canonical_aasx::{
    CanonicalAasxArtifact, CanonicalAasxExportError, CanonicalAasxImportError,
    CanonicalizedAasxUpload, canonicalize_uploaded_aasx, export_canonical_aasx,
    parse_canonical_aasx,
};
pub use identity::{ApparatusId, ApparatusIdError};
pub use isa95::{
    AasIdentity, ApparatusCapacity, ApparatusDisplay, ApparatusLifecycle,
    ApparatusOperationalPolicies, CANONICAL_APPARATUS_SCHEMA_VERSION, CanonicalApparatusDraft,
    CanonicalApparatusRevision, CapacityAvailability, EquipmentCapability, EquipmentCapabilityCode,
    EquipmentClassId, EquipmentHierarchyScope, ExecutionOperation, ExecutionProfile,
    FactoryMapPlacement, HierarchyLevelId, LifecycleState, MaterialExecutionPolicy,
    MaterialRequirementSet, PhysicalAssetId, ProcessTechnology, QueueDiscipline, RevisionMetadata,
    RevisionSource, ToolingExecutionPolicy, TrainingProfile, VirtualTaskPolicy, WorkingWindowV1,
};
pub use projector::{
    AasxSha256, AdminApparatusSummary, ApparatusCapacityProjection, ApparatusMaterialProjection,
    ApparatusProjectionSet, ApparatusQueueProjection, RuntimeApparatusConfiguration,
    RuntimeApparatusProjection, project_apparatus_revision,
};

pub const IDTA_RELEASE: &str = "26-01";
pub const AAS_METAMODEL_VERSION: &str = "3.2.0";
pub const AASX_PART_5_VERSION: &str = "IDTA-01005 v3.2";
pub const AASX_PACKAGE_FORMAT: &str = "Open Packaging Conventions";
pub const AASX_MEDIA_TYPE: &str = "application/asset-administration-shell-package";

/// Project-owned semantic target. It is not an IDTA-issued semantic ID.
pub const AAS_APPARATUS_SUBMODEL_SEMANTIC_ID: &str =
    "urn:mini-rs-erp:semantic-id:submodel:apparatus:1";
pub const AAS_APPARATUS_SUBMODEL_ID_PREFIX: &str = "urn:mini-rs-erp:submodel:apparatus:";
