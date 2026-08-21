use sqlx::{PgPool, Row};

use crate::core::apparatus_standard::{
    AasxSha256, ApparatusCapacityProjection, ApparatusId, ApparatusMaterialProjection,
    ApparatusQueueProjection, CanonicalAasxArtifact, CanonicalApparatusError,
    CanonicalApparatusRevision, RuntimeApparatusConfiguration, RuntimeApparatusProjection,
    StoredCanonicalAasx, parse_canonical_aasx,
};

pub(super) async fn current_projection(
    pool: &PgPool,
    apparatus_id: &ApparatusId,
) -> Result<Option<RuntimeApparatusProjection>, CanonicalApparatusError> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_apparatus
         WHERE id = $1 AND source_revision IS NOT NULL",
    )
    .bind(apparatus_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    payload.map(parse_projection).transpose()
}

pub(super) async fn list_runtime_projections(
    pool: &PgPool,
) -> Result<Vec<RuntimeApparatusProjection>, CanonicalApparatusError> {
    sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_apparatus
         WHERE source_revision IS NOT NULL
         ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?
    .into_iter()
    .map(parse_projection)
    .collect()
}

pub(super) async fn current_aasx(
    pool: &PgPool,
    apparatus_id: &ApparatusId,
) -> Result<Option<StoredCanonicalAasx>, CanonicalApparatusError> {
    let row = sqlx::query(
        "SELECT revision.revision, revision.canonical_payload,
                revision.aasx_package, revision.aasx_sha256
         FROM mini_canonical_apparatus_heads head
         JOIN mini_canonical_apparatus_revisions revision
           ON revision.apparatus_id = head.apparatus_id
          AND revision.revision = head.current_revision
          AND revision.aasx_sha256 = head.current_aasx_sha256
         WHERE head.apparatus_id = $1",
    )
    .bind(apparatus_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let revision = u64::try_from(row.get::<i64, _>("revision"))
        .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
    let bytes = row.get::<Vec<u8>, _>("aasx_package");
    let hash = AasxSha256::from_hex(row.get::<String, _>("aasx_sha256").as_str())
        .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
    let artifact = CanonicalAasxArtifact::from_stored(bytes, hash)
        .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
    let parsed = parse_canonical_aasx(artifact.bytes())
        .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
    let authoritative =
        serde_json::from_value::<CanonicalApparatusRevision>(row.get("canonical_payload"))
            .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
    authoritative
        .validate()
        .map_err(|_| CanonicalApparatusError::ArtifactIntegrity)?;
    if parsed != authoritative
        || parsed.apparatus_id != *apparatus_id
        || parsed.revision_metadata.revision != revision
    {
        return Err(CanonicalApparatusError::ArtifactIntegrity);
    }
    Ok(Some(StoredCanonicalAasx {
        apparatus_id: apparatus_id.clone(),
        revision,
        artifact,
    }))
}

pub(super) async fn current_configuration(
    pool: &PgPool,
    apparatus_id: &ApparatusId,
) -> Result<Option<RuntimeApparatusConfiguration>, CanonicalApparatusError> {
    let row = sqlx::query(
        "SELECT runtime.payload_json AS runtime_payload,
                queue.payload_json AS queue_payload,
                material.payload_json AS material_payload,
                capacity.payload_json AS capacity_payload
         FROM mini_apparatus runtime
         JOIN mini_apparatus_queue_policies queue
           ON queue.canonical_apparatus_id = runtime.id
          AND (queue.source_revision, queue.source_aasx_sha256)
              = (runtime.source_revision, runtime.source_aasx_sha256)
         JOIN mini_apparatus_material_rules material
           ON material.canonical_apparatus_id = runtime.id
          AND (material.source_revision, material.source_aasx_sha256)
              = (runtime.source_revision, runtime.source_aasx_sha256)
         JOIN mini_apparatus_capacity_profiles capacity
           ON capacity.canonical_apparatus_id = runtime.id
          AND (capacity.source_revision, capacity.source_aasx_sha256)
              = (runtime.source_revision, runtime.source_aasx_sha256)
         WHERE runtime.id = $1 AND runtime.source_revision IS NOT NULL",
    )
    .bind(apparatus_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    row.map(|row| {
        Ok(RuntimeApparatusConfiguration {
            runtime: parse_json(row.get("runtime_payload"))?,
            queue: parse_json::<ApparatusQueueProjection>(row.get("queue_payload"))?,
            material: parse_json::<ApparatusMaterialProjection>(row.get("material_payload"))?,
            capacity: parse_json::<ApparatusCapacityProjection>(row.get("capacity_payload"))?,
        })
    })
    .transpose()
}

fn parse_projection(
    payload: serde_json::Value,
) -> Result<RuntimeApparatusProjection, CanonicalApparatusError> {
    serde_json::from_value(payload).map_err(|_| CanonicalApparatusError::ArtifactIntegrity)
}

fn parse_json<T: serde::de::DeserializeOwned>(
    payload: serde_json::Value,
) -> Result<T, CanonicalApparatusError> {
    serde_json::from_value(payload).map_err(|_| CanonicalApparatusError::ArtifactIntegrity)
}
