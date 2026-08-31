//! Apparatus-focused ISA-95 / IEC 62264 profile.
//!
//! The profile intentionally models configuration only. Orders, queue
//! positions, WIP, downtime occurrences, workers, and other live execution
//! state are PostgreSQL runtime data referencing [`ApparatusId`].

mod identifiers;
mod model;
mod validation;

pub use identifiers::{EquipmentClassId, HierarchyLevelId, PhysicalAssetId};
pub use model::{
    AasIdentity, ApparatusCapacity, ApparatusDisplay, ApparatusLifecycle,
    ApparatusOperationalPolicies, CanonicalApparatusDraft, CanonicalApparatusRevision,
    CapacityAvailability, EquipmentCapability, EquipmentCapabilityCode, EquipmentHierarchyScope,
    ExecutionOperation, ExecutionProfile, FactoryMapPlacement, LifecycleState,
    MaterialExecutionPolicy, MaterialRequirementSet, ProcessTechnology, QueueDiscipline,
    RevisionMetadata, RevisionSource, ToolingExecutionPolicy, TrainingProfile, VirtualTaskPolicy,
    WorkingWindowV1,
};
pub use validation::CanonicalApparatusValidationError;

pub const CANONICAL_APPARATUS_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
pub(crate) mod tests;
