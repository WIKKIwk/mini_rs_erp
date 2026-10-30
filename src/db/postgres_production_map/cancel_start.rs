//! Compare the entire Start witness under the normal order/resource locks.
//! Never infer an unused roll from a zero quantity or its current colour.
use super::*;
use crate::core::production_map::{
    OrderRunStatus, cancel_start as domain,
    queue_state::{ApparatusQueueAction as Action, ApparatusQueueOrderState as State},
};
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};

mod checkpoint;
use checkpoint::*;
pub(super) use checkpoint::{after_start_tx, before_start_tx};

fn failed(error: sqlx::Error) -> ProductionMapError {
    tracing::error!(%error, "cancel start transaction failed");
    ProductionMapError::StoreFailed
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

pub(super) async fn restore_scanned_input_tx(
    tx: &mut Transaction<'_, Postgres>, kind: &str, batch: &str, before: &Value,
) -> Result<(), ProductionMapError> {
    let kind=match kind {
        "progress_batch" => Kind::Progress,
        "opening_wip" => Kind::Opening,
        _ => return Err(ProductionMapError::QueueActionNotAllowed),
    };
    restore(tx,&Resource { kind,key:batch.into(),before:before.clone(),after:Value::Null,last_material_event:None }).await
}

/// The final input may release a whole unused Start, including its material
/// claims. All resource witnesses are rechecked under the same transaction.
pub(super) async fn can_finish_scanned_session_tx(
    tx: &mut Transaction<'_, Postgres>, session: &OrderRunSession,
) -> Result<bool, ProductionMapError> {
    let Ok(checkpoint)=serde_json::from_value::<Checkpoint>(session.payload_json[domain::CHECKPOINT].clone())
        else { return Ok(false); };
    let Some(control)=checkpoint.control.as_ref() else { return Ok(false); };
    if control.worker_ref!=session.worker_ref || control.session_id!=session.session_id { return Ok(false); }
    let outputs: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_progress_batches WHERE session_id=$1)")
        .bind(&session.session_id).fetch_one(&mut **tx).await.map_err(failed)?;
    let map: Value=sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&session.order_id).fetch_one(&mut **tx).await.map_err(failed)?;
    if outputs || map!=checkpoint.map
        || assignments(tx,&session.apparatus,&session.order_id).await?!=checkpoint.assignments
        || reports(tx,&session.apparatus,&session.order_id).await?!=checkpoint.reports
    { return Ok(false); }
    let schedules=checkpoint.resources.iter().filter(|r| matches!(r.kind,Kind::Schedule))
        .map(|r| r.key.clone()).collect::<Vec<_>>();
    if schedule_ids(tx,&session.apparatus,&session.order_id).await?!=schedules { return Ok(false); }
    for resource in &checkpoint.resources {
        if matches!(resource.kind,Kind::Progress|Kind::Opening|Kind::Intake) { continue; }
        if row(tx,&resource.kind,&resource.key).await?!=resource.after
            || matches!(resource.kind,Kind::Stock)
                && last_material_event(tx,&resource.key).await?!=resource.last_material_event
        { return Ok(false); }
    }
    Ok(true)
}

pub(super) async fn finish_scanned_session_tx(
    tx: &mut Transaction<'_, Postgres>, session: &OrderRunSession, actor: &QueueActionActor, token: &str,
) -> Result<State, ProductionMapError> {
    if !can_finish_scanned_session_tx(tx,session).await? { return Err(ProductionMapError::QueueActionNotAllowed); }
    let checkpoint: Checkpoint=serde_json::from_value(session.payload_json[domain::CHECKPOINT].clone())
        .map_err(|_|ProductionMapError::QueueActionNotAllowed)?;
    for resource in &checkpoint.resources {
        if matches!(resource.kind,Kind::Progress|Kind::Opening|Kind::Intake) { continue; }
        restore(tx,resource).await?;
        if matches!(resource.kind,Kind::Stock) {
            record_material_return(tx,resource,&format!("return-wip:{token}"),&checkpoint.start_event,
                &session.order_id,&session.apparatus,actor).await?;
        }
    }
    sqlx::query("DELETE FROM mini_order_run_sessions WHERE session_id=$1")
        .bind(&session.session_id).execute(&mut **tx).await.map_err(failed)?;
    Ok(checkpoint.control.ok_or(ProductionMapError::QueueActionNotAllowed)?.previous_state)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    store: &PostgresProductionMapStore,
    apparatus: &str,
    order: &str,
    session_id: &str,
    token: &str,
    actor: &QueueActionActor,
    assigned: &[String],
) -> Result<(), ProductionMapError> {
    if !crate::core::production_map::temporary_pause::valid_actor(
        apparatus, order, session_id, token, actor, assigned,
    ) {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let mut tx = store.pool.begin().await.map_err(failed)?;
    transaction_locks::lock_order_and_apparatuses_tx(&mut tx, order, &[apparatus]).await?;
    let event_id = format!("cancel-start:{token}");
    let receipt: Option<(String,String,String,String,Value)> = sqlx::query_as(
        "SELECT order_id,canonical_apparatus_id,actor_role,actor_ref,payload_json FROM mini_queue_action_events WHERE event_id=$1")
        .bind(&event_id).fetch_optional(&mut *tx).await.map_err(failed)?;
    if let Some((o, m, r, w, p)) = receipt {
        // A lost response may be retried after a different roll has started.
        return if (
            o.as_str(),
            m.as_str(),
            r.as_str(),
            w.as_str(),
            text(&p, "session_id"),
        ) == (
            order,
            apparatus,
            actor.role.as_str(),
            actor.ref_.as_str(),
            session_id,
        ) {
            Ok(())
        } else {
            Err(ProductionMapError::QueueActionNotAllowed)
        };
    }
    sqlx::query("SELECT session_id FROM mini_order_run_sessions WHERE session_id=$1 FOR UPDATE")
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(failed)?;
    let session = stage_execution::sessions_tx(&mut tx, order)
        .await?
        .into_iter()
        .find(|s| s.session_id == session_id && s.apparatus == apparatus)
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let control = domain::control(&session).ok_or(ProductionMapError::QueueActionNotAllowed)?;
    if control.token != token || control.worker_ref != actor.ref_ {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let checkpoint: Checkpoint =
        serde_json::from_value(session.payload_json[domain::CHECKPOINT].clone())
            .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
    let state: Option<String> = sqlx::query_scalar("SELECT state FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2 FOR UPDATE")
        .bind(order).bind(apparatus).fetch_optional(&mut *tx).await.map_err(failed)?;
    let blocked: bool = sqlx::query_scalar("SELECT
        EXISTS(SELECT 1 FROM mini_order_control_states WHERE order_id=$1 AND (state<>'active' OR early_close IS NOT NULL)) OR
        EXISTS(SELECT 1 FROM mini_stage_order_controls WHERE order_id=$1 AND stage_node_id=$2 AND
            (record_json->>'state'<>'active' OR record_json->'early_close' IS NOT NULL AND record_json->'early_close'<>'null'::jsonb))")
        .bind(order).bind(&session.stage_node_id).fetch_one(&mut *tx).await.map_err(failed)?;
    let latest: Option<(String, String)> = sqlx::query_as(
        "SELECT event_id,policy FROM mini_queue_action_events
        WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY id DESC LIMIT 1",
    )
    .bind(order)
    .bind(apparatus)
    .fetch_optional(&mut *tx)
    .await
    .map_err(failed)?;
    let policy = latest
        .filter(|(id, _)| id == &checkpoint.start_event)
        .and_then(|(_, policy)| crate::core::production_map::ApparatusQueuePolicy::parse(&policy))
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let changed: bool = sqlx::query_scalar("SELECT
        EXISTS(SELECT 1 FROM mini_order_progress_events WHERE session_id=$1 AND (event_id<>$2 OR action<>'start' OR produced_qty<>0)) OR
        EXISTS(SELECT 1 FROM mini_progress_batches WHERE session_id=$1)")
        .bind(session_id).bind(&checkpoint.progress_event).fetch_one(&mut *tx).await.map_err(failed)?;
    let map: Value =
        sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1 FOR UPDATE")
            .bind(order)
            .fetch_one(&mut *tx)
            .await
            .map_err(failed)?;
    if blocked
        || changed
        || state.as_deref() != Some("in_progress")
        || map != checkpoint.map
        || assignments(&mut tx, apparatus, order).await? != checkpoint.assignments
        || reports(&mut tx, apparatus, order).await? != checkpoint.reports
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let expected_schedules = checkpoint
        .resources
        .iter()
        .filter(|r| matches!(r.kind, Kind::Schedule))
        .map(|r| r.key.clone())
        .collect::<Vec<_>>();
    if schedule_ids(&mut tx, apparatus, order).await? != expected_schedules {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    for resource in &checkpoint.resources {
        if row(&mut tx, &resource.kind, &resource.key).await? != resource.after
            || matches!(resource.kind, Kind::Stock)
                && last_material_event(&mut tx, &resource.key).await?
                    != resource.last_material_event
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
    }
    for resource in &checkpoint.resources {
        restore(&mut tx, resource).await?;
        if matches!(resource.kind, Kind::Stock) {
            record_material_return(
                &mut tx,
                resource,
                &event_id,
                &checkpoint.start_event,
                order,
                apparatus,
                actor,
            )
            .await?;
        }
    }
    // There are no outputs, partial rolls or later events. The unused session
    // is removed; its Start and cancellation remain immutable audit events.
    sqlx::query("DELETE FROM mini_order_run_sessions WHERE session_id=$1")
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(failed)?;
    let payload = json!({"event":"cancel_start","session_id":session_id,"start_event_id":checkpoint.start_event,
        "previous_state":control.previous_state,"input_batch_id":checkpoint.resources[0].key});
    let event = ApparatusQueueActionEvent {
        event_id: event_id.clone(),
        apparatus: apparatus.into(),
        order_id: order.into(),
        stage_node_id: session.stage_node_id.clone(),
        action: Action::Pause,
        from_state: State::InProgress,
        to_state: control.previous_state,
        policy,
        actor: actor.clone(),
        assigned_apparatus: assigned.to_vec(),
        payload_json: payload.clone(),
    };
    put_queue_action_state_tx(&mut tx, &event).await?;
    insert_queue_action_event_tx(&mut tx, &event).await?;
    let mut progress = domain::audit_event(&session, actor, payload);
    progress.event_id = format!("progress:{event_id}");
    put_order_progress_event_tx(&mut tx, &progress).await?;
    lifecycle::refresh_production_order_lifecycle_tx(
        &mut tx,
        order,
        actor,
        &event_id,
        "cancel_start",
    )
    .await?;
    tx.commit().await.map_err(failed)
}

async fn record_material_return(
    tx: &mut Transaction<'_, Postgres>,
    resource: &Resource,
    event: &str,
    start_event: &str,
    order: &str,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    use crate::db::postgres_raw_material_events::{
        RawMaterialEventDraft, insert_raw_material_event_tx,
    };
    let row = &resource.before;
    insert_raw_material_event_tx(
        tx,
        RawMaterialEventDraft {
            idempotency_key: format!("{event}:{}", resource.key),
            event_type: "usage_cancelled".into(),
            warehouse: text(row, "warehouse").into(),
            barcode: text(row, "barcode").into(),
            item_code: text(row, "item_code").into(),
            item_name: text(row, "item_name").into(),
            qty_delta: 0.0,
            uom: text(row, "uom").into(),
            stock_status_before: Some("in_use".into()),
            stock_status_after: Some(text(row, "status").into()),
            order_id: Some(order.into()),
            apparatus: Some(apparatus.into()),
            actor_role: actor.role.clone(),
            actor_ref: actor.ref_.clone(),
            actor_display_name: actor.display_name.clone(),
            owner_role: String::new(),
            owner_ref: String::new(),
            owner_display_name: String::new(),
            source_type: "consumption".into(),
            source_id: event.into(),
            source_line_ref: Some(text(row, "barcode").into()),
            correlation_id: Some(event.into()),
            payload_json: json!({"reason_code":"cancel_start","start_event_id":start_event,"stock_id":row["id"]}),
        },
    )
    .await
    .map_err(failed)?;
    Ok(())
}
