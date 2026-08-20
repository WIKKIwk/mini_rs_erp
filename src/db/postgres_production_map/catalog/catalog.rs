use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{PgPool, Postgres, Transaction};

use crate::core::apparatus_standard::{
    ApparatusId, CanonicalApparatus, CapabilityCode, QueuePolicy,
};
use crate::core::production_map::{
    ApparatusCapacityProfile, ApparatusQueuePolicy, ApparatusQueuePolicyMap,
    ApparatusWorkingWindow, ProductionMapDefinition, ProductionMapError, QueueActionActor,
};

use super::transaction_locks::lock_apparatus_tx;

pub(super) async fn load_canonical_apparatuses(
    pool: &PgPool,
) -> Result<Vec<CanonicalApparatus>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT id, payload_json
         FROM mini_apparatus
         WHERE jsonb_typeof(payload_json->'canonical_apparatus') = 'object'
         ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|(id, payload)| canonical_from_payload(id, payload))
        .collect()
}

pub(super) async fn load_canonical_apparatus(
    pool: &PgPool,
    apparatus_id: &ApparatusId,
) -> Result<Option<CanonicalApparatus>, ProductionMapError> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_apparatus
         WHERE id = $1
           AND jsonb_typeof(payload_json->'canonical_apparatus') = 'object'
         LIMIT 1",
    )
    .bind(apparatus_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    payload
        .map(|payload| canonical_from_payload(apparatus_id.to_string(), payload))
        .transpose()
}

pub(super) async fn mutate_canonical_apparatus_tx<F>(
    tx: &mut Transaction<'_, Postgres>,
    apparatus_id: &ApparatusId,
    mutate: F,
) -> Result<CanonicalApparatus, ProductionMapError>
where
    F: FnOnce(&mut CanonicalApparatus) -> Result<(), ProductionMapError>,
{
    let row = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT id, payload_json
         FROM mini_apparatus
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(apparatus_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::StoreFailed)?;
    let mut canonical = canonical_from_payload(row.0, row.1)?;
    mutate(&mut canonical)?;
    canonical.versioning.revision = canonical
        .versioning
        .revision
        .checked_add(1)
        .ok_or(ProductionMapError::StoreFailed)?;
    canonical
        .validate()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let payload = serde_json::to_value(&canonical).map_err(|_| ProductionMapError::StoreFailed)?;
    let updated = sqlx::query(
        "UPDATE mini_apparatus
         SET payload_json = jsonb_set(
                 COALESCE(payload_json, '{}'::jsonb),
                 '{canonical_apparatus}',
                 $2,
                 true
             ),
             updated_at = now()
         WHERE id = $1",
    )
    .bind(apparatus_id.as_str())
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if updated.rows_affected() != 1 {
        return Err(ProductionMapError::StoreFailed);
    }
    Ok(canonical)
}

pub(super) fn capacity_profile_from_canonical(
    canonical: &CanonicalApparatus,
    now_unix: i64,
) -> Result<ApparatusCapacityProfile, ProductionMapError> {
    canonical
        .validate()
        .map_err(|_| ProductionMapError::StoreFailed)?;

    let mut capabilities = Vec::new();
    let mut capability_levels = BTreeMap::new();
    for capability in &canonical.capabilities {
        let code = capability_code_name(*capability);
        let profiles = canonical
            .capability_profiles
            .iter()
            .filter(|profile| profile.code == *capability)
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            capabilities.push(code.to_string());
            capability_levels.insert(code.to_string(), 1);
            continue;
        }
        let active = profiles
            .into_iter()
            .filter(|profile| {
                profile.enabled
                    && profile
                        .valid_from_unix
                        .is_none_or(|starts_at| now_unix >= starts_at)
                    && profile
                        .valid_to_unix
                        .is_none_or(|ends_at| now_unix < ends_at)
            })
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(ProductionMapError::StoreFailed);
        }
        if let Some(profile) = active.first() {
            capabilities.push(code.to_string());
            capability_levels.insert(code.to_string(), profile.level);
        }
    }

    Ok(ApparatusCapacityProfile {
        apparatus_id: canonical.identity.id.clone(),
        apparatus: canonical.identity.display.display_name.clone(),
        capacity_slots: canonical.capacity.capacity_slots,
        setup_minutes: canonical.capacity.setup_minutes,
        cleanup_minutes: canonical.capacity.cleanup_minutes,
        efficiency_percent: canonical.capacity.efficiency_percent,
        finite_capacity: canonical.capacity.finite_capacity,
        working_windows: canonical
            .capacity
            .working_windows
            .iter()
            .map(|window| ApparatusWorkingWindow {
                weekday: window.weekday,
                start_minute: window.start_minute,
                end_minute: window.end_minute,
            })
            .collect(),
        capabilities,
        capability_levels,
        notes: String::new(),
        updated_at_unix: now_unix,
    })
}

pub(super) fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or_default()
}

fn canonical_from_payload(
    id: String,
    payload: serde_json::Value,
) -> Result<CanonicalApparatus, ProductionMapError> {
    let id = ApparatusId::new(id).map_err(|_| ProductionMapError::StoreFailed)?;
    let canonical = payload
        .get("canonical_apparatus")
        .cloned()
        .ok_or(ProductionMapError::StoreFailed)
        .and_then(|payload| {
            serde_json::from_value::<CanonicalApparatus>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)
        })?;
    if canonical.identity.id != id {
        return Err(ProductionMapError::StoreFailed);
    }
    canonical
        .validate()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(canonical)
}

fn capability_code_name(code: CapabilityCode) -> &'static str {
    match code {
        CapabilityCode::Print => "print",
        CapabilityCode::Pechat => "pechat",
        CapabilityCode::Flexo => "flexo",
        CapabilityCode::Laminate => "laminate",
        CapabilityCode::Cut => "cut",
        CapabilityCode::Package => "package",
        CapabilityCode::Glue => "glue",
        CapabilityCode::Apparatus => "apparatus",
    }
}

pub(super) async fn load_maps(
    pool: &PgPool,
) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json
         FROM mini_production_maps
         ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|payload| {
            serde_json::from_value::<ProductionMapDefinition>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)
        })
        .collect()
}

pub(super) async fn delete_map_by_id(
    pool: &PgPool,
    map_id: &str,
) -> Result<(), ProductionMapError> {
    let map_id = map_id.trim();
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mini_order_id = sqlx::query_scalar::<_, Option<String>>(
        "SELECT order_id FROM mini_production_maps WHERE id = $1 FOR UPDATE",
    )
    .bind(map_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .flatten();
    sqlx::query("DELETE FROM mini_queue_states WHERE order_id = $1")
        .bind(map_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "UPDATE mini_queue_sequences
         SET order_ids = COALESCE(
                 (
                     SELECT jsonb_agg(entry.value ORDER BY entry.ordinality)
                     FROM jsonb_array_elements(mini_queue_sequences.order_ids)
                          WITH ORDINALITY AS entry(value, ordinality)
                     WHERE entry.value <> to_jsonb($1::text)
                 ),
                 '[]'::jsonb
             ),
             updated_at = now()
         WHERE order_ids @> jsonb_build_array($1::text)",
    )
    .bind(map_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query("DELETE FROM mini_production_maps WHERE id = $1")
        .bind(map_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    if let Some(order_id) = mini_order_id {
        sqlx::query("DELETE FROM mini_orders WHERE id = $1")
            .bind(order_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn load_apparatus_sequences(
    pool: &PgPool,
) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT canonical_apparatus_id, order_ids
         FROM mini_queue_sequences
         ORDER BY canonical_apparatus_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|(apparatus, payload)| {
            let apparatus = ApparatusId::new(apparatus)
                .map_err(|_| ProductionMapError::StoreFailed)?
                .to_string();
            let order_ids = serde_json::from_value::<Vec<String>>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)?;
            Ok((apparatus, order_ids))
        })
        .collect()
}

pub(super) async fn save_apparatus_sequence(
    pool: &PgPool,
    apparatus: &str,
    order_ids: Vec<String>,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    save_apparatus_sequence_tx(&mut tx, apparatus, &order_ids).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn save_apparatus_sequence_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    order_ids: &[String],
) -> Result<(), ProductionMapError> {
    let apparatus_id = lock_apparatus_tx(tx, apparatus).await?;
    let order_ids = order_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    let payload = serde_json::to_value(order_ids).map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_queue_sequences
            (apparatus, canonical_apparatus_id, order_ids, updated_at)
         VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
           apparatus = excluded.apparatus,
           order_ids = excluded.order_ids,
           updated_at = excluded.updated_at",
    )
    .bind(apparatus_id.as_str())
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn apply_apparatus_sequence_delta_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    order_id: &str,
    incoming_order_ids: &[String],
    remove_order: bool,
    append_order: bool,
) -> Result<(), ProductionMapError> {
    let apparatus_id = lock_apparatus_tx(tx, apparatus).await?;
    let current = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT order_ids
         FROM mini_queue_sequences
         WHERE canonical_apparatus_id = $1
         FOR UPDATE",
    )
    .bind(apparatus_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut order_ids = current
        .map(|payload| {
            serde_json::from_value::<Vec<String>>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)
        })
        .transpose()?
        .unwrap_or_else(|| incoming_order_ids.to_vec());
    if remove_order {
        order_ids.retain(|candidate| candidate.trim() != order_id.trim());
    }
    if append_order {
        order_ids.retain(|candidate| candidate.trim() != order_id.trim());
        order_ids.push(order_id.trim().to_string());
    }
    save_apparatus_sequence_tx(tx, apparatus_id.as_str(), &order_ids).await
}

pub(super) async fn load_apparatus_queue_states(
    pool: &PgPool,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT canonical_apparatus_id, order_id, state
         FROM mini_queue_states
         ORDER BY canonical_apparatus_id ASC, order_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    let mut grouped = BTreeMap::<String, BTreeMap<String, String>>::new();
    for (apparatus, order_id, state) in rows {
        let apparatus = ApparatusId::new(apparatus)
            .map_err(|_| ProductionMapError::StoreFailed)?
            .to_string();
        grouped
            .entry(apparatus)
            .or_default()
            .insert(order_id, state);
    }
    Ok(grouped)
}

pub(super) async fn load_apparatus_queue_policies(
    pool: &PgPool,
) -> Result<ApparatusQueuePolicyMap, ProductionMapError> {
    let mut policies = BTreeMap::new();
    for canonical in load_canonical_apparatuses(pool).await? {
        let policy = match canonical.policies.queue {
            QueuePolicy::StrictSequence => ApparatusQueuePolicy::StrictSequence,
            QueuePolicy::FreePick => ApparatusQueuePolicy::FreePick,
        };
        if policies.insert(canonical.identity.id, policy).is_some() {
            return Err(ProductionMapError::StoreFailed);
        }
    }
    Ok(policies)
}

pub(super) async fn save_apparatus_queue_policy(
    pool: &PgPool,
    apparatus_id: &ApparatusId,
    _apparatus_display: &str,
    policy: ApparatusQueuePolicy,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let canonical = mutate_canonical_apparatus_tx(&mut tx, apparatus_id, |canonical| {
        canonical.policies.queue = match policy {
            ApparatusQueuePolicy::StrictSequence => QueuePolicy::StrictSequence,
            ApparatusQueuePolicy::FreePick => QueuePolicy::FreePick,
        };
        Ok(())
    })
    .await?;
    let payload = serde_json::json!({
        "actor": actor,
        "policy": policy.as_str(),
    });
    sqlx::query(
        "INSERT INTO mini_apparatus_queue_policies
            (apparatus, canonical_apparatus_id, policy, actor_role, actor_ref, actor_display_name, payload_json, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
           apparatus = excluded.apparatus,
           policy = excluded.policy,
           actor_role = excluded.actor_role,
           actor_ref = excluded.actor_ref,
           actor_display_name = excluded.actor_display_name,
           payload_json = excluded.payload_json,
           updated_at = excluded.updated_at",
    )
    .bind(canonical.identity.display.display_name.trim())
    .bind(apparatus_id.as_str())
    .bind(policy.as_str())
    .bind(actor.role.trim())
    .bind(actor.ref_.trim())
    .bind(actor.display_name.trim())
    .bind(payload)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}
