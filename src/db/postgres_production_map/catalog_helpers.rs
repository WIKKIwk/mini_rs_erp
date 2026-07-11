use std::collections::BTreeMap;

use sqlx::PgPool;

use crate::core::production_map::{
    ApparatusQueuePolicy, ProductionMapDefinition, ProductionMapError, QueueActionActor,
};

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
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn load_apparatus_sequences(
    pool: &PgPool,
) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT apparatus, order_ids
         FROM mini_queue_sequences
         ORDER BY apparatus ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|(apparatus, payload)| {
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
    let order_ids = order_ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    let payload = serde_json::to_value(order_ids).map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_queue_sequences (apparatus, order_ids, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (apparatus) DO UPDATE SET
           order_ids = excluded.order_ids,
           updated_at = excluded.updated_at",
    )
    .bind(apparatus.trim())
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn load_apparatus_queue_states(
    pool: &PgPool,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT apparatus, order_id, state
         FROM mini_queue_states
         ORDER BY apparatus ASC, order_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    let mut grouped = BTreeMap::<String, BTreeMap<String, String>>::new();
    for (apparatus, order_id, state) in rows {
        grouped
            .entry(apparatus)
            .or_default()
            .insert(order_id, state);
    }
    Ok(grouped)
}

pub(super) async fn load_apparatus_queue_policies(
    pool: &PgPool,
) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT apparatus, policy
         FROM mini_apparatus_queue_policies
         ORDER BY apparatus ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|(apparatus, policy)| {
            let policy =
                ApparatusQueuePolicy::parse(&policy).ok_or(ProductionMapError::StoreFailed)?;
            Ok((apparatus, policy))
        })
        .collect()
}

pub(super) async fn save_apparatus_queue_policy(
    pool: &PgPool,
    apparatus: &str,
    policy: ApparatusQueuePolicy,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let payload = serde_json::json!({
        "actor": actor,
        "policy": policy.as_str(),
    });
    sqlx::query(
        "INSERT INTO mini_apparatus_queue_policies
            (apparatus, policy, actor_role, actor_ref, actor_display_name, payload_json, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (apparatus) DO UPDATE SET
           policy = excluded.policy,
           actor_role = excluded.actor_role,
           actor_ref = excluded.actor_ref,
           actor_display_name = excluded.actor_display_name,
           payload_json = excluded.payload_json,
           updated_at = excluded.updated_at",
    )
    .bind(apparatus.trim())
    .bind(policy.as_str())
    .bind(actor.role.trim())
    .bind(actor.ref_.trim())
    .bind(actor.display_name.trim())
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}
