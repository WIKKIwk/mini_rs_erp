use async_trait::async_trait;
use sqlx::{Postgres, Transaction};

use super::{CommitFaultPoint, PostgresCanonicalApparatusRepository, projections};
use crate::core::apparatus_standard::service::{
    CanonicalApparatusRepository, CanonicalRevisionIntent, CanonicalWritePermit,
};
use crate::core::apparatus_standard::{
    CanonicalApparatusError, CanonicalApparatusRevision, CommittedCanonicalApparatus,
    export_canonical_aasx, parse_canonical_aasx, project_apparatus_revision,
};

#[async_trait]
impl CanonicalApparatusRepository for PostgresCanonicalApparatusRepository {
    async fn cutover_preflight(
        &self,
    ) -> Result<crate::core::apparatus_standard::CutoverPreflightReport, CanonicalApparatusError>
    {
        super::cutover::collect_from_pool(&self.pool).await
    }

    async fn commit_cutover(
        &self,
        _permit: &CanonicalWritePermit,
        plan: crate::core::apparatus_standard::cutover::PreparedCutoverPlan,
    ) -> Result<(), CanonicalApparatusError> {
        super::cutover::commit_plan(&self.pool, plan).await
    }

    async fn commit(
        &self,
        _permit: &CanonicalWritePermit,
        intent: CanonicalRevisionIntent,
    ) -> Result<CommittedCanonicalApparatus, CanonicalApparatusError> {
        let apparatus_id = intent.apparatus_id().clone();
        let expected_revision = intent.expected_revision();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| CanonicalApparatusError::Persistence)?;
        sqlx::query("SELECT set_config('mini_rs_erp.canonical_writer', 'on', true)")
            .execute(&mut *tx)
            .await
            .map_err(|_| CanonicalApparatusError::Persistence)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("canonical-apparatus:{}", apparatus_id.as_str()))
            .execute(&mut *tx)
            .await
            .map_err(|_| CanonicalApparatusError::Persistence)?;
        let current = lock_current_revision(&mut tx, &apparatus_id).await?;
        self.fault_at(CommitFaultPoint::HeadLock)?;
        match (expected_revision, current.as_ref()) {
            (None, Some(_)) => return Err(CanonicalApparatusError::AlreadyExists),
            (Some(_), None) => return Err(CanonicalApparatusError::NotFound),
            (Some(expected), Some(current)) if current.revision_metadata.revision != expected => {
                return Err(CanonicalApparatusError::RevisionConflict);
            }
            _ => {}
        }
        self.fault_at(CommitFaultPoint::ExpectedRevision)?;

        let is_create = current.is_none();
        if is_create && identity_exists(&mut tx, &apparatus_id).await? {
            return Err(CanonicalApparatusError::AlreadyExists);
        }
        let (revision, event_type) =
            crate::core::apparatus_standard::service::materialize_revision(
                current.as_ref(),
                intent,
            )?;
        revision.validate()?;
        self.fault_at(CommitFaultPoint::CandidateValidation)?;
        let artifact = export_canonical_aasx(&revision)
            .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
        if parse_canonical_aasx(artifact.bytes())
            .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?
            != revision
        {
            return Err(CanonicalApparatusError::ArtifactIntegrity);
        }
        self.fault_at(CommitFaultPoint::ArtifactGeneration)?;
        let projection = project_apparatus_revision(&revision, artifact.sha256());
        self.fault_at(CommitFaultPoint::Projection)?;

        if is_create {
            insert_identity(&mut tx, &revision).await?;
        }
        self.fault_at(CommitFaultPoint::IdentityInsert)?;
        insert_revision(
            &mut tx,
            &revision,
            artifact.bytes(),
            artifact.sha256().to_hex(),
        )
        .await?;
        self.fault_at(CommitFaultPoint::RevisionInsert)?;
        cas_head(
            &mut tx,
            &revision,
            expected_revision,
            artifact.sha256().to_hex(),
        )
        .await?;
        self.fault_at(CommitFaultPoint::HeadCas)?;
        projections::write_runtime_projection(&mut tx, &revision, &projection.runtime).await?;
        self.fault_at(CommitFaultPoint::RuntimeProjection)?;
        projections::write_derived_projections(&mut tx, &revision, &projection).await?;
        self.fault_at(CommitFaultPoint::DerivedProjections)?;
        insert_outbox(&mut tx, &revision, event_type, &projection.runtime).await?;
        self.fault_at(CommitFaultPoint::Outbox)?;
        tx.commit()
            .await
            .map_err(|_| CanonicalApparatusError::Persistence)?;

        Ok(CommittedCanonicalApparatus {
            revision,
            runtime_projection: projection.runtime,
            aasx_sha256: artifact.sha256(),
        })
    }

    async fn current_projection(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<
        Option<crate::core::apparatus_standard::RuntimeApparatusProjection>,
        CanonicalApparatusError,
    > {
        super::reads::current_projection(&self.pool, apparatus_id).await
    }

    async fn current_aasx(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<
        Option<crate::core::apparatus_standard::service::StoredCanonicalAasx>,
        CanonicalApparatusError,
    > {
        super::reads::current_aasx(&self.pool, apparatus_id).await
    }

    async fn current_configuration(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<
        Option<crate::core::apparatus_standard::RuntimeApparatusConfiguration>,
        CanonicalApparatusError,
    > {
        super::reads::current_configuration(&self.pool, apparatus_id).await
    }

    async fn list_runtime_projections(
        &self,
    ) -> Result<
        Vec<crate::core::apparatus_standard::RuntimeApparatusProjection>,
        CanonicalApparatusError,
    > {
        super::reads::list_runtime_projections(&self.pool).await
    }
}

async fn lock_current_revision(
    tx: &mut Transaction<'_, Postgres>,
    apparatus_id: &crate::core::apparatus_standard::ApparatusId,
) -> Result<Option<CanonicalApparatusRevision>, CanonicalApparatusError> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT revision.canonical_payload
         FROM mini_canonical_apparatus_heads head
         JOIN mini_canonical_apparatus_revisions revision
           ON revision.apparatus_id = head.apparatus_id
          AND revision.revision = head.current_revision
          AND revision.aasx_sha256 = head.current_aasx_sha256
         WHERE head.apparatus_id = $1
         FOR UPDATE OF head",
    )
    .bind(apparatus_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    payload
        .map(|payload| {
            let revision = serde_json::from_value::<CanonicalApparatusRevision>(payload)
                .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
            revision.validate()?;
            Ok(revision)
        })
        .transpose()
}

async fn identity_exists(
    tx: &mut Transaction<'_, Postgres>,
    apparatus_id: &crate::core::apparatus_standard::ApparatusId,
) -> Result<bool, CanonicalApparatusError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM mini_canonical_apparatus_identities WHERE apparatus_id = $1
         )",
    )
    .bind(apparatus_id.as_str())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)
}

pub(super) async fn insert_identity(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
) -> Result<(), CanonicalApparatusError> {
    sqlx::query(
        "INSERT INTO mini_canonical_apparatus_identities (
             apparatus_id, physical_asset_id, aas_shell_id, aas_submodel_id
         ) VALUES ($1, $2, $3, $4)",
    )
    .bind(revision.apparatus_id.as_str())
    .bind(revision.physical_asset_id.as_str())
    .bind(&revision.aas_identity.shell_id)
    .bind(&revision.aas_identity.submodel_id)
    .execute(&mut **tx)
    .await
    .map_err(map_unique_or_persistence)?;
    Ok(())
}

pub(super) async fn insert_revision(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    aasx_package: &[u8],
    aasx_sha256: String,
) -> Result<(), CanonicalApparatusError> {
    let payload =
        serde_json::to_value(revision).map_err(|_| CanonicalApparatusError::Persistence)?;
    let source = enum_name(&revision.revision_metadata.source)?;
    let revision_number = i64::try_from(revision.revision_metadata.revision)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let schema_version =
        i32::try_from(revision.schema_version).map_err(|_| CanonicalApparatusError::Persistence)?;
    sqlx::query(
        "INSERT INTO mini_canonical_apparatus_revisions (
             apparatus_id, revision, schema_version, canonical_payload,
             aasx_package, aasx_sha256, equipment_class_id, physical_asset_id,
             aas_shell_id, aas_submodel_id, aas_semantic_id, lifecycle_state,
             committed_at_unix_ms, actor_id, command_id, revision_source,
             source_reference
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
             $13, $14, $15, $16, $17
         )",
    )
    .bind(revision.apparatus_id.as_str())
    .bind(revision_number)
    .bind(schema_version)
    .bind(payload)
    .bind(aasx_package)
    .bind(aasx_sha256)
    .bind(revision.equipment_class_id.as_str())
    .bind(revision.physical_asset_id.as_str())
    .bind(&revision.aas_identity.shell_id)
    .bind(&revision.aas_identity.submodel_id)
    .bind(&revision.aas_identity.semantic_id)
    .bind(enum_name(&revision.lifecycle.state)?)
    .bind(revision.revision_metadata.committed_at_unix_ms)
    .bind(&revision.revision_metadata.actor_id)
    .bind(&revision.revision_metadata.command_id)
    .bind(source)
    .bind(&revision.revision_metadata.source_reference)
    .execute(&mut **tx)
    .await
    .map_err(map_unique_or_persistence)?;
    Ok(())
}

pub(super) async fn cas_head(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    expected_revision: Option<u64>,
    aasx_sha256: String,
) -> Result<(), CanonicalApparatusError> {
    let revision_number = i64::try_from(revision.revision_metadata.revision)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let affected = if let Some(expected) = expected_revision {
        sqlx::query(
            "UPDATE mini_canonical_apparatus_heads
             SET current_revision = $2, current_aasx_sha256 = $3, updated_at = now()
             WHERE apparatus_id = $1 AND current_revision = $4",
        )
        .bind(revision.apparatus_id.as_str())
        .bind(revision_number)
        .bind(aasx_sha256)
        .bind(i64::try_from(expected).map_err(|_| CanonicalApparatusError::RevisionConflict)?)
        .execute(&mut **tx)
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?
        .rows_affected()
    } else {
        sqlx::query(
            "INSERT INTO mini_canonical_apparatus_heads (
                 apparatus_id, current_revision, current_aasx_sha256
             ) VALUES ($1, $2, $3)",
        )
        .bind(revision.apparatus_id.as_str())
        .bind(revision_number)
        .bind(aasx_sha256)
        .execute(&mut **tx)
        .await
        .map_err(map_unique_or_persistence)?
        .rows_affected()
    };
    if affected != 1 {
        return Err(CanonicalApparatusError::RevisionConflict);
    }
    Ok(())
}

pub(super) async fn insert_outbox(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    event_type: &str,
    runtime: &crate::core::apparatus_standard::RuntimeApparatusProjection,
) -> Result<(), CanonicalApparatusError> {
    let revision_number = i64::try_from(revision.revision_metadata.revision)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let event_id = format!("apparatus-change:{}", revision.revision_metadata.command_id);
    let payload = serde_json::json!({
        "apparatus_id": revision.apparatus_id,
        "revision": revision.revision_metadata.revision,
        "aasx_sha256": runtime.source_aasx_sha256,
        "lifecycle_state": revision.lifecycle.state,
    });
    sqlx::query(
        "INSERT INTO mini_canonical_apparatus_change_outbox (
             event_id, apparatus_id, revision, event_type, event_payload
         ) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(event_id)
    .bind(revision.apparatus_id.as_str())
    .bind(revision_number)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(map_unique_or_persistence)?;
    Ok(())
}

fn enum_name<T: serde::Serialize>(value: &T) -> Result<String, CanonicalApparatusError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or(CanonicalApparatusError::Persistence)
}

fn map_unique_or_persistence(error: sqlx::Error) -> CanonicalApparatusError {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
            CanonicalApparatusError::AlreadyExists
        }
        _ => CanonicalApparatusError::Persistence,
    }
}
