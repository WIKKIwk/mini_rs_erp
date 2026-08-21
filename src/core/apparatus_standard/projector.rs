//! Side-effect-free runtime projection of one canonical revision.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};

use super::{
    ApparatusCapacity, ApparatusDisplay, ApparatusId, ApparatusLifecycle,
    CanonicalApparatusRevision, CapacityAvailability, EquipmentCapabilityCode, EquipmentClassId,
    EquipmentHierarchyScope, ExecutionProfile, FactoryMapPlacement, LifecycleState,
    MaterialExecutionPolicy, PhysicalAssetId, QueueDiscipline, ToolingExecutionPolicy,
    TrainingProfile, WorkingWindowV1,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AasxSha256([u8; 32]);

impl AasxSha256 {
    pub fn digest(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    pub fn from_hex(value: &str) -> Result<Self, AasxHashParseError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AasxHashParseError);
        }
        let mut bytes = [0u8; 32];
        for (index, slot) in bytes.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| AasxHashParseError)?;
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use fmt::Write;
            write!(&mut output, "{byte:02x}").expect("writing to String is infallible");
        }
        output
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AasxHashParseError;

impl fmt::Display for AasxHashParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AASX SHA-256 must be exactly 64 hexadecimal characters")
    }
}

impl std::error::Error for AasxHashParseError {}

impl fmt::Display for AasxSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl Serialize for AasxSha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for AasxSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_hex(&value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeApparatusProjection {
    pub apparatus_id: ApparatusId,
    pub source_revision: u64,
    pub source_aasx_sha256: AasxSha256,
    pub display: ApparatusDisplay,
    pub equipment_class_id: EquipmentClassId,
    pub physical_asset_id: PhysicalAssetId,
    pub hierarchy: EquipmentHierarchyScope,
    pub capabilities: BTreeMap<EquipmentCapabilityCode, u16>,
    pub execution_profile: ExecutionProfile,
    pub placement: Option<FactoryMapPlacement>,
    pub training: TrainingProfile,
    pub lifecycle: ApparatusLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusQueueProjection {
    pub apparatus_id: ApparatusId,
    pub source_revision: u64,
    pub source_aasx_sha256: AasxSha256,
    pub discipline: QueueDiscipline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusMaterialProjection {
    pub apparatus_id: ApparatusId,
    pub source_revision: u64,
    pub source_aasx_sha256: AasxSha256,
    pub policy: MaterialExecutionPolicy,
    pub tooling: ToolingExecutionPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusCapacityProjection {
    pub apparatus_id: ApparatusId,
    pub source_revision: u64,
    pub source_aasx_sha256: AasxSha256,
    pub capacity_slots: u16,
    pub setup_minutes: u32,
    pub cleanup_minutes: u32,
    pub efficiency_percent: u16,
    pub finite_capacity: bool,
    pub always_available: bool,
    pub working_windows: Vec<WorkingWindowV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminApparatusSummary {
    pub apparatus_id: ApparatusId,
    pub source_revision: u64,
    pub source_aasx_sha256: AasxSha256,
    pub display_name: String,
    pub equipment_class_id: EquipmentClassId,
    pub physical_asset_id: PhysicalAssetId,
    pub lifecycle: ApparatusLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusProjectionSet {
    pub runtime: RuntimeApparatusProjection,
    pub queue: ApparatusQueueProjection,
    pub material: ApparatusMaterialProjection,
    pub capacity: ApparatusCapacityProjection,
    pub admin: AdminApparatusSummary,
}

/// Complete PostgreSQL runtime configuration assembled exclusively from
/// materialized projection rows. It never requires canonical payload or AASX
/// access during normal execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeApparatusConfiguration {
    pub runtime: RuntimeApparatusProjection,
    pub queue: ApparatusQueueProjection,
    pub material: ApparatusMaterialProjection,
    pub capacity: ApparatusCapacityProjection,
}

impl From<ApparatusProjectionSet> for RuntimeApparatusConfiguration {
    fn from(value: ApparatusProjectionSet) -> Self {
        Self {
            runtime: value.runtime,
            queue: value.queue,
            material: value.material,
            capacity: value.capacity,
        }
    }
}

impl RuntimeApparatusConfiguration {
    /// Proves that every row in the assembled runtime configuration came from
    /// the same canonical apparatus revision and exact AASX artifact.
    pub fn has_coherent_source(&self) -> bool {
        let id = &self.runtime.apparatus_id;
        let revision = self.runtime.source_revision;
        let hash = self.runtime.source_aasx_sha256;
        self.queue.apparatus_id == *id
            && self.material.apparatus_id == *id
            && self.capacity.apparatus_id == *id
            && self.queue.source_revision == revision
            && self.material.source_revision == revision
            && self.capacity.source_revision == revision
            && self.queue.source_aasx_sha256 == hash
            && self.material.source_aasx_sha256 == hash
            && self.capacity.source_aasx_sha256 == hash
    }

    pub fn is_active(&self) -> bool {
        self.runtime.lifecycle.state == LifecycleState::Active
    }

    pub fn supports(&self, capability: EquipmentCapabilityCode) -> bool {
        self.runtime.capabilities.contains_key(&capability)
    }
}

pub fn project_apparatus_revision(
    revision: &CanonicalApparatusRevision,
    source_aasx_sha256: AasxSha256,
) -> ApparatusProjectionSet {
    let apparatus_id = revision.apparatus_id.clone();
    let source_revision = revision.revision_metadata.revision;
    let provenance = || (apparatus_id.clone(), source_revision, source_aasx_sha256);
    let capabilities = revision
        .capabilities
        .iter()
        .map(|capability| (capability.code, capability.level))
        .collect();
    let (always_available, working_windows) = match &revision.capacity.availability {
        CapacityAvailability::Always => (true, Vec::new()),
        CapacityAvailability::Scheduled { working_windows } => (false, working_windows.clone()),
    };
    let ApparatusCapacity {
        capacity_slots,
        setup_minutes,
        cleanup_minutes,
        efficiency_percent,
        finite_capacity,
        ..
    } = revision.capacity;

    let (id, source_revision, hash) = provenance();
    let runtime = RuntimeApparatusProjection {
        apparatus_id: id,
        source_revision,
        source_aasx_sha256: hash,
        display: revision.display.clone(),
        equipment_class_id: revision.equipment_class_id.clone(),
        physical_asset_id: revision.physical_asset_id.clone(),
        hierarchy: revision.hierarchy.clone(),
        capabilities,
        execution_profile: revision.execution_profile.clone(),
        placement: revision.placement.clone(),
        training: revision.training.clone(),
        lifecycle: revision.lifecycle.clone(),
    };
    let (id, source_revision, hash) = provenance();
    let queue = ApparatusQueueProjection {
        apparatus_id: id,
        source_revision,
        source_aasx_sha256: hash,
        discipline: revision.policies.queue,
    };
    let (id, source_revision, hash) = provenance();
    let material = ApparatusMaterialProjection {
        apparatus_id: id,
        source_revision,
        source_aasx_sha256: hash,
        policy: revision.policies.material.clone(),
        tooling: revision.policies.tooling.clone(),
    };
    let (id, source_revision, hash) = provenance();
    let capacity = ApparatusCapacityProjection {
        apparatus_id: id,
        source_revision,
        source_aasx_sha256: hash,
        capacity_slots,
        setup_minutes,
        cleanup_minutes,
        efficiency_percent,
        finite_capacity,
        always_available,
        working_windows,
    };
    let (id, source_revision, hash) = provenance();
    let admin = AdminApparatusSummary {
        apparatus_id: id,
        source_revision,
        source_aasx_sha256: hash,
        display_name: revision.display.display_name.clone(),
        equipment_class_id: revision.equipment_class_id.clone(),
        physical_asset_id: revision.physical_asset_id.clone(),
        lifecycle: revision.lifecycle.clone(),
    };
    ApparatusProjectionSet {
        runtime,
        queue,
        material,
        capacity,
        admin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_standard::isa95::tests::revision_with;

    #[test]
    fn projection_is_repeatable_and_carries_revision_and_hash() {
        let revision = revision_with(
            "apparatus:test:projection-01",
            "physical-asset:projection-01",
            "Projection fixture",
        );
        let hash = AasxSha256::digest(b"stored AASX bytes");
        let first = project_apparatus_revision(&revision, hash);
        let second = project_apparatus_revision(&revision, hash);

        assert_eq!(first, second);
        assert_eq!(first.runtime.apparatus_id, revision.apparatus_id);
        assert_eq!(first.runtime.source_revision, 1);
        assert_eq!(first.runtime.source_aasx_sha256, hash);
        assert_eq!(first.queue.source_aasx_sha256, hash);
        assert_eq!(first.material.source_aasx_sha256, hash);
        assert_eq!(first.capacity.source_aasx_sha256, hash);
    }

    #[test]
    fn sha256_is_exact_and_round_trips_as_lower_hex() {
        let hash = AasxSha256::digest(b"abc");
        assert_eq!(
            hash.to_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(AasxSha256::from_hex(&hash.to_hex()).unwrap(), hash);
        assert_eq!(serde_json::to_string(&hash).unwrap(), format!("\"{hash}\""));
    }
}
