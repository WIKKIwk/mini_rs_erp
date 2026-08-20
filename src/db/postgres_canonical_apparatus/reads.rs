use sqlx::{PgPool, Row};

use crate::core::apparatus_standard::{
    AasxSha256, ApparatusId, CanonicalAasxArtifact, CanonicalApparatusError,
    CanonicalApparatusRevision, RuntimeApparatusProjection, StoredCanonicalAasx,
    parse_canonical_aasx,
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

fn parse_projection(
    payload: serde_json::Value,
) -> Result<RuntimeApparatusProjection, CanonicalApparatusError> {
    serde_json::from_value(payload).map_err(|_| CanonicalApparatusError::ArtifactIntegrity)
}
