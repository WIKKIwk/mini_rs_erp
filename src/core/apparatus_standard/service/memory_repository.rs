use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{
    CanonicalApparatusError, CanonicalApparatusRepository, CanonicalRevisionIntent,
    CanonicalWritePermit, CommittedCanonicalApparatus, StoredCanonicalAasx, materialize_revision,
};
use crate::core::apparatus_standard::{
    ApparatusCapacityProjection, ApparatusId, ApparatusMaterialProjection,
    ApparatusQueueProjection, CanonicalAasxArtifact, CanonicalApparatusRevision, PhysicalAssetId,
    RuntimeApparatusConfiguration, RuntimeApparatusProjection, cutover::PreparedCutoverPlan,
    export_canonical_aasx, parse_canonical_aasx, project_apparatus_revision,
};

pub(super) struct MemoryCanonicalApparatusRepository {
    state: Mutex<MemoryState>,
}

#[derive(Default)]
struct MemoryState {
    entries: BTreeMap<ApparatusId, MemoryEntry>,
    physical_assets: BTreeMap<PhysicalAssetId, ApparatusId>,
    command_ids: BTreeSet<String>,
}

struct MemoryEntry {
    revision: CanonicalApparatusRevision,
    artifact: CanonicalAasxArtifact,
    runtime: RuntimeApparatusProjection,
    queue: ApparatusQueueProjection,
    material: ApparatusMaterialProjection,
    capacity: ApparatusCapacityProjection,
}

impl MemoryCanonicalApparatusRepository {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(MemoryState::default()),
        }
    }

    pub(super) fn with_revisions(
        revisions: impl IntoIterator<Item = CanonicalApparatusRevision>,
    ) -> Self {
        let mut state = MemoryState::default();
        for revision in revisions {
            let apparatus_id = revision.apparatus_id.clone();
            let artifact = export_canonical_aasx(&revision).expect("valid test AASX");
            let projections = project_apparatus_revision(&revision, artifact.sha256());
            state
                .physical_assets
                .insert(revision.physical_asset_id.clone(), apparatus_id.clone());
            state
                .command_ids
                .insert(revision.revision_metadata.command_id.clone());
            state.entries.insert(
                apparatus_id,
                MemoryEntry {
                    revision,
                    artifact,
                    runtime: projections.runtime,
                    queue: projections.queue,
                    material: projections.material,
                    capacity: projections.capacity,
                },
            );
        }
        Self {
            state: Mutex::new(state),
        }
    }
}

#[async_trait]
impl CanonicalApparatusRepository for MemoryCanonicalApparatusRepository {
    async fn cutover_preflight(
        &self,
    ) -> Result<crate::core::apparatus_standard::CutoverPreflightReport, CanonicalApparatusError>
    {
        Err(CanonicalApparatusError::CutoverBlocked(
            "legacy cutover requires the PostgreSQL repository".to_string(),
        ))
    }

    async fn commit_cutover(
        &self,
        _permit: &CanonicalWritePermit,
        _plan: PreparedCutoverPlan,
    ) -> Result<(), CanonicalApparatusError> {
        Err(CanonicalApparatusError::CutoverBlocked(
            "legacy cutover requires the PostgreSQL repository".to_string(),
        ))
    }

    async fn commit(
        &self,
        _permit: &CanonicalWritePermit,
        intent: CanonicalRevisionIntent,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        let apparatus_id = intent.apparatus_id().clone();
        let expected_revision = intent.expected_revision();
        let mut state = self.state.lock().await;
        let current = state
            .entries
            .get(&apparatus_id)
            .map(|entry| &entry.revision);
        match (expected_revision, current) {
            (None, Some(_)) => return Err(CanonicalApparatusError::AlreadyExists),
            (Some(_), None) => return Err(CanonicalApparatusError::NotFound),
            (Some(expected), Some(current)) if current.revision_metadata.revision != expected => {
                return Err(CanonicalApparatusError::RevisionConflict);
            }
            _ => {}
        }
        let (revision, _) = materialize_revision(current, intent)?;
        if state
            .command_ids
            .contains(&revision.revision_metadata.command_id)
        {
            return Err(CanonicalApparatusError::AlreadyExists);
        }
        if let Some(owner) = state.physical_assets.get(&revision.physical_asset_id)
            && owner != &apparatus_id
        {
            return Err(CanonicalApparatusError::AlreadyExists);
        }
        let artifact = export_canonical_aasx(&revision)
            .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
        if parse_canonical_aasx(artifact.bytes())
            .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?
            != revision
        {
            return Err(CanonicalApparatusError::ArtifactIntegrity);
        }
        let projections = project_apparatus_revision(&revision, artifact.sha256());
        let runtime = projections.runtime;
        let aasx_sha256 = runtime.source_aasx_sha256;
        let queue = projections.queue;
        let material = projections.material;
        let capacity = projections.capacity;
        state
            .physical_assets
            .insert(revision.physical_asset_id.clone(), apparatus_id.clone());
        state
            .command_ids
            .insert(revision.revision_metadata.command_id.clone());
        state.entries.insert(
            apparatus_id,
            MemoryEntry {
                revision: revision.clone(),
                artifact,
                runtime: runtime.clone(),
                queue,
                material,
                capacity,
            },
        );
        Ok(CommittedCanonicalApparatus {
            revision,
            runtime_projection: runtime,
            aasx_sha256,
        })
    }

    async fn current_projection(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<RuntimeApparatusProjection>, CanonicalApparatusError> {
        Ok(self
            .state
            .lock()
            .await
            .entries
            .get(apparatus_id)
            .map(|entry| entry.runtime.clone()))
    }

    async fn current_aasx(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<StoredCanonicalAasx>, CanonicalApparatusError> {
        Ok(self
            .state
            .lock()
            .await
            .entries
            .get(apparatus_id)
            .map(|entry| StoredCanonicalAasx {
                apparatus_id: apparatus_id.clone(),
                revision: entry.revision.revision_metadata.revision,
                artifact: entry.artifact.clone(),
            }))
    }

    async fn current_configuration(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<RuntimeApparatusConfiguration>, CanonicalApparatusError> {
        Ok(self
            .state
            .lock()
            .await
            .entries
            .get(apparatus_id)
            .map(|entry| RuntimeApparatusConfiguration {
                runtime: entry.runtime.clone(),
                queue: entry.queue.clone(),
                material: entry.material.clone(),
                capacity: entry.capacity.clone(),
            }))
    }

    async fn list_runtime_projections(
        &self,
    ) -> Result<Vec<RuntimeApparatusProjection>, CanonicalApparatusError> {
        Ok(self
            .state
            .lock()
            .await
            .entries
            .values()
            .map(|entry| entry.runtime.clone())
            .collect())
    }
}
