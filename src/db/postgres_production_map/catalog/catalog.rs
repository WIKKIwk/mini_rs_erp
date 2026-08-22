use std::collections::BTreeMap;

use sqlx::{PgPool, Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{ProductionMapDefinition, ProductionMapError};

use super::transaction_locks::lock_apparatus_tx;

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
