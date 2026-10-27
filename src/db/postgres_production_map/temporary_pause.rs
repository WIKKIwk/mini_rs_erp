use super::*;
use crate::core::production_map::{ApparatusQueuePolicy, temporary_pause as domain};
use sqlx::{Postgres, Transaction};

async fn session_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    order_id: &str,
) -> Result<OrderRunSession, ProductionMapError> {
    sqlx::query("SELECT session_id FROM mini_order_run_sessions WHERE order_id=$1 AND canonical_apparatus_id=$2 FOR UPDATE")
        .bind(order_id).bind(apparatus).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    stage_execution::sessions_tx(tx, order_id)
        .await?
        .into_iter()
        .filter(|s| s.apparatus == apparatus && s.status.is_open())
        .max_by_key(|s| (s.started_at_unix, s.session_id.clone()))
        .ok_or(ProductionMapError::QueueActionNotAllowed)
}

async fn policy_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
) -> Result<crate::core::apparatus_standard::ApparatusQueueProjection, ProductionMapError> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json FROM mini_apparatus_queue_policies WHERE canonical_apparatus_id=$1 FOR SHARE")
        .bind(apparatus).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    serde_json::from_value(payload).map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn execute(
    store: &PostgresProductionMapStore,
    apparatus: &str,
    order_id: &str,
    session_id: &str,
    token: &str,
    actor: &QueueActionActor,
    assigned: &[String],
    policy: ApparatusQueuePolicy,
) -> Result<(), ProductionMapError> {
    if !domain::valid_actor(apparatus, order_id, session_id, token, actor, assigned) {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let mut tx = store
        .pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    transaction_locks::lock_order_and_apparatuses_tx(&mut tx, order_id, &[apparatus]).await?;
    let receipt: Option<(String, String, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT order_id,canonical_apparatus_id,actor_role,actor_ref,payload_json FROM mini_queue_action_events WHERE event_id=$1")
        .bind(domain::receipt_id(token)).fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if let Some((order, machine, role, worker, payload)) = receipt {
        if order != order_id
            || machine != apparatus
            || role != actor.role
            || worker != actor.ref_
            || payload
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                != Some(session_id)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        return Ok(());
    }
    let current = session_tx(&mut tx, apparatus, order_id).await?;
    let states = sqlx::query_as::<_, (String, String)>(
        "SELECT order_id,state FROM mini_queue_states WHERE canonical_apparatus_id=$1 FOR UPDATE",
    )
    .bind(apparatus)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .into_iter()
    .collect();
    let current_policy = policy_tx(&mut tx, apparatus).await?;
    if current_policy.discipline.as_str() != policy.as_str() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let write =
        domain::suspension_write(&current, session_id, token, actor, assigned, states, policy)?;
    apply_queue_progress_tx(&mut tx, &write).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

/// Validate preparation witnesses under the apparatus/order locks before any mutation.
pub(super) async fn validate_write_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<(), ProductionMapError> {
    let event = &write.event;
    if let Some(sequence) = event.payload_json.get("queue_expected_sequence") {
        let current_sequence = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT order_ids FROM mini_queue_sequences WHERE canonical_apparatus_id=$1 FOR UPDATE",
        )
        .bind(&write.apparatus)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .unwrap_or_else(|| serde_json::json!([]));
        let current_states = sqlx::query_as::<_, (String, String)>(
            "SELECT order_id,state FROM mini_queue_states WHERE canonical_apparatus_id=$1 FOR UPDATE")
            .bind(&write.apparatus).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?
            .into_iter().collect::<BTreeMap<_, _>>();
        let policy = policy_tx(tx, &write.apparatus).await?;
        if *sequence != current_sequence
            || event.payload_json.get("queue_expected_states")
                != Some(&serde_json::json!(current_states))
            || event
                .payload_json
                .get("queue_expected_policy_revision")
                .and_then(serde_json::Value::as_u64)
                != Some(policy.source_revision)
            || event.policy.as_str() != policy.discipline.as_str()
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if let Some(anchors) = event
            .payload_json
            .get("queue_expected_anchor_sessions")
            .and_then(serde_json::Value::as_array)
        {
            for expected in anchors {
                let session: OrderRunSession = serde_json::from_value(expected.clone())
                    .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
                let current = session_tx(tx, &session.apparatus, &session.order_id).await?;
                if current != session {
                    return Err(ProductionMapError::QueueActionNotAllowed);
                }
            }
        }
    }
    let Some(expected) = event.payload_json.get(domain::EXPECTED_SESSION) else {
        return Ok(());
    };
    let expected: OrderRunSession = serde_json::from_value(expected.clone())
        .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
    let current = session_tx(tx, &write.apparatus, &event.order_id).await?;
    if current != expected {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let early_closed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_order_control_states WHERE order_id=$1 AND early_close IS NOT NULL)")
        .bind(&event.order_id).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if early_closed {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    if event.action != crate::core::production_map::queue_state::ApparatusQueueAction::Resume {
        return Ok(());
    }
    if !domain::is_temporarily_paused(&current) {
        return Err(ProductionMapError::ProgressBatchNotResumable);
    }
    let witnesses = event
        .payload_json
        .get("temporary_pause_held_inputs")
        .and_then(serde_json::Value::as_array)
        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
    for input in witnesses {
        let id = input
            .get("batch_id")
            .and_then(serde_json::Value::as_str)
            .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
        let opening =
            input.get("source_kind").and_then(serde_json::Value::as_str) == Some("opening_wip");
        let row = if opening {
            sqlx::query_as::<_, (String, String, String, String)>(
                "SELECT order_id,wip_status,COALESCE(used_by_session_id,''),COALESCE(used_by_apparatus,'') FROM mini_opening_wip_batches WHERE batch_id=$1 FOR UPDATE")
                .bind(id).fetch_optional(&mut **tx).await
        } else {
            sqlx::query_as::<_, (String, String, String, String)>(
                "SELECT order_id,wip_status,COALESCE(used_by_session_id,''),COALESCE(canonical_used_by_apparatus_id,'') FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE")
                .bind(id).fetch_optional(&mut **tx).await
        }.map_err(|_| ProductionMapError::StoreFailed)?;
        if row
            != Some((
                current.order_id.clone(),
                "in_use".into(),
                current.session_id.clone(),
                current.apparatus.clone(),
            ))
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
    }
    let map_json = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json FROM mini_production_maps WHERE id=$1 FOR UPDATE",
    )
    .bind(&current.order_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let map: ProductionMapDefinition =
        serde_json::from_value(map_json).map_err(|_| ProductionMapError::StoreFailed)?;
    if event
        .payload_json
        .get("temporary_pause_map_fingerprint")
        .and_then(serde_json::Value::as_str)
        != Some(crate::core::production_map::wip_route::production_map_fingerprint(&map).as_str())
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    for material in crate::core::production_map::ProductionQrSessionResources::for_session(&current)
        .raw_materials
    {
        let valid = sqlx::query_scalar::<_, bool>(
            "SELECT status='in_use' AND reserved_order_id=$2 AND EXISTS(SELECT 1 FROM mini_raw_material_assignments a WHERE lower(a.barcode)=lower(s.barcode) AND a.order_id=$2 AND a.canonical_apparatus_id=$3) FROM mini_raw_material_stock s WHERE lower(s.barcode)=lower($1) FOR UPDATE")
            .bind(&material.barcode).bind(&current.order_id).bind(&current.apparatus)
            .fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if valid != Some(true) {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
    }
    Ok(())
}
