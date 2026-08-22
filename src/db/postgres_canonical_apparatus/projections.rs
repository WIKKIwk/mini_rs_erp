use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::{
    ApparatusProjectionSet, CanonicalApparatusError, CanonicalApparatusRevision,
    RuntimeApparatusProjection,
};

pub(super) async fn write_runtime_projection(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    runtime: &RuntimeApparatusProjection,
) -> Result<(), CanonicalApparatusError> {
    let payload =
        serde_json::to_value(runtime).map_err(|_| CanonicalApparatusError::Persistence)?;
    let hierarchy = serde_json::to_value(&runtime.hierarchy)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let capabilities = serde_json::to_value(&runtime.capabilities)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let execution = serde_json::to_value(&runtime.execution_profile)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let policies = serde_json::to_value(&revision.policies)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let capacity = serde_json::to_value(&revision.capacity)
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let source_revision = to_i64(runtime.source_revision)?;
    sqlx::query(
        "INSERT INTO mini_apparatus (
             id, name, payload_json, source_revision,
             source_aasx_sha256, schema_version, physical_asset_id,
             equipment_class_id, hierarchy_json, capabilities_json,
             execution_profile_json, policies_json, capacity_json,
             lifecycle_state
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8,
             $9, $10, $11, $12, $13, $14
         )
         ON CONFLICT (id) DO UPDATE SET
             name = EXCLUDED.name,
             payload_json = EXCLUDED.payload_json,
             source_revision = EXCLUDED.source_revision,
             source_aasx_sha256 = EXCLUDED.source_aasx_sha256,
             schema_version = EXCLUDED.schema_version,
             physical_asset_id = EXCLUDED.physical_asset_id,
             equipment_class_id = EXCLUDED.equipment_class_id,
             hierarchy_json = EXCLUDED.hierarchy_json,
             capabilities_json = EXCLUDED.capabilities_json,
             execution_profile_json = EXCLUDED.execution_profile_json,
             policies_json = EXCLUDED.policies_json,
             capacity_json = EXCLUDED.capacity_json,
             lifecycle_state = EXCLUDED.lifecycle_state,
             updated_at = now()",
    )
    .bind(runtime.apparatus_id.as_str())
    .bind(&runtime.display.display_name)
    .bind(payload)
    .bind(source_revision)
    .bind(runtime.source_aasx_sha256.to_hex())
    .bind(i32::try_from(revision.schema_version).map_err(|_| CanonicalApparatusError::Persistence)?)
    .bind(runtime.physical_asset_id.as_str())
    .bind(runtime.equipment_class_id.as_str())
    .bind(hierarchy)
    .bind(capabilities)
    .bind(execution)
    .bind(policies)
    .bind(capacity)
    .bind(enum_name(&runtime.lifecycle.state)?)
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

pub(super) async fn write_derived_projections(
    tx: &mut Transaction<'_, Postgres>,
    _revision: &CanonicalApparatusRevision,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    write_queue(tx, projections).await?;
    write_material(tx, projections).await?;
    write_capacity(tx, projections).await
}

async fn write_queue(
    tx: &mut Transaction<'_, Postgres>,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    let queue = &projections.queue;
    let payload = serde_json::to_value(queue).map_err(|_| CanonicalApparatusError::Persistence)?;
    sqlx::query(
        "INSERT INTO mini_apparatus_queue_policies (
             canonical_apparatus_id, payload_json, source_revision,
             source_aasx_sha256, updated_at
         ) VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
             payload_json = EXCLUDED.payload_json,
             source_revision = EXCLUDED.source_revision,
             source_aasx_sha256 = EXCLUDED.source_aasx_sha256,
             updated_at = now()",
    )
    .bind(queue.apparatus_id.as_str())
    .bind(payload)
    .bind(to_i64(queue.source_revision)?)
    .bind(queue.source_aasx_sha256.to_hex())
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

async fn write_material(
    tx: &mut Transaction<'_, Postgres>,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    let material = &projections.material;
    let payload =
        serde_json::to_value(material).map_err(|_| CanonicalApparatusError::Persistence)?;
    sqlx::query(
        "INSERT INTO mini_apparatus_material_rules (
             canonical_apparatus_id, payload_json, source_revision,
             source_aasx_sha256, updated_at
         ) VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
             payload_json = EXCLUDED.payload_json,
             source_revision = EXCLUDED.source_revision,
             source_aasx_sha256 = EXCLUDED.source_aasx_sha256,
             updated_at = now()",
    )
    .bind(material.apparatus_id.as_str())
    .bind(payload)
    .bind(to_i64(material.source_revision)?)
    .bind(material.source_aasx_sha256.to_hex())
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

async fn write_capacity(
    tx: &mut Transaction<'_, Postgres>,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    let capacity = &projections.capacity;
    sqlx::query(
        "INSERT INTO mini_apparatus_capacity_profiles (
             canonical_apparatus_id, payload_json, source_revision,
             source_aasx_sha256, updated_at
         ) VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
             payload_json = EXCLUDED.payload_json,
             source_revision = EXCLUDED.source_revision,
             source_aasx_sha256 = EXCLUDED.source_aasx_sha256,
             updated_at = now()",
    )
    .bind(capacity.apparatus_id.as_str())
    .bind(serde_json::to_value(capacity).map_err(|_| CanonicalApparatusError::Persistence)?)
    .bind(to_i64(capacity.source_revision)?)
    .bind(capacity.source_aasx_sha256.to_hex())
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

fn enum_name<T: serde::Serialize>(value: &T) -> Result<String, CanonicalApparatusError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or(CanonicalApparatusError::Persistence)
}

fn to_i64(value: u64) -> Result<i64, CanonicalApparatusError> {
    i64::try_from(value).map_err(|_| CanonicalApparatusError::Persistence)
}
