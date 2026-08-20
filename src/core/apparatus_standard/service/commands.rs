use serde::{Deserialize, Serialize};

use super::super::{
    ApparatusCapacity, ApparatusDisplay, ApparatusOperationalPolicies, EquipmentCapability,
    EquipmentClassId, EquipmentHierarchyScope, ExecutionProfile, FactoryMapPlacement,
    TrainingProfile,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalCommandMetadata {
    pub actor_id: String,
    pub command_id: String,
    #[serde(skip)]
    pub(crate) committed_at_unix_ms: i64,
    #[serde(skip)]
    pub(crate) source_reference: Option<String>,
}

impl CanonicalCommandMetadata {
    pub fn new(actor_id: impl Into<String>, command_id: impl Into<String>) -> Self {
        Self {
            actor_id: actor_id.into(),
            command_id: command_id.into(),
            committed_at_unix_ms: 0,
            source_reference: None,
        }
    }

    pub(crate) fn with_timestamp(mut self, value: i64) -> Self {
        self.committed_at_unix_ms = value;
        self
    }

    pub(crate) fn with_source_reference(mut self, value: Option<String>) -> Self {
        self.source_reference = value;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CanonicalApparatusPatch {
    pub display: Option<ApparatusDisplay>,
    pub equipment_class_id: Option<EquipmentClassId>,
    pub hierarchy: Option<EquipmentHierarchyScope>,
    pub capabilities: Option<Vec<EquipmentCapability>>,
    pub execution_profile: Option<ExecutionProfile>,
    pub policies: Option<ApparatusOperationalPolicies>,
    pub capacity: Option<ApparatusCapacity>,
    pub placement: Option<Option<FactoryMapPlacement>>,
    pub training: Option<TrainingProfile>,
}
