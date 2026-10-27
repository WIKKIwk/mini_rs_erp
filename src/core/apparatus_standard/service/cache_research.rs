//! Database-free coverage of the actual canonical service cache methods.
//!
//! The repository only returns synthetic, valid canonical projections. Timed
//! runs perform warmed hits through both unchanged Tokio RwLocks, generation
//! reads and Arc cloning. Repository calls, AASX creation, cache warming,
//! runtime creation and fingerprints are outside the timed region. This is a
//! single-task cache-hit workload, not a database or contention benchmark.

use super::*;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use crate::core::apparatus_standard::{
    CanonicalApparatusRevision, PhysicalAssetId, RevisionMetadata, RevisionSource,
    cutover::PreparedCutoverPlan, export_canonical_aasx, project_apparatus_revision,
};

struct ReadOnlyRepository {
    entries: RwLock<Vec<RuntimeApparatusConfiguration>>,
    runtime_reads: AtomicUsize,
    configuration_reads: AtomicUsize,
    #[cfg(test)]
    pause: Option<Arc<ReadPause>>,
}

impl ReadOnlyRepository {
    fn new(entries: Vec<RuntimeApparatusConfiguration>) -> Self {
        Self {
            entries: RwLock::new(entries),
            runtime_reads: AtomicUsize::new(0),
            configuration_reads: AtomicUsize::new(0),
            #[cfg(test)]
            pause: None,
        }
    }
}

#[async_trait]
impl CanonicalApparatusRepository for ReadOnlyRepository {
    async fn cutover_preflight(&self) -> Result<CutoverPreflightReport, CanonicalApparatusError> {
        Err(CanonicalApparatusError::Persistence)
    }

    async fn commit_cutover(
        &self,
        _permit: &CanonicalWritePermit,
        _plan: PreparedCutoverPlan,
    ) -> Result<(), CanonicalApparatusError> {
        Err(CanonicalApparatusError::Persistence)
    }

    async fn commit(
        &self,
        _permit: &CanonicalWritePermit,
        _intent: CanonicalRevisionIntent,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        Err(CanonicalApparatusError::Persistence)
    }

    async fn current_projection(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<RuntimeApparatusProjection>, CanonicalApparatusError> {
        self.runtime_reads.fetch_add(1, AtomicOrdering::Relaxed);
        let captured = self
            .entries
            .read()
            .await
            .iter()
            .find(|entry| entry.runtime.apparatus_id == *apparatus_id)
            .map(|entry| entry.runtime.clone());
        #[cfg(test)]
        if let Some(pause) = &self.pause {
            pause.after_capture(CacheKind::Runtime).await;
        }
        Ok(captured)
    }

    async fn current_aasx(
        &self,
        _apparatus_id: &ApparatusId,
    ) -> Result<Option<StoredCanonicalAasx>, CanonicalApparatusError> {
        Err(CanonicalApparatusError::Persistence)
    }

    async fn current_configuration(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<RuntimeApparatusConfiguration>, CanonicalApparatusError> {
        self.configuration_reads
            .fetch_add(1, AtomicOrdering::Relaxed);
        let captured = self
            .entries
            .read()
            .await
            .iter()
            .find(|entry| entry.runtime.apparatus_id == *apparatus_id)
            .cloned();
        #[cfg(test)]
        if let Some(pause) = &self.pause {
            pause.after_capture(CacheKind::Configuration).await;
        }
        Ok(captured)
    }

    async fn list_runtime_projections(
        &self,
    ) -> Result<Vec<RuntimeApparatusProjection>, CanonicalApparatusError> {
        Ok(self
            .entries
            .read()
            .await
            .iter()
            .map(|entry| entry.runtime.clone())
            .collect())
    }
}

fn configuration(index: usize, revision: u64) -> RuntimeApparatusConfiguration {
    let mut draft = factory_default_apparatus()
        .into_iter()
        .next()
        .expect("factory has a canonical draft")
        .draft;
    draft.display.display_name = format!("Research machine {index:04} revision {revision}");
    draft.physical_asset_id =
        PhysicalAssetId::new(format!("physical-asset:research:machine-{index:04}")).unwrap();
    let revision = CanonicalApparatusRevision::from_draft(
        ApparatusId::new(format!("apparatus:research:machine-{index:04}")).unwrap(),
        draft,
        RevisionMetadata {
            revision,
            committed_at_unix_ms: 1_800_000_000_000,
            actor_id: "user:research".into(),
            command_id: format!("command:research:machine-{index:04}-revision-{revision}"),
            source: RevisionSource::Admin,
            source_reference: None,
        },
    )
    .expect("research canonical draft is valid");
    let artifact = export_canonical_aasx(&revision).expect("research AASX is valid");
    let configuration: RuntimeApparatusConfiguration =
        project_apparatus_revision(&revision, artifact.sha256()).into();
    assert!(configuration.has_coherent_source());
    configuration
}

#[cfg(feature = "bbtree-bench")]
struct CacheHitWorkload {
    runtime: tokio::runtime::Runtime,
    service: CanonicalApparatusService,
    repository: Arc<ReadOnlyRepository>,
    ids: Vec<ApparatusId>,
    last: Option<(
        Vec<Arc<RuntimeApparatusProjection>>,
        Vec<Arc<RuntimeApparatusConfiguration>>,
    )>,
}

#[cfg(feature = "bbtree-bench")]
impl CacheHitWorkload {
    fn new(size: usize) -> Self {
        let configurations = (0..size)
            .map(|index| configuration(index, 1))
            .collect::<Vec<_>>();
        let mut ids = configurations
            .iter()
            .map(|entry| entry.runtime.apparatus_id.clone())
            .collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| {
            let index = id
                .as_str()
                .rsplit('-')
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap();
            index.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        });
        let repository = Arc::new(ReadOnlyRepository::new(configurations));
        let service = CanonicalApparatusService::new(repository.clone());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("cache research runtime");
        runtime.block_on(async {
            for id in &ids {
                service.current_projection(id).await.unwrap().unwrap();
                service.current_configuration(id).await.unwrap().unwrap();
            }
        });
        Self {
            runtime,
            service,
            repository,
            ids,
            last: None,
        }
    }
}

#[cfg(feature = "bbtree-bench")]
impl crate::core::production_map::bbtree_research::Workload for CacheHitWorkload {
    fn name(&self) -> &'static str {
        "apparatus_warmed_runtime_and_configuration"
    }

    fn size(&self) -> usize {
        self.ids.len()
    }

    fn run(&mut self) {
        self.last = Some(self.runtime.block_on(async {
            let mut projections = Vec::with_capacity(self.ids.len());
            let mut configurations = Vec::with_capacity(self.ids.len());
            for id in &self.ids {
                projections.push(self.service.current_projection(id).await.unwrap().unwrap());
                configurations.push(
                    self.service
                        .current_configuration(id)
                        .await
                        .unwrap()
                        .unwrap(),
                );
            }
            std::hint::black_box((projections, configurations))
        }));
    }

    fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        assert_eq!(
            self.repository.runtime_reads.load(AtomicOrdering::Relaxed),
            self.ids.len(),
            "every timed runtime read must hit the warmed cache"
        );
        assert_eq!(
            self.repository
                .configuration_reads
                .load(AtomicOrdering::Relaxed),
            self.ids.len(),
            "every timed configuration read must hit the warmed cache"
        );
        let (projections, configurations) = self.last.as_ref().expect("cache workload must run");
        let full_results = (
            projections.iter().map(Arc::as_ref).collect::<Vec<_>>(),
            configurations.iter().map(Arc::as_ref).collect::<Vec<_>>(),
        );
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&full_results).expect("full cache results serialize")
            )
        )
    }
}

#[cfg(feature = "bbtree-bench")]
pub(super) fn workloads() -> Vec<Box<dyn crate::core::production_map::bbtree_research::Workload>> {
    [16, 64, 128, 512]
        .into_iter()
        .map(|size| {
            Box::new(CacheHitWorkload::new(size))
                as Box<dyn crate::core::production_map::bbtree_research::Workload>
        })
        .collect()
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum CacheKind {
    Runtime,
    Configuration,
}

#[cfg(test)]
struct ReadPause {
    kind: CacheKind,
    pending: std::sync::atomic::AtomicBool,
    captured: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

#[cfg(test)]
impl ReadPause {
    fn new(kind: CacheKind) -> Self {
        Self {
            kind,
            pending: std::sync::atomic::AtomicBool::new(true),
            captured: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        }
    }

    async fn after_capture(&self, kind: CacheKind) {
        if self.kind == kind && self.pending.swap(false, AtomicOrdering::AcqRel) {
            self.captured.notify_one();
            self.resume.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bbtree_apparatus_warm_hits_preserve_arc_identity_and_both_invalidations() {
        let expected = configuration(0, 1);
        let id = expected.runtime.apparatus_id.clone();
        let repository = Arc::new(ReadOnlyRepository::new(vec![expected.clone()]));
        let service = CanonicalApparatusService::new(repository.clone());
        let first_runtime = service.current_projection(&id).await.unwrap().unwrap();
        let first_configuration = service.current_configuration(&id).await.unwrap().unwrap();
        assert_eq!(first_runtime.as_ref(), &expected.runtime);
        assert_eq!(first_configuration.as_ref(), &expected);
        assert!(Arc::ptr_eq(
            &first_runtime,
            &service.current_projection(&id).await.unwrap().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &first_configuration,
            &service.current_configuration(&id).await.unwrap().unwrap()
        ));
        assert_eq!(repository.runtime_reads.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(
            repository.configuration_reads.load(AtomicOrdering::Relaxed),
            1
        );

        repository.entries.write().await[0] = configuration(0, 2);
        service.invalidate_runtime_caches().await;
        let second_runtime = service.current_projection(&id).await.unwrap().unwrap();
        let second_configuration = service.current_configuration(&id).await.unwrap().unwrap();
        assert_eq!(second_runtime.source_revision, 2);
        assert_eq!(second_configuration.runtime.source_revision, 2);
        assert!(!Arc::ptr_eq(&first_runtime, &second_runtime));
        assert!(!Arc::ptr_eq(&first_configuration, &second_configuration));
        assert_eq!(service.cache_generation.load(Ordering::Acquire), 1);
    }

    async fn assert_in_flight_read_is_retried(kind: CacheKind) {
        let pause = Arc::new(ReadPause::new(kind));
        let expected = configuration(0, 2);
        let id = expected.runtime.apparatus_id.clone();
        let mut repository = ReadOnlyRepository::new(vec![configuration(0, 1)]);
        repository.pause = Some(pause.clone());
        let repository = Arc::new(repository);
        let service = CanonicalApparatusService::new(repository.clone());
        let pending_service = service.clone();
        let pending_id = id.clone();
        let pending = tokio::spawn(async move {
            match kind {
                CacheKind::Runtime => serde_json::to_value(
                    pending_service
                        .current_projection(&pending_id)
                        .await
                        .unwrap()
                        .unwrap()
                        .as_ref(),
                )
                .unwrap(),
                CacheKind::Configuration => serde_json::to_value(
                    pending_service
                        .current_configuration(&pending_id)
                        .await
                        .unwrap()
                        .unwrap()
                        .as_ref(),
                )
                .unwrap(),
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), pause.captured.notified())
            .await
            .expect("repository read captured its stale snapshot");
        repository.entries.write().await[0] = expected.clone();
        service.invalidate_runtime_caches().await;
        pause.resume.notify_one();
        let actual = tokio::time::timeout(std::time::Duration::from_secs(2), pending)
            .await
            .expect("in-flight lookup completes after invalidation")
            .unwrap();
        let wanted = match kind {
            CacheKind::Runtime => serde_json::to_value(&expected.runtime).unwrap(),
            CacheKind::Configuration => serde_json::to_value(&expected).unwrap(),
        };
        assert_eq!(actual, wanted);
        let reads = match kind {
            CacheKind::Runtime => &repository.runtime_reads,
            CacheKind::Configuration => &repository.configuration_reads,
        };
        assert_eq!(
            reads.load(AtomicOrdering::Relaxed),
            2,
            "stale read must retry"
        );
        let runtime = service.current_projection(&id).await.unwrap().unwrap();
        let configuration = service.current_configuration(&id).await.unwrap().unwrap();
        assert_eq!(runtime.as_ref(), &expected.runtime);
        assert_eq!(configuration.as_ref(), &expected);
        assert_eq!(
            reads.load(AtomicOrdering::Relaxed),
            2,
            "second read stays cached"
        );
    }

    #[tokio::test]
    async fn bbtree_apparatus_in_flight_runtime_read_cannot_restore_old_generation() {
        assert_in_flight_read_is_retried(CacheKind::Runtime).await;
    }

    #[tokio::test]
    async fn bbtree_apparatus_in_flight_configuration_read_cannot_restore_old_generation() {
        assert_in_flight_read_is_retried(CacheKind::Configuration).await;
    }
}
