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
    #[serde(
        default,
        deserialize_with = "deserialize_placement_patch",
        skip_serializing_if = "Option::is_none"
    )]
    pub placement: Option<Option<FactoryMapPlacement>>,
    pub training: Option<TrainingProfile>,
}

// PATCH must distinguish an omitted field (keep) from JSON null (unlink).
// Serde's default Option<Option<T>> deserializer collapses both to None.
fn deserialize_placement_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<FactoryMapPlacement>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<FactoryMapPlacement>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod placement_patch_tests {
    use super::*;

    #[test]
    fn placement_patch_distinguishes_keep_unlink_and_attach() {
        for (json, expected) in [
            (serde_json::json!({}), None),
            (serde_json::json!({"placement": null}), Some(None)),
            (
                serde_json::json!({"placement": {"factory_map_object_id": "node:7"}}),
                Some(Some(FactoryMapPlacement {
                    factory_map_object_id: "node:7".into(),
                })),
            ),
        ] {
            let patch: CanonicalApparatusPatch = serde_json::from_value(json).unwrap();
            assert_eq!(patch.placement, expected);
            let encoded = serde_json::to_value(&patch).unwrap();
            assert_eq!(encoded.get("placement").is_some(), expected.is_some());
            let decoded: CanonicalApparatusPatch = serde_json::from_value(encoded).unwrap();
            assert_eq!(decoded.placement, expected);
        }
    }
}
