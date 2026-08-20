//! Sole write owner for canonical apparatus revisions and projections.

mod commands;
mod error;
mod repository;

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
    RuntimeApparatusProjection, canonicalize_uploaded_aasx,
};

#[derive(Clone)]
pub struct CanonicalApparatusService {
    repository: Arc<dyn CanonicalApparatusRepository>,
    runtime_cache: Arc<RwLock<BTreeMap<ApparatusId, Arc<RuntimeApparatusProjection>>>>,
}

impl CanonicalApparatusService {
    pub(crate) fn new(repository: Arc<dyn CanonicalApparatusRepository>) -> Self {
        Self {
            repository,
            runtime_cache: Arc::new(RwLock::new(BTreeMap::new())),
        }
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

    pub async fn list_runtime_projections(
        &self,
    ) -> Result<Vec<RuntimeApparatusProjection>, CanonicalApparatusError> {
        self.repository.list_runtime_projections().await
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
