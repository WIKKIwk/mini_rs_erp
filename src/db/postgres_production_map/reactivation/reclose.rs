//! Close an unstarted continuation. This writes no production quantities and
//! never rolls back an astatka/physical remainder already recorded by the worker.
use super::*;
use crate::core::production_map::queue_state::{
    ApparatusQueueAction as Action, ApparatusQueueOrderState as State,
};
use crate::core::production_map::{ReactivationRecloseTarget, reactivation::reclose_control};

fn denied() -> ProductionMapError {
    ProductionMapError::QueueActionNotAllowed
}

pub(in crate::db::postgres_production_map) async fn execute(
    store: &PostgresProductionMapStore,
    target: &ReactivationRecloseTarget,
    actor: &QueueActionActor,
    assigned: &[String],
) -> Result<(), ProductionMapError> {
    if actor.role != "aparatchi"
        || actor.ref_.is_empty()
        || !assigned.contains(&target.apparatus)
        || target.order_id.starts_with("training-")
    {
        return Err(denied());
    }
    let mut tx = store
        .pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    transaction_locks::lock_order_and_apparatuses_tx(
        &mut tx,
        &target.order_id,
        &[&target.apparatus],
    )
    .await?;
    let event_id = format!("reactivation-reclose:{}", target.token);
    let receipt: Option<ReactivationReceipt> = sqlx::query_as("SELECT order_id,canonical_apparatus_id,actor_role,actor_ref,stage_node_id,payload_json FROM mini_queue_action_events WHERE event_id=$1")
        .bind(&event_id).fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if let Some(receipt) = receipt {
        if receipt.0 != target.order_id
            || receipt.1 != target.apparatus
            || receipt.2 != actor.role
            || receipt.3 != actor.ref_
            || receipt.4 != target.stage_node_id
            || field(&receipt.5, "session_id") != target.session_id
        {
            return Err(denied());
        }
        return Ok(()); // A retry never touches a newer session or stock.
    }
    let mut session = stage_execution::sessions_tx(&mut tx, &target.order_id)
        .await?
        .into_iter()
        .filter(|s| s.apparatus == target.apparatus)
        .max_by_key(|s| (s.started_at_unix, s.session_id.clone()))
        .ok_or_else(denied)?;
    let control = reclose_control(&session).ok_or_else(denied)?;
    if control.session_id != target.session_id
        || control.stage_node_id != target.stage_node_id
        || control.token != target.token
        || control.worker_role != actor.role
        || control.worker_ref != actor.ref_
    {
        return Err(denied());
    }
    let checkpoint = session.payload_json["_reactivation_reclose"].clone();
    let source_id = field(&session.payload_json, "_reactivation_of_session_id").to_string();
    let map: Option<(Value, String)> = sqlx::query_as(
        "SELECT map_json,lifecycle_status FROM mini_production_maps WHERE id=$1 FOR UPDATE",
    )
    .bind(&target.order_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if map.is_none_or(|(map, state)| {
        map != checkpoint["map"] || matches!(state.as_str(), "closed" | "cancelled")
    }) {
        return Err(denied());
    }
    let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_order_control_states WHERE order_id=$1 AND (state<>'active' OR early_close IS NOT NULL))")
        .bind(&target.order_id).fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let state: Option<String> = sqlx::query_scalar("SELECT state FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2 FOR UPDATE")
        .bind(&target.order_id).bind(&target.apparatus).fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let latest: Option<(String, String)> = sqlx::query_as("SELECT event_id,policy FROM mini_queue_action_events WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY id DESC LIMIT 1")
        .bind(&target.order_id).bind(&target.apparatus).fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let policy = latest
        .filter(|(id, _)| *id == format!("reactivation:{}", target.token))
        .and_then(|(_, policy)| crate::core::production_map::ApparatusQueuePolicy::parse(&policy))
        .ok_or_else(denied)?;
    let worked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_order_progress_events WHERE session_id=$1 AND event_id<>$2) OR EXISTS(SELECT 1 FROM mini_progress_batches WHERE session_id=$1 AND COALESCE(payload_json->>'origin','')<>'layer_remainder')")
        .bind(&session.session_id).bind(format!("progress:reactivation:{}", target.token))
        .fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if blocked || state.as_deref() != Some("paused") || worked {
        return Err(denied());
    }
    // Immutable receipts need no row update lock; the order/apparatus lock
    // serializes their writers, and the runtime role intentionally cannot edit them.
    let operations: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(o) FROM mini_layer_remainder_operations o WHERE session_id=$1 AND order_id=$2 AND apparatus=$3 AND recorded_by_ref=$4 ORDER BY operation_id")
        .bind(&session.session_id).bind(&session.order_id).bind(&session.apparatus).bind(&actor.ref_)
        .fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let assignments: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(a)-'updated_at' FROM mini_raw_material_assignments a WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY barcode FOR UPDATE")
        .bind(&session.order_id).bind(&session.apparatus).fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let originals = checkpoint["assignments"].as_array().ok_or_else(denied)?;
    if originals.iter().any(|a| !assignments.contains(a))
        || assignments.iter().any(|a| {
            !originals.contains(a)
                && !operations.iter().any(|op| {
                    field(&a["payload_json"], "layer_remainder_operation_id")
                        == field(op, "operation_id")
                        && field(&op["response_json"]["raw_material_stock"], "barcode")
                            == field(a, "barcode")
                })
        })
    {
        return Err(denied());
    }
    let baseline_links =
        order_run_input_links_from_payload(&checkpoint["payload"]).map_err(|_| denied())?;
    let links = order_run_input_links_from_payload(&session.payload_json).map_err(|_| denied())?;
    if links.len() != baseline_links.len()
        || links.iter().any(|link| {
            !baseline_links.iter().any(|old| {
                old.input_batch_id == link.input_batch_id && old.source_kind == link.source_kind
            })
        })
    {
        return Err(denied());
    }
    let input_id = field(&checkpoint["payload"], "input_progress_batch_id");
    let actual_input = field(&session.payload_json, "input_progress_batch_id");
    if actual_input != input_id
        && !(actual_input.is_empty() && has_remainder(&operations, "progress_batch", input_id))
    {
        return Err(denied());
    }
    let original_resources = ProductionQrSessionResources::for_session(&OrderRunSession {
        payload_json: checkpoint["payload"].clone(),
        ..session.clone()
    });
    let resources = ProductionQrSessionResources::for_session(&session);
    if resources
        .raw_materials
        .iter()
        .any(|r| !original_resources.raw_materials.contains(r))
    {
        return Err(denied());
    }
    settle_input(&mut tx, &checkpoint, &session, &operations).await?;
    settle_raw(
        &mut tx,
        &checkpoint,
        &session,
        &operations,
        &source_id,
        &target.token,
        actor,
    )
    .await?;
    // Keep any astatka report and all original completion amounts unchanged.
    session.status = OrderRunStatus::Completed;
    session.updated_at_unix = crate::core::production_map::stage_execution::report_now();
    session.payload_json["_reactivation_reclosed_without_work"] = json!(true);
    session.payload_json["_completion_queue_event_id"] = json!(event_id);
    session
        .payload_json
        .as_object_mut()
        .map(|p| p.remove("_reactivation_reclose"));
    let mut links =
        order_run_input_links_from_payload(&session.payload_json).map_err(|_| denied())?;
    for link in &mut links {
        if link.status == OrderRunInputStatus::InUse {
            link.status = OrderRunInputStatus::Processed;
            link.processed_at_unix = Some(session.updated_at_unix);
        }
    }
    write_order_run_input_links(&mut session.payload_json, &links);
    stage_execution::stamp_report_tx(&mut tx, &mut session, &event_id, actor).await?;
    put_order_run_session_tx(&mut tx, &session).await?;
    // Closing this worker's continuation does not complete incoming rolls or
    // an upstream operation reopened while the continuation was paused.
    let to_state = queue_state_after_reclose(&mut tx, &session).await?;
    let event = ApparatusQueueActionEvent {
        event_id: event_id.clone(),
        apparatus: target.apparatus.clone(),
        order_id: target.order_id.clone(),
        stage_node_id: target.stage_node_id.clone(),
        action: Action::Complete,
        from_state: State::Paused,
        to_state,
        policy,
        actor: actor.clone(),
        assigned_apparatus: assigned.to_vec(),
        payload_json: json!({"event":"reactivation_reclosed_without_work", "session_id":target.session_id,
            "source_session_id":source_id, "token":target.token, "no_new_production":true}),
    };
    put_queue_action_state_tx(&mut tx, &event).await?;
    insert_queue_action_event_tx(&mut tx, &event).await?;
    update_apparatus_schedule_reservation_status_tx(
        &mut tx,
        &target.order_id,
        &ApparatusId::new(target.apparatus.clone()).map_err(|_| denied())?,
        crate::core::production_map::ApparatusScheduleStatus::Completed,
        actor,
    )
    .await?;
    lifecycle::refresh_production_order_lifecycle_tx(
        &mut tx,
        &target.order_id,
        actor,
        &event_id,
        "reactivation_reclosed_without_work",
    )
    .await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

async fn queue_state_after_reclose(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<State, ProductionMapError> {
    let map: Value = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&session.order_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let map: ProductionMapDefinition =
        serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    let sessions = stage_execution::sessions_tx(tx, &session.order_id).await?;
    let inputs = stage_execution::inputs_tx(tx, &session.order_id).await?;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT canonical_apparatus_id,state FROM mini_queue_states WHERE order_id=$1",
    )
    .bind(&session.order_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let states = rows
        .into_iter()
        .map(|(apparatus, state)| (apparatus, [(session.order_id.clone(), state)].into()))
        .collect();
    let events: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT stage_node_id,action,to_state FROM mini_queue_action_events WHERE order_id=$1
         AND stage_node_id<>'' AND COALESCE(payload_json->>'completion_request','false')<>'true' ORDER BY id",
    ).bind(&session.order_id).fetch_all(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let events = events
        .into_iter()
        .map(|(stage_node_id, action, state)| {
            Ok(crate::core::production_map::ProductionStageLifecycleEvent {
                stage_node_id,
                action: Action::parse(&action).ok_or_else(denied)?,
                to_state: State::parse(&state).ok_or_else(denied)?,
            })
        })
        .collect::<Result<Vec<_>, ProductionMapError>>()?;
    let work = crate::core::production_map::stage_execution::stage_work_statuses(
        &map, &sessions, &inputs, &states, &events,
    );
    let stage = work
        .iter()
        .find(|stage| {
            chain::stage_node_ids_match_for_map(&map, &stage.stage_node_id, &session.stage_node_id)
        })
        .ok_or_else(denied)?;
    Ok(if stage.completed {
        State::Completed
    } else {
        State::Pending
    })
}

fn has_remainder(operations: &[Value], kind: &str, id: &str) -> bool {
    operations
        .iter()
        .any(|op| field(op, "source_kind") == kind && field(op, "source_id") == id)
}

async fn settle_input(
    tx: &mut Transaction<'_, Postgres>,
    checkpoint: &Value,
    session: &OrderRunSession,
    operations: &[Value],
) -> Result<(), ProductionMapError> {
    if checkpoint["input"].is_object() {
        let original: OrderProgressBatch =
            serde_json::from_value(checkpoint["input"].clone()).map_err(|_| denied())?;
        sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE")
            .bind(&original.batch_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut current = order_query_helpers::load_progress_batch(&mut **tx, &original.batch_id)
            .await?
            .ok_or_else(denied)?;
        if has_remainder(operations, "progress_batch", &original.batch_id) {
            if current.wip_status != OrderProgressBatchWipStatus::Processed
                || current.processed_by_session_id != session.session_id
            {
                return Err(denied());
            }
        } else {
            if current.wip_status != OrderProgressBatchWipStatus::InUse
                || current.used_by_session_id != session.session_id
                || current.used_by_apparatus != session.apparatus
                || current.revision != original.revision
                || current.produced_qty != original.produced_qty
                || current.qr_payload != original.qr_payload
            {
                return Err(denied());
            }
            current.wip_status = original.wip_status;
            current.used_by_session_id = original.used_by_session_id;
            current.used_by_apparatus = original.used_by_apparatus;
            current.processed_by_session_id = original.processed_by_session_id;
            current.processed_by_apparatus = original.processed_by_apparatus;
            current.current_apparatus = original.current_apparatus;
            current.current_location = original.current_location;
            put_order_progress_batch_tx(tx, &current).await?;
        }
    }
    if checkpoint["opening_input"].is_object() {
        let original = &checkpoint["opening_input"];
        let affected = sqlx::query("UPDATE mini_opening_wip_batches SET wip_status='processed',used_by_session_id=$4,used_by_apparatus=$5,processed_by_session_id=$6,processed_by_apparatus=$7,updated_at=now() WHERE batch_id=$1 AND wip_status='in_use' AND used_by_session_id=$2 AND used_by_apparatus=$3")
            .bind(field(original,"batch_id")).bind(&session.session_id).bind(&session.apparatus)
            .bind(field(original,"used_by_session_id")).bind(field(original,"used_by_apparatus"))
            .bind(field(original,"processed_by_session_id")).bind(field(original,"processed_by_apparatus"))
            .execute(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?.rows_affected();
        if affected != 1 {
            return Err(denied());
        }
    }
    Ok(())
}

async fn settle_raw(
    tx: &mut Transaction<'_, Postgres>,
    checkpoint: &Value,
    session: &OrderRunSession,
    operations: &[Value],
    source_id: &str,
    token: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    for original in checkpoint["raw"].as_array().ok_or_else(denied)? {
        let current: Value = sqlx::query_scalar(
            "SELECT to_jsonb(s) FROM mini_raw_material_stock s WHERE id=$1 FOR UPDATE",
        )
        .bind(field(original, "id"))
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or_else(denied)?;
        for key in [
            "id",
            "barcode",
            "item_code",
            "qty",
            "uom",
            "warehouse",
            "reserved_order_id",
            "width_mm",
            "micron",
            "length_m",
        ] {
            if current.get(key) != original.get(key) {
                return Err(denied());
            }
        }
        if has_remainder(operations, "raw_material", field(original, "id")) {
            if field(&current, "status") != "consumed" {
                return Err(denied());
            }
            continue; // Its residual child and ledger already represent physical stock.
        }
        if field(&current, "status") != "in_use" {
            return Err(denied());
        }
        if field(original, "status") != "consumed" {
            continue;
        }
        let owner: (String, String, String) = sqlx::query_as("SELECT COALESCE(owner_role,''),COALESCE(owner_ref,''),COALESCE(owner_display_name,'') FROM mini_raw_material_events WHERE correlation_id=$1 AND event_type='consumption_reversed' AND lower(barcode)=lower($2) AND order_id=$3 AND canonical_apparatus_id=$4 ORDER BY id DESC LIMIT 1")
            .bind(token).bind(field(original,"barcode")).bind(&session.order_id).bind(&session.apparatus)
            .fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?.ok_or_else(denied)?;
        sqlx::query("UPDATE mini_raw_material_stock SET status='consumed',payload_json=(payload_json-'in_use_order_id')||jsonb_build_object('consumed_order_id',$2::text),updated_at=now() WHERE id=$1")
            .bind(field(original,"id")).bind(&session.order_id).execute(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        insert_raw_material_event_tx(tx, RawMaterialEventDraft {
            idempotency_key: format!("reactivation-reclose:{token}:{}", field(original,"id")),
            event_type: "consumption_posted".into(), warehouse:field(original,"warehouse").into(),
            barcode:field(original,"barcode").into(),item_code:field(original,"item_code").into(),item_name:field(original,"item_name").into(),
            qty_delta:-original["qty"].as_f64().ok_or_else(denied)?,uom:field(original,"uom").into(),
            stock_status_before:Some("in_use".into()),stock_status_after:Some("consumed".into()),
            order_id:Some(session.order_id.clone()),apparatus:Some(session.apparatus.clone()),
            actor_role:actor.role.clone(),actor_ref:actor.ref_.clone(),actor_display_name:actor.display_name.clone(),
            owner_role:owner.0,owner_ref:owner.1,owner_display_name:owner.2,source_type:"consumption".into(),
            source_id:session.order_id.clone(),source_line_ref:Some(field(original,"barcode").into()),correlation_id:Some(token.into()),
            payload_json:json!({"stock_id":field(original,"id"),"session_id":session.session_id,
                "reactivation_reclose_source_session_id":source_id,"reason_code":"reactivation_reclosed_without_work"}),
        }).await.map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}
