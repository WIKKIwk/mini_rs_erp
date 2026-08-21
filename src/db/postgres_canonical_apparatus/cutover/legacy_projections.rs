use std::collections::BTreeMap;

use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::{
    ApparatusProjectionSet, CanonicalApparatusError, CanonicalApparatusRevision,
    MaterialExecutionPolicy,
};

pub(super) async fn write(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    write_runtime(tx, revision, projections).await?;
    write_queue(tx, revision, projections).await?;
    write_material(tx, revision, projections).await?;
    write_capacity(tx, revision, projections).await
}

async fn write_runtime(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    let runtime = &projections.runtime;
    sqlx::query(
        "UPDATE mini_apparatus SET
             name = $2, base_name = $2, kind = 'canonical_projection',
             payload_json = $3, source_revision = $4, source_aasx_sha256 = $5,
             schema_version = $6, physical_asset_id = $7, equipment_class_id = $8,
             hierarchy_json = $9, capabilities_json = $10,
             execution_profile_json = $11, policies_json = $12,
             capacity_json = $13, lifecycle_state = $14, updated_at = now()
         WHERE id = $1",
    )
    .bind(runtime.apparatus_id.as_str())
    .bind(&runtime.display.display_name)
    .bind(json(runtime)?)
    .bind(to_i64(runtime.source_revision)?)
    .bind(runtime.source_aasx_sha256.to_hex())
    .bind(i32::try_from(revision.schema_version).map_err(|_| CanonicalApparatusError::Persistence)?)
    .bind(runtime.physical_asset_id.as_str())
    .bind(runtime.equipment_class_id.as_str())
    .bind(json(&runtime.hierarchy)?)
    .bind(json(&runtime.capabilities)?)
    .bind(json(&runtime.execution_profile)?)
    .bind(json(&revision.policies)?)
    .bind(json(&revision.capacity)?)
    .bind(enum_name(&runtime.lifecycle.state)?)
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

async fn write_queue(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    let queue = &projections.queue;
    sqlx::query(
        "INSERT INTO mini_apparatus_queue_policies (
             apparatus, canonical_apparatus_id, policy, actor_role, actor_ref,
             actor_display_name, payload_json, source_revision,
             source_aasx_sha256, updated_at
         ) VALUES ($1, $2, $3, '', '', '', $4, $5, $6, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
             apparatus = EXCLUDED.apparatus, policy = EXCLUDED.policy,
             actor_role = '', actor_ref = '', actor_display_name = '',
             payload_json = EXCLUDED.payload_json,
             source_revision = EXCLUDED.source_revision,
             source_aasx_sha256 = EXCLUDED.source_aasx_sha256, updated_at = now()",
    )
    .bind(&revision.display.display_name)
    .bind(revision.apparatus_id.as_str())
    .bind(enum_name(&queue.discipline)?)
    .bind(json(queue)?)
    .bind(to_i64(queue.source_revision)?)
    .bind(queue.source_aasx_sha256.to_hex())
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

async fn write_material(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    let material = &projections.material;
    let (required, item_groups, requirement_groups) = match &material.policy {
        MaterialExecutionPolicy::NotRequired => (
            false,
            json(&Vec::<String>::new())?,
            json(&Vec::<String>::new())?,
        ),
        MaterialExecutionPolicy::AllRequired { item_group_ids } => {
            (true, json(item_group_ids)?, json(&Vec::<String>::new())?)
        }
        MaterialExecutionPolicy::RequirementSets { sets } => {
            (true, json(&Vec::<String>::new())?, json(sets)?)
        }
    };
    sqlx::query(
        "INSERT INTO mini_apparatus_material_rules (
             apparatus, canonical_apparatus_id, item_groups, requirement_groups,
             requires_material, payload_json, source_revision,
             source_aasx_sha256, updated_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
             apparatus = EXCLUDED.apparatus, item_groups = EXCLUDED.item_groups,
             requirement_groups = EXCLUDED.requirement_groups,
             requires_material = EXCLUDED.requires_material,
             payload_json = EXCLUDED.payload_json,
             source_revision = EXCLUDED.source_revision,
             source_aasx_sha256 = EXCLUDED.source_aasx_sha256, updated_at = now()",
    )
    .bind(&revision.display.display_name)
    .bind(revision.apparatus_id.as_str())
    .bind(item_groups)
    .bind(requirement_groups)
    .bind(required)
    .bind(json(material)?)
    .bind(to_i64(material.source_revision)?)
    .bind(material.source_aasx_sha256.to_hex())
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

async fn write_capacity(
    tx: &mut Transaction<'_, Postgres>,
    revision: &CanonicalApparatusRevision,
    projections: &ApparatusProjectionSet,
) -> Result<(), CanonicalApparatusError> {
    let capacity = &projections.capacity;
    let levels = revision
        .capabilities
        .iter()
        .map(|capability| Ok((enum_name(&capability.code)?, capability.level)))
        .collect::<Result<BTreeMap<_, _>, CanonicalApparatusError>>()?;
    let capabilities = levels.keys().cloned().collect::<Vec<_>>();
    sqlx::query(
        "INSERT INTO mini_apparatus_capacity_profiles (
             canonical_apparatus_id, apparatus_id, apparatus, capacity_slots,
             setup_minutes, cleanup_minutes, efficiency_percent, finite_capacity,
             working_windows, capabilities, capability_levels, notes,
             source_revision, source_aasx_sha256, updated_at
         ) VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, '', $11, $12, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
             apparatus_id = EXCLUDED.apparatus_id, apparatus = EXCLUDED.apparatus,
             capacity_slots = EXCLUDED.capacity_slots, setup_minutes = EXCLUDED.setup_minutes,
             cleanup_minutes = EXCLUDED.cleanup_minutes,
             efficiency_percent = EXCLUDED.efficiency_percent,
             finite_capacity = EXCLUDED.finite_capacity,
             working_windows = EXCLUDED.working_windows,
             capabilities = EXCLUDED.capabilities,
             capability_levels = EXCLUDED.capability_levels, notes = '',
             source_revision = EXCLUDED.source_revision,
             source_aasx_sha256 = EXCLUDED.source_aasx_sha256, updated_at = now()",
    )
    .bind(revision.apparatus_id.as_str())
    .bind(&revision.display.display_name)
    .bind(i32::from(capacity.capacity_slots))
    .bind(i32::try_from(capacity.setup_minutes).map_err(|_| CanonicalApparatusError::Persistence)?)
    .bind(
        i32::try_from(capacity.cleanup_minutes)
            .map_err(|_| CanonicalApparatusError::Persistence)?,
    )
    .bind(i32::from(capacity.efficiency_percent))
    .bind(capacity.finite_capacity)
    .bind(json(&capacity.working_windows)?)
    .bind(json(&capabilities)?)
    .bind(json(&levels)?)
    .bind(to_i64(capacity.source_revision)?)
    .bind(capacity.source_aasx_sha256.to_hex())
    .execute(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(())
}

fn json(value: &impl serde::Serialize) -> Result<serde_json::Value, CanonicalApparatusError> {
    serde_json::to_value(value).map_err(|_| CanonicalApparatusError::Persistence)
}

fn enum_name(value: &impl serde::Serialize) -> Result<String, CanonicalApparatusError> {
    json(value)?
        .as_str()
        .map(str::to_string)
        .ok_or(CanonicalApparatusError::Persistence)
}

fn to_i64(value: u64) -> Result<i64, CanonicalApparatusError> {
    i64::try_from(value).map_err(|_| CanonicalApparatusError::Persistence)
}
