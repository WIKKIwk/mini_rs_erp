use super::transaction_locks::lock_orders_and_apparatuses_tx;
use crate::core::apparatus_standard::RuntimeApparatusConfiguration;
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, QueueActionActor, SequenceMove,
    SequenceMoveResult, SequenceMoveState,
};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};

fn db_error(error: sqlx::Error) -> ProductionMapError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("40001" | "40P01") => ProductionMapError::QueueReorderConflict,
        _ => ProductionMapError::StoreFailed,
    }
}

pub(super) async fn commit(
    pool: &PgPool,
    canonical: &RuntimeApparatusConfiguration,
    command: &SequenceMove,
    actor: &QueueActionActor,
) -> Result<SequenceMoveResult, ProductionMapError> {
    command.validate()?;
    // A newly assigned order can enter the apparatus while locks are acquired.
    // Retry with its order lock included, or return a bounded conflict.
    for attempt in 0..3 {
        let result = commit_once(pool, canonical, command, actor).await;
        if attempt == 2 || result != Err(ProductionMapError::QueueReorderConflict) {
            return result;
        }
    }
    unreachable!()
}

async fn commit_once(
    pool: &PgPool,
    canonical: &RuntimeApparatusConfiguration,
    command: &SequenceMove,
    actor: &QueueActionActor,
) -> Result<SequenceMoveResult, ProductionMapError> {
    let mut tx = pool.begin().await.map_err(db_error)?;
    // Read committed is deliberate: validation must see a start/freeze that
    // committed while this request was waiting for the advisory locks. Taking
    // a serializable snapshot BEFORE waiting could read the old queue state.
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    let candidates = load_maps_tx(&mut tx).await?;
    let mut locked_orders = SequenceMoveState::from_data(
        canonical,
        &candidates,
        &[],
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
    .order_ids
    .into_iter()
    .collect::<BTreeSet<_>>();
    locked_orders.insert(command.order_id.clone());
    locked_orders.extend(
        command
            .before_order_id
            .iter()
            .chain(&command.after_order_id)
            .cloned(),
    );
    let refs = locked_orders.iter().map(String::as_str).collect::<Vec<_>>();
    // Match the established lock order: sorted orders, then apparatus. Map
    // edits/control changes lock orders; starts/holds/sequences lock apparatus.
    lock_orders_and_apparatuses_tx(&mut tx, &refs, &[&command.apparatus]).await?;
    let request = serde_json::to_value(command).map_err(|_| ProductionMapError::StoreFailed)?;
    let receipt: Option<(serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT request_json, result_json FROM mini_queue_reorder_commands
         WHERE canonical_apparatus_id=$1 AND actor_role=$2 AND actor_ref=$3 AND idempotency_key=$4",
    )
    .bind(&command.apparatus)
    .bind(&actor.role)
    .bind(&actor.ref_)
    .bind(&command.idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    if let Some((previous, result)) = receipt {
        if previous != request {
            return Err(ProductionMapError::QueueReorderIdempotencyConflict);
        }
        return serde_json::from_value(result).map_err(|_| ProductionMapError::StoreFailed);
    }
    let maps = load_maps_tx(&mut tx).await?;
    let selected = SequenceMoveState::from_data(
        canonical,
        &maps,
        &[],
        &BTreeMap::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    );
    if selected
        .order_ids
        .iter()
        .any(|id| !locked_orders.contains(id))
    {
        return Err(ProductionMapError::QueueReorderConflict);
    }
    let stored: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT order_ids FROM mini_queue_sequences WHERE canonical_apparatus_id=$1",
    )
    .bind(&command.apparatus)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    let stored: Vec<String> =
        serde_json::from_value(stored.unwrap_or_else(|| serde_json::json!([])))
            .map_err(|_| ProductionMapError::StoreFailed)?;
    let states: Vec<(String, String)> = sqlx::query_as(
        "SELECT order_id,state FROM mini_queue_states WHERE canonical_apparatus_id=$1",
    )
    .bind(&command.apparatus)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    let frozen: Vec<String> =
        sqlx::query_scalar("SELECT order_id FROM mini_order_control_states WHERE state='frozen'")
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
    let holds: Vec<String> = sqlx::query_scalar(
        "SELECT order_id FROM mini_print_preflight_holds
        WHERE canonical_apparatus_id=$1 AND status IN ('held','running','passed')",
    )
    .bind(&command.apparatus)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_error)?;
    let state = SequenceMoveState::from_data(
        canonical,
        &maps,
        &stored,
        &states.into_iter().collect::<BTreeMap<_, _>>(),
        &frozen.into_iter().collect::<BTreeSet<_>>(),
        &holds.into_iter().collect::<BTreeSet<_>>(),
    );
    let result = state.apply(command)?;
    let mut result = result;
    if let Some(op) = result.event.clone() {
        let (new_rev,): (i64,) = sqlx::query_as(
            "INSERT INTO mini_queue_sequences (apparatus,canonical_apparatus_id,order_ids,revision,updated_at)
            VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id=$1),$1),$1,$2,1,now())
            ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
                order_ids=excluded.order_ids,
                revision=COALESCE(mini_queue_sequences.revision, 0) + 1,
                updated_at=excluded.updated_at
            RETURNING revision",
        )
        .bind(&command.apparatus)
        .bind(serde_json::json!(result.order_ids))
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;

        let base_rev = (new_rev - 1).max(0);
        sqlx::query(
            "INSERT INTO mini_queue_events (canonical_apparatus_id, revision, base_revision, event_type, ops)
            VALUES ($1, $2, $3, 'delta', $4)
            ON CONFLICT (canonical_apparatus_id, revision) DO NOTHING",
        )
        .bind(&command.apparatus)
        .bind(new_rev)
        .bind(base_rev)
        .bind(serde_json::json!([op]))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;

        result.revision = Some(new_rev);
    } else {
        let current_rev: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM mini_queue_sequences WHERE canonical_apparatus_id=$1",
        )
        .bind(&command.apparatus)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        result.revision = current_rev;
    }
    sqlx::query(
        "INSERT INTO mini_queue_reorder_commands
        (canonical_apparatus_id,actor_role,actor_ref,idempotency_key,request_json,result_json)
        VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(&command.apparatus)
    .bind(&actor.role)
    .bind(&actor.ref_)
    .bind(&command.idempotency_key)
    .bind(request)
    .bind(serde_json::json!(result))
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(result)
}

async fn load_maps_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
    let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT map_json FROM mini_production_maps ORDER BY updated_at DESC, id ASC",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    // Match the existing read-side tolerance for unrelated legacy bad maps.
    Ok(payloads
        .into_iter()
        .filter_map(|payload| match serde_json::from_value(payload) {
            Ok(map) => Some(map),
            Err(_) => {
                tracing::warn!("skipping invalid map while resolving queue move");
                None
            }
        })
        .collect())
}
