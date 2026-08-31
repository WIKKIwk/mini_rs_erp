//! Sole write owner for canonical apparatus revisions and projections.

mod commands;
mod error;
#[cfg(any(test, feature = "verification"))]
mod memory_repository;
mod repository;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;

pub use commands::{CanonicalApparatusPatch, CanonicalCommandMetadata};
pub use error::CanonicalApparatusError;
pub(crate) use repository::{
    CanonicalApparatusRepository, CanonicalRevisionIntent, CanonicalWritePermit,
    materialize_revision,
};
pub use repository::{CommittedCanonicalApparatus, StoredCanonicalAasx};

use super::{
    ApparatusId, CanonicalAasxImportError, CanonicalApparatusDraft, CanonicalizedAasxUpload,
    CutoverPreflightReport, LegacyCutoverManifest, ResolvedCutoverManifest,
    RuntimeApparatusConfiguration, RuntimeApparatusProjection, canonicalize_uploaded_aasx,
    cutover::prepare_cutover,
    factory_defaults::{
        FACTORY_DEFAULT_COMMITTED_AT_UNIX_MS, factory_default_apparatus,
        flexo_default_execution_profile_upgrade, is_flexo_default_execution_profile,
    },
};

#[derive(Clone)]
pub struct CanonicalApparatusService {
    repository: Arc<dyn CanonicalApparatusRepository>,
    runtime_cache: Arc<RwLock<BTreeMap<ApparatusId, Arc<RuntimeApparatusProjection>>>>,
    configuration_cache: Arc<RwLock<BTreeMap<ApparatusId, Arc<RuntimeApparatusConfiguration>>>>,
}

impl CanonicalApparatusService {
    pub(crate) fn new(repository: Arc<dyn CanonicalApparatusRepository>) -> Self {
        Self {
            repository,
            runtime_cache: Arc::new(RwLock::new(BTreeMap::new())),
            configuration_cache: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    #[cfg(any(test, feature = "verification"))]
    pub(crate) fn memory() -> Self {
        Self::new(Arc::new(
            memory_repository::MemoryCanonicalApparatusRepository::new(),
        ))
    }

    #[cfg(test)]
    pub(crate) fn memory_with_standard_test_apparatus() -> Self {
        Self::new(Arc::new(
            memory_repository::MemoryCanonicalApparatusRepository::with_revisions(
                super::test_support::standard_revisions(),
            ),
        ))
    }

    #[cfg(test)]
    pub(crate) async fn seed_for_test(
        &self,
        apparatus_id: ApparatusId,
        draft: CanonicalApparatusDraft,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        let command_id = format!("command:test-seed:{}", apparatus_id.as_str());
        self.commit(CanonicalRevisionIntent::Create {
            apparatus_id,
            draft,
            metadata: CanonicalCommandMetadata::new("user:test", command_id)
                .with_timestamp(1_800_000_000_000),
        })
        .await
    }

    pub async fn create(
        &self,
        draft: CanonicalApparatusDraft,
        metadata: CanonicalCommandMetadata,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        let apparatus_id = opaque_apparatus_id();
        self.commit(CanonicalRevisionIntent::Create {
            apparatus_id,
            draft,
            metadata: metadata.with_timestamp(now_unix_ms()?),
        })
        .await
    }

    pub async fn bootstrap_factory_defaults(&self) -> Result<usize, CanonicalApparatusError> {
        let defaults = factory_default_apparatus();
        let current = self.repository.list_runtime_projections().await?;
        let current_by_id = current
            .into_iter()
            .map(|projection| (projection.apparatus_id.clone(), projection))
            .collect::<BTreeMap<_, _>>();
        let current_ids = current_by_id
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let default_ids = defaults
            .iter()
            .map(|default| default.apparatus_id.clone())
            .collect::<std::collections::BTreeSet<_>>();

        if !current_ids.is_empty() && current_ids.is_disjoint(&default_ids) {
            return Ok(0);
        }

        let mut changed = 0;
        for default in defaults {
            if let Some(current) = current_by_id.get(&default.apparatus_id) {
                if let Some(execution_profile) = flexo_default_execution_profile_upgrade(
                    &current.apparatus_id,
                    &current.execution_profile,
                ) {
                    let result = self
                        .patch(
                            current.apparatus_id.clone(),
                            current.source_revision,
                            CanonicalApparatusPatch {
                                execution_profile: Some(execution_profile),
                                ..CanonicalApparatusPatch::default()
                            },
                            CanonicalCommandMetadata::new(
                                "system:factory-default-bootstrap",
                                "command:factory-default-upgrade:flexo-order-limits-v1",
                            ),
                        )
                        .await;
                    match result {
                        Ok(_) => changed += 1,
                        Err(error)
                            if matches!(
                                error,
                                CanonicalApparatusError::RevisionConflict
                                    | CanonicalApparatusError::AlreadyExists
                            ) =>
                        {
                            let refreshed = self
                                .repository
                                .current_projection(&current.apparatus_id)
                                .await?;
                            if !refreshed.is_some_and(|projection| {
                                is_flexo_default_execution_profile(
                                    &projection.apparatus_id,
                                    &projection.execution_profile,
                                )
                            }) {
                                return Err(error);
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
                continue;
            }
            let command_id = format!(
                "command:factory-default-bootstrap:{}",
                default.apparatus_id.as_str().replace(':', "-")
            );
            let intent = CanonicalRevisionIntent::Create {
                apparatus_id: default.apparatus_id.clone(),
                draft: default.draft,
                metadata: CanonicalCommandMetadata::new(
                    "system:factory-default-bootstrap",
                    command_id,
                )
                .with_timestamp(FACTORY_DEFAULT_COMMITTED_AT_UNIX_MS),
            };
            match self.commit(intent).await {
                Ok(_) => changed += 1,
                Err(CanonicalApparatusError::AlreadyExists)
                    if self
                        .repository
                        .current_projection(&default.apparatus_id)
                        .await?
                        .is_some() => {}
                Err(error) => return Err(error),
            }
        }
        Ok(changed)
    }

    pub async fn update(
        &self,
        apparatus_id: ApparatusId,
        expected_revision: u64,
        draft: CanonicalApparatusDraft,
        metadata: CanonicalCommandMetadata,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        self.commit(CanonicalRevisionIntent::Update {
            apparatus_id,
            expected_revision,
            draft,
            metadata: metadata.with_timestamp(now_unix_ms()?),
        })
        .await
    }

    pub async fn patch(
        &self,
        apparatus_id: ApparatusId,
        expected_revision: u64,
        patch: CanonicalApparatusPatch,
        metadata: CanonicalCommandMetadata,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        self.commit(CanonicalRevisionIntent::Patch {
            apparatus_id,
            expected_revision,
            patch,
            metadata: metadata.with_timestamp(now_unix_ms()?),
        })
        .await
    }

    pub async fn replace_from_aasx(
        &self,
        apparatus_id: ApparatusId,
        expected_revision: u64,
        uploaded: &[u8],
        metadata: CanonicalCommandMetadata,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        let CanonicalizedAasxUpload {
            revision: imported,
            canonical_artifact: _,
        } = canonicalize_uploaded_aasx(uploaded).map_err(map_import_error)?;
        if imported.apparatus_id != apparatus_id {
            return Err(CanonicalApparatusError::IdentityConflict);
        }
        let source_reference = Some(format!("sha256:{}", super::AasxSha256::digest(uploaded)));
        self.commit(CanonicalRevisionIntent::ReplaceFromAasx {
            apparatus_id,
            expected_revision,
            draft: imported.to_draft(),
            metadata: metadata
                .with_source_reference(source_reference)
                .with_timestamp(now_unix_ms()?),
        })
        .await
    }

    pub async fn retire(
        &self,
        apparatus_id: ApparatusId,
        expected_revision: u64,
        retirement_reason: String,
        metadata: CanonicalCommandMetadata,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        self.commit(CanonicalRevisionIntent::Retire {
            apparatus_id,
            expected_revision,
            retirement_reason,
            metadata: metadata.with_timestamp(now_unix_ms()?),
        })
        .await
    }

    pub async fn current_projection(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusProjection>>, CanonicalApparatusError> {
        if let Some(cached) = self.runtime_cache.read().await.get(apparatus_id).cloned() {
            return Ok(Some(cached));
        }
        let Some(projection) = self.repository.current_projection(apparatus_id).await? else {
            return Ok(None);
        };
        let projection = Arc::new(projection);
        self.runtime_cache
            .write()
            .await
            .insert(apparatus_id.clone(), projection.clone());
        Ok(Some(projection))
    }

    pub async fn current_aasx(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<StoredCanonicalAasx>, CanonicalApparatusError> {
        self.repository.current_aasx(apparatus_id).await
    }

    pub async fn current_configuration(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<Arc<RuntimeApparatusConfiguration>>, CanonicalApparatusError> {
        if let Some(cached) = self
            .configuration_cache
            .read()
            .await
            .get(apparatus_id)
            .cloned()
        {
            return Ok(Some(cached));
        }
        let Some(configuration) = self.repository.current_configuration(apparatus_id).await? else {
            return Ok(None);
        };
        let configuration = Arc::new(configuration);
        self.configuration_cache
            .write()
            .await
            .insert(apparatus_id.clone(), configuration.clone());
        Ok(Some(configuration))
    }

    pub async fn list_runtime_configurations(
        &self,
    ) -> Result<Vec<RuntimeApparatusConfiguration>, CanonicalApparatusError> {
        let runtimes = self.repository.list_runtime_projections().await?;
        let mut configurations = Vec::with_capacity(runtimes.len());
        for runtime in runtimes {
            let configuration = self
                .current_configuration(&runtime.apparatus_id)
                .await?
                .ok_or(CanonicalApparatusError::ArtifactIntegrity)?;
            configurations.push(configuration.as_ref().clone());
        }
        Ok(configurations)
    }

    pub async fn list_runtime_projections(
        &self,
    ) -> Result<Vec<RuntimeApparatusProjection>, CanonicalApparatusError> {
        self.repository.list_runtime_projections().await
    }

    pub async fn cutover_preflight(
        &self,
    ) -> Result<CutoverPreflightReport, CanonicalApparatusError> {
        self.repository.cutover_preflight().await
    }

    pub async fn preview_legacy_cutover(
        &self,
        report: &CutoverPreflightReport,
        manifest: LegacyCutoverManifest,
    ) -> Result<ResolvedCutoverManifest, CanonicalApparatusError> {
        let (_, resolved) = prepare_cutover(report, manifest)?;
        Ok(resolved)
    }

    pub async fn apply_legacy_cutover(
        &self,
        manifest: LegacyCutoverManifest,
    ) -> Result<ResolvedCutoverManifest, CanonicalApparatusError> {
        let report = self.repository.cutover_preflight().await?;
        let (plan, resolved) = prepare_cutover(&report, manifest)?;
        let ids = plan
            .entries
            .iter()
            .map(|entry| entry.revision.apparatus_id.clone())
            .collect::<Vec<_>>();
        let permit = CanonicalWritePermit::new();
        self.repository.commit_cutover(&permit, plan).await?;
        let mut runtime_cache = self.runtime_cache.write().await;
        let mut configuration_cache = self.configuration_cache.write().await;
        for apparatus_id in ids {
            runtime_cache.remove(&apparatus_id);
            configuration_cache.remove(&apparatus_id);
        }
        Ok(resolved)
    }

    async fn commit(
        &self,
        intent: CanonicalRevisionIntent,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        let apparatus_id = intent.apparatus_id().clone();
        let permit = CanonicalWritePermit::new();
        let committed = self.repository.commit(&permit, intent).await?;
        // The repository returns only after COMMIT. Cache mutation before this
        // point would expose a projection whose transaction may still roll back.
        self.runtime_cache.write().await.remove(&apparatus_id);
        self.configuration_cache.write().await.remove(&apparatus_id);
        Ok(committed)
    }
}

fn opaque_apparatus_id() -> ApparatusId {
    let bytes = rand::random::<[u8; 16]>();
    let mut key = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut key, "{byte:02x}").expect("writing to String is infallible");
    }
    ApparatusId::new(format!("apparatus:generated:{key}"))
        .expect("generated apparatus IDs satisfy the canonical shape")
}

fn now_unix_ms() -> Result<i64, CanonicalApparatusError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CanonicalApparatusError::Clock)?;
    i64::try_from(duration.as_millis()).map_err(|_| CanonicalApparatusError::Clock)
}

fn map_import_error(_error: CanonicalAasxImportError) -> CanonicalApparatusError {
    CanonicalApparatusError::InvalidAasx
}
