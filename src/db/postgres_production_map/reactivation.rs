//! Premature completion correction, serialized with production, intake and stock writes.
use super::*;
use crate::core::production_map::{
    OrderProgressBatchWipStatus, OrderRunInputStatus, OrderRunStatus, ReactivationPreview,
    ReactivationSelector, ReactivationTarget, chain, order_run_input_links_from_payload,
    write_order_run_input_links,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use std::collections::BTreeSet;
pub(super) mod reclose;

struct Prepared {
    target: ReactivationTarget,
    session: OrderRunSession,
    outputs: Vec<OrderProgressBatch>,
    input: Option<OrderProgressBatch>,
    opening_input: Option<Value>,
    policy: crate::core::production_map::ApparatusQueuePolicy,
    raw: Vec<Value>,
    queue_state: String,
    latest_started: i64,
    latest_session_id: String,
}
macro_rules! deny {
    ($reason:literal) => {
        return Ok(Err($reason))
    };
}
fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    selector: &ReactivationSelector,
    actor: &QueueActionActor,
    is_print: bool,
) -> Result<Result<Prepared, &'static str>, ProductionMapError> {
    transaction_locks::lock_order_and_apparatuses_tx(
        tx,
        &selector.order_id,
        &[&selector.apparatus],
    )
    .await?;
    let apparatus_config:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM mini_apparatus a WHERE id=$1 AND lifecycle_state='active' FOR SHARE")
        .bind(&selector.apparatus).fetch_optional(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    let Some(apparatus_config) = apparatus_config else {
        deny!("stale_target");
    };
    let Some((map_json, lifecycle)) = sqlx::query_as::<_, (Value, String)>(
        "SELECT map_json,lifecycle_status FROM mini_production_maps WHERE id=$1 FOR UPDATE",
    )
    .bind(&selector.order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    else {
        deny!("stale_target");
    };
    if matches!(lifecycle.as_str(), "closed" | "cancelled") {
        deny!("order_closed");
    }
    let map: ProductionMapDefinition =
        serde_json::from_value(map_json.clone()).map_err(|_| ProductionMapError::StoreFailed)?;
    let control: Option<(String, Option<Value>)> = sqlx::query_as(
        "SELECT state,early_close FROM mini_order_control_states WHERE order_id=$1 FOR UPDATE",
    )
    .bind(&selector.order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if control.is_some_and(|(state, early)| state != "active" || early.is_some()) {
        deny!("order_closed");
    }
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_queue_states WHERE canonical_apparatus_id=$1 AND state IN ('in_progress','print_preflight')) OR EXISTS(SELECT 1 FROM mini_order_run_sessions WHERE canonical_apparatus_id=$1 AND status='active') OR EXISTS(SELECT 1 FROM mini_print_preflight_holds WHERE canonical_apparatus_id=$1 AND status IN ('held','running','passed'))")
        .bind(&selector.apparatus).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if busy {
        deny!("apparatus_busy");
    }
    let sessions = stage_execution::sessions_tx(tx, &selector.order_id).await?;
    if sessions
        .iter()
        .any(|s| s.apparatus == selector.apparatus && s.status.is_open())
    {
        deny!("apparatus_busy");
    }
    let selected = if is_print {
        if !selector.batch_id.is_empty() {
            deny!("stale_target");
        }
        let candidates = sessions
            .iter()
            .filter(|s| {
                s.apparatus == selector.apparatus
                    && s.status == OrderRunStatus::Completed
                    && s.payload_json.get("_reactivation_reclosed_without_work").and_then(Value::as_bool) != Some(true)
                    && s.worker_role == actor.role
                    && s.worker_ref == actor.ref_
            })
            .collect::<Vec<_>>();
        if candidates
            .iter()
            .map(|s| s.stage_node_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != 1
        {
            deny!("unsupported_history");
        }
        candidates
            .into_iter()
            .max_by_key(|s| (s.started_at_unix, &s.session_id))
            .cloned()
    } else {
        if selector.batch_id.is_empty() {
            deny!("stale_target");
        }
        let Some(batch) =
            order_query_helpers::load_progress_batch(&mut **tx, &selector.batch_id).await?
        else {
            deny!("stale_target");
        };
        if batch.order_id != selector.order_id || batch.apparatus != selector.apparatus {
            deny!("stale_target");
        }
        sessions
            .iter()
            .find(|s| s.session_id == batch.session_id)
            .cloned()
    };
    let Some(session) = selected else {
        deny!("stale_target");
    };
    if session.status != OrderRunStatus::Completed
        || session.worker_role != actor.role
        || session.worker_ref != actor.ref_
        || session.stage_node_id.is_empty()
    {
        deny!("unsupported_history");
    }
    if sessions.iter().any(|s| {
        s.payload_json
            .get("_reactivation_of_session_id")
            .and_then(Value::as_str)
            == Some(session.session_id.as_str())
            && s.payload_json.get("_reactivation_reclosed_without_work").and_then(Value::as_bool) != Some(true)
    }) {
        deny!("stale_target");
    }
    if chain::work_stage_for_station(&map, &selector.apparatus, &session.stage_node_id)
        .is_none_or(|stage| stage.node_id != session.stage_node_id)
    {
        deny!("stale_target");
    }
    let states: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> = {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT canonical_apparatus_id,order_id,state FROM mini_queue_states WHERE order_id=$1",
        )
        .bind(&selector.order_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut result = std::collections::BTreeMap::new();
        for (apparatus, order, state) in rows {
            result
                .entry(apparatus)
                .or_insert_with(std::collections::BTreeMap::new)
                .insert(order, state);
        }
        result
    };
    let events = sqlx::query_as::<_,(String,String,String)>("SELECT stage_node_id,action,to_state FROM mini_queue_action_events WHERE order_id=$1 AND stage_node_id<>'' ORDER BY id")
        .bind(&selector.order_id).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?.into_iter().filter_map(|(stage_node_id,action,state)| Some(crate::core::production_map::ProductionStageLifecycleEvent { stage_node_id, action: crate::core::production_map::queue_state::ApparatusQueueAction::parse(&action)?, to_state: crate::core::production_map::queue_state::ApparatusQueueOrderState::parse(&state)? })).collect::<Vec<_>>();
    let stage_inputs = stage_execution::inputs_tx(tx, &selector.order_id).await?;
    let stage_work = crate::core::production_map::stage_execution::stage_work_statuses(
        &map,
        &sessions,
        &stage_inputs,
        &states,
        &events,
    );
    if stage_work
        .iter()
        .any(|work| chain::is_final_work_stage_node(&map, &work.stage_node_id) && work.completed)
    {
        deny!("final_stage_closed");
    }
    // Reactivation must anchor an actual Complete, never an issue/forced closure.
    let report_id = session
        .payload_json
        .get("_completion_queue_event_id")
        .and_then(Value::as_str)
        .or_else(|| {
            session
                .payload_json
                .get("stage_work_report")
                .and_then(|p| p.get("report_id"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default();
    // A later astatka report has its own report ID. Prefer a real queue receipt;
    // otherwise require a single completion at the source session boundary.
    let exact_anchor:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_queue_action_events WHERE event_id=$1 AND order_id=$2 AND canonical_apparatus_id=$3 AND stage_node_id=$4 AND action='complete')")
        .bind(report_id).bind(&selector.order_id).bind(&selector.apparatus).bind(&session.stage_node_id).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    let report_id = if exact_anchor { report_id } else { "" };
    let mut closings: Vec<(Value,String)> = sqlx::query_as("SELECT payload_json,policy FROM mini_queue_action_events WHERE order_id=$1 AND canonical_apparatus_id=$2 AND stage_node_id=$3 AND action='complete' AND actor_role=$4 AND actor_ref=$5 AND (($6<>'' AND event_id=$6) OR ($6='' AND extract(epoch FROM created_at)>=$7 AND extract(epoch FROM created_at)<$8)) ORDER BY id DESC LIMIT 2")
        .bind(&selector.order_id).bind(&selector.apparatus).bind(&session.stage_node_id).bind(&actor.role).bind(&actor.ref_).bind(report_id).bind(session.updated_at_unix as f64).bind((session.updated_at_unix+1) as f64).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if closings.len() != 1 {
        deny!("unsupported_history");
    }
    let Some((closing, policy)) = closings.pop() else {
        deny!("unsupported_history");
    };
    if closing.get("completed_with_issue").and_then(Value::as_bool) == Some(true)
        || closing.get("completion_request").and_then(Value::as_bool) == Some(true)
    {
        deny!("unsupported_history");
    }
    let policy = crate::core::production_map::ApparatusQueuePolicy::parse(&policy)
        .ok_or(ProductionMapError::StoreFailed)?;
    let mut output_ids = if is_print {
        Vec::new()
    } else {
        sqlx::query_scalar::<_,String>("SELECT batch_id FROM mini_progress_batches WHERE session_id=$1 AND action='complete' ORDER BY batch_id")
            .bind(&session.session_id).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?
    };
    if !is_print {
        let reports: Option<Value> = sqlx::query_scalar("SELECT payload_json FROM mini_order_progress_events WHERE session_id=$1 AND action='complete' ORDER BY id DESC LIMIT 1")
            .bind(&session.session_id).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(report) = reports {
            if let Some(ids) = report
                .get("rezka_previously_recorded_batches")
                .and_then(Value::as_array)
            {
                output_ids.extend(ids.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
    }
    output_ids.sort();
    output_ids.dedup();
    if !is_print && !output_ids.contains(&selector.batch_id) {
        deny!("unsupported_history");
    }
    // Lock packages before their rolls, matching receipt/movement lock order.
    let paddons = sqlx::query_as::<_,(String,bool)>("SELECT p.id,(p.locked_at IS NOT NULL OR p.receipt_json IS NOT NULL) FROM mini_paddons p WHERE EXISTS(SELECT 1 FROM mini_paddon_items i WHERE i.paddon_id=p.id AND i.progress_batch_id=ANY($1) AND i.removed_at IS NULL) ORDER BY p.id FOR UPDATE")
        .bind(&output_ids).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if paddons.iter().any(|(_, locked)| *locked) {
        deny!("source_not_free");
    }
    sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=ANY($1) ORDER BY batch_id FOR UPDATE")
        .bind(&output_ids).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let mut outputs = Vec::new();
    for id in &output_ids {
        let Some(batch) = order_query_helpers::load_progress_batch(&mut **tx, id).await? else {
            deny!("source_not_free");
        };
        if batch.session_id != session.session_id
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
            || !batch.used_by_session_id.is_empty()
            || !batch.processed_by_session_id.is_empty()
            || !batch.used_by_apparatus.is_empty()
            || !batch.processed_by_apparatus.is_empty()
            || batch.is_layer_remainder()
            || !field(&batch.payload_json, "finished_goods_stock_id").is_empty()
            || !field(&batch.payload_json, "received_warehouse").is_empty()
        {
            deny!("source_not_free");
        }
        let external: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_paddon_receipt_lines WHERE progress_batch_id=$1) OR EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='progress_batch' AND source_id=$1) OR EXISTS(SELECT 1 FROM mini_paddon_items i JOIN mini_paddons p ON p.id=i.paddon_id WHERE i.progress_batch_id=$1 AND i.removed_at IS NULL AND (NOT(p.id=ANY($2)) OR p.locked_at IS NOT NULL OR p.receipt_json IS NOT NULL))")
            .bind(id).bind(paddons.iter().map(|(id,_)| id.clone()).collect::<Vec<_>>()).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if external {
            deny!("source_not_free");
        }
        let previously_used:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_order_run_sessions WHERE session_id<>$2 AND (payload_json->>'input_progress_batch_id'=$1 OR EXISTS(SELECT 1 FROM jsonb_array_elements(CASE WHEN jsonb_typeof(payload_json->'input_lineage')='array' THEN payload_json->'input_lineage' ELSE '[]'::jsonb END) link WHERE link->>'input_batch_id'=$1)))")
            .bind(id).bind(&session.session_id).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
        if previously_used {
            deny!("source_not_free");
        }
        outputs.push(batch);
    }
    let input_id = field(&session.payload_json, "input_progress_batch_id");
    let source_kind = field(&session.payload_json, "input_wip_source_kind");
    let opening_input = if source_kind == "opening_wip" {
        let row:Option<Value>=sqlx::query_scalar("SELECT to_jsonb(b) FROM mini_opening_wip_batches b WHERE batch_id=$1 AND order_id=$2 FOR UPDATE")
            .bind(input_id).bind(&selector.order_id).fetch_optional(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
        let Some(mut row) = row else {
            deny!("source_not_free");
        };
        if field(&row, "wip_status") != "processed"
            || field(&row, "processed_by_session_id") != session.session_id
            || field(&row, "processed_by_apparatus") != selector.apparatus
        {
            deny!("source_not_free");
        }
        let intake: Option<Value> = sqlx::query_scalar(
            "SELECT to_jsonb(i) || jsonb_build_object(
                'resume_apparatus', COALESCE(resume_apparatus, ''),
                'actor', jsonb_build_object('role', actor_role, 'ref_', actor_ref, 'display_name', actor_display_name),
                'created_at_unix', extract(epoch FROM created_at)::bigint,
                'updated_at_unix', extract(epoch FROM updated_at)::bigint)
             FROM mini_opening_wip_intakes i WHERE intake_id=$1 FOR SHARE",
        )
        .bind(field(&row, "intake_id"))
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let Some(intake_json) = intake else {
            deny!("source_not_free");
        };
        let intake: crate::core::production_map::OpeningWipIntake =
            serde_json::from_value(intake_json.clone())
                .map_err(|_| ProductionMapError::StoreFailed)?;
        if intake.status != crate::core::production_map::OpeningWipIntakeStatus::Confirmed
            || intake.order_id != selector.order_id
        {
            deny!("source_not_free");
        }
        if crate::core::production_map::ProductionMapService::opening_wip_target_stage(
            &map,
            &intake,
            &selector.apparatus,
            &session.stage_node_id,
        )
        .is_none_or(|stage| stage.node_id != session.stage_node_id)
        {
            deny!("stale_target");
        }
        row["_reactivation_intake_snapshot"] = intake_json;
        Some(row)
    } else {
        None
    };
    let links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
    if links.len() > 1
        && session
            .payload_json
            .get("_completion_reactivation_payload")
            .is_none()
    {
        deny!("unsupported_history");
    }
    let input = if opening_input.is_some() {
        None
    } else if input_id.is_empty() {
        if !is_print {
            deny!("unsupported_history");
        }
        None
    } else {
        sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE")
            .bind(input_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let Some(batch) = order_query_helpers::load_progress_batch(&mut **tx, input_id).await?
        else {
            deny!("source_not_free");
        };
        if batch.order_id != selector.order_id
            || batch.wip_status != OrderProgressBatchWipStatus::Processed
            || batch.processed_by_session_id != session.session_id
            || batch.processed_by_apparatus != selector.apparatus
        {
            deny!("source_not_free");
        }
        let returned: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='progress_batch' AND source_id=$1)").bind(input_id).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if returned {
            deny!("source_not_free");
        }
        if crate::core::production_map::wip_route::resolve_wip_input_route(&map, &batch).is_ok_and(
            |route| {
                route.stage_node_id == session.stage_node_id
                    && route.consumer_apparatus_ids.contains(&selector.apparatus)
            },
        ) == false
        {
            deny!("stale_target");
        }
        Some(batch)
    };
    let mut raw=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(s) FROM mini_raw_material_stock s JOIN mini_raw_material_assignments a ON lower(a.barcode)=lower(s.barcode) WHERE a.order_id=$1 AND a.canonical_apparatus_id=$2 ORDER BY s.id FOR UPDATE OF s,a")
        .bind(&selector.order_id).bind(&selector.apparatus).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let recorded_resources = ProductionQrSessionResources::for_session(&session);
    let unlinked = closing
        .get("unused_raw_material_unlinks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if recorded_resources.raw_materials.iter().any(|material| {
        !raw.iter()
            .any(|row| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
            && !unlinked
                .iter()
                .any(|row| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
    }) {
        deny!("resource_unavailable");
    }
    if recorded_resources.raw_materials.is_empty() && !raw.is_empty() {
        deny!("unsupported_history");
    }
    raw.retain(|row| {
        recorded_resources
            .raw_materials
            .iter()
            .any(|material| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
    });
    for stock in &raw {
        let source_material = recorded_resources
            .raw_materials
            .iter()
            .find(|material| field(stock, "barcode").eq_ignore_ascii_case(&material.barcode))
            .ok_or(ProductionMapError::RawMaterialStockUnavailable)?;
        if source_material.item_code != field(stock, "item_code")
            || source_material
                .stock_id
                .as_deref()
                .is_some_and(|id| id != field(stock, "id"))
            || source_material
                .uom
                .as_deref()
                .is_some_and(|uom| uom != field(stock, "uom"))
        {
            deny!("resource_unavailable");
        }
        if !matches!(field(stock, "status"), "consumed" | "in_use")
            || field(stock, "reserved_order_id") != selector.order_id
        {
            deny!("resource_unavailable");
        }
        let source_used: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='raw_material' AND source_id=$1)").bind(field(stock,"id")).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if source_used {
            deny!("resource_unavailable");
        }
        if field(stock, "status") == "consumed" {
            let event: Option<(String,f64,bool)> = sqlx::query_as("SELECT e.event_id,e.qty_delta::float8,(e.payload_json->>'stock_id'=$4 AND ((extract(epoch FROM e.occurred_at)>=$5 AND extract(epoch FROM e.occurred_at)<$6) OR e.payload_json->>'reactivation_reclose_source_session_id'=$7)) FROM mini_raw_material_events e WHERE lower(e.barcode)=lower($1) AND e.order_id=$2 AND e.canonical_apparatus_id=$3 AND e.event_type='consumption_posted' AND NOT EXISTS(SELECT 1 FROM mini_raw_material_events r WHERE r.event_type='consumption_reversed' AND r.payload_json->>'reversed_event_id'=e.event_id) ORDER BY e.id DESC LIMIT 1")
                .bind(field(stock,"barcode")).bind(&selector.order_id).bind(&selector.apparatus).bind(field(stock,"id")).bind(session.started_at_unix as f64).bind((session.updated_at_unix+1) as f64).bind(&session.session_id).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
            if event.is_none_or(|(_, qty, eligible)| {
                !eligible
                    || stock
                        .get("qty")
                        .and_then(Value::as_f64)
                        .is_none_or(|expected| (qty + expected).abs() > 0.000001)
            }) {
                deny!("resource_unavailable");
            }
        }
    }
    if is_print {
        sqlx::query(
            "SELECT pg_advisory_xact_lock_shared(hashtextextended('qolip:product-composition',0))",
        )
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut active = session.clone();
        active.status = OrderRunStatus::Active;
        for outcome in [
            qolip_session_helpers::reject_qolip_in_use_tx(tx, &active).await,
            qolip_session_helpers::validate_qolip_set_tx(tx, &active).await,
        ] {
            if let Err(error) = outcome {
                if error == ProductionMapError::StoreFailed {
                    return Err(error);
                }
                deny!("resource_unavailable");
            }
        }
        let codes = session
            .payload_json
            .get("qolip_codes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        for code in codes {
            let held_elsewhere:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_qolip_checkouts WHERE lower(qolip_code)=lower($1) AND status='open' AND lower(issued_to_ref)<>lower($2))")
                .bind(code).bind(&actor.ref_).fetch_one(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
            if held_elsewhere {
                deny!("resource_unavailable");
            }
        }
    }
    let queue_state = states
        .get(&selector.apparatus)
        .and_then(|s| s.get(&selector.order_id))
        .cloned()
        .unwrap_or("pending".into());
    if !matches!(queue_state.as_str(), "completed" | "pending") {
        deny!("apparatus_busy");
    }
    let continuations = sessions.iter().filter(|s| field(&s.payload_json, "_reactivation_of_session_id") == session.session_id).map(|s| &s.session_id).collect::<BTreeSet<_>>();
    let token=format!("{:x}",Sha256::digest(serde_json::to_vec(&json!({"apparatus_config":apparatus_config,"map":map_json,"session":session,"outputs":outputs,"input":input,"opening_input":opening_input,"raw":raw,"actor_role":actor.role,"actor_ref":actor.ref_,"queue":queue_state,"continuations":continuations})).map_err(|_| ProductionMapError::StoreFailed)?));
    let target = ReactivationTarget {
        apparatus: selector.apparatus.clone(),
        order_id: selector.order_id.clone(),
        batch_id: selector.batch_id.clone(),
        session_id: session.session_id.clone(),
        stage_node_id: session.stage_node_id.clone(),
        token,
    };
    Ok(Ok(Prepared {
        target,
        session,
        outputs,
        input,
        opening_input,
        policy,
        raw,
        queue_state,
        latest_started: sessions
            .iter()
            .filter(|s| s.apparatus == selector.apparatus)
            .map(|s| s.started_at_unix)
            .max()
            .unwrap_or(0),
        latest_session_id: sessions
            .iter()
            .filter(|s| s.apparatus == selector.apparatus)
            .max_by_key(|s| (s.started_at_unix, &s.session_id))
            .map(|s| s.session_id.clone())
            .unwrap_or_default(),
    }))
}

pub(super) async fn preview(
    store: &PostgresProductionMapStore,
    selector: &ReactivationSelector,
    actor: &QueueActionActor,
    is_print: bool,
) -> Result<ReactivationPreview, ProductionMapError> {
    let mut tx = store
        .pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let result = prepare(&mut tx, selector, actor, is_print).await?;
    tx.rollback()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(match result {
        Ok(prepared) => ReactivationPreview {
            available: true,
            denial_reason: None,
            target: Some(prepared.target),
        },
        Err(reason) => ReactivationPreview::denied(reason),
    })
}

type ReactivationReceipt = (String, String, String, String, String, Value);

fn receipt_matches(receipt: &ReactivationReceipt, target: &ReactivationTarget, actor: &QueueActionActor) -> Result<bool, ProductionMapError> {
    if receipt.0 != target.order_id || receipt.1 != target.apparatus
        || receipt.2 != actor.role || receipt.3 != actor.ref_
        || receipt.4 != target.stage_node_id
        || field(&receipt.5, "source_session_id") != target.session_id
        || field(&receipt.5, "batch_id") != target.batch_id
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(true)
}

pub(super) async fn was_applied(store: &PostgresProductionMapStore, target: &ReactivationTarget, actor: &QueueActionActor) -> Result<bool, ProductionMapError> {
    let receipt: Option<ReactivationReceipt> = sqlx::query_as(
        "SELECT order_id,canonical_apparatus_id,actor_role,actor_ref,stage_node_id,payload_json FROM mini_queue_action_events WHERE event_id=$1"
    ).bind(format!("reactivation:{}", target.token)).fetch_optional(&store.pool).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    receipt.as_ref().map(|receipt| receipt_matches(receipt, target, actor)).transpose().map(|result| result.unwrap_or(false))
}

pub(super) async fn execute(
    store: &PostgresProductionMapStore,
    target: &ReactivationTarget,
    actor: &QueueActionActor,
    assigned: &[String],
    checkouts: &[crate::core::qolip::QolipCheckout],
    is_print: bool,
) -> Result<(), ProductionMapError> {
    if actor.role != "aparatchi"
        || actor.ref_.is_empty()
        || !assigned.iter().any(|id| id == &target.apparatus)
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
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
    let event_id = format!("reactivation:{}", target.token);
    let receipt:Option<(String,String,String,String,String,Value)>=sqlx::query_as("SELECT order_id,canonical_apparatus_id,actor_role,actor_ref,stage_node_id,payload_json FROM mini_queue_action_events WHERE event_id=$1").bind(&event_id).fetch_optional(&mut *tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    if let Some(receipt) = receipt {
        receipt_matches(&receipt, target, actor)?;
        return Ok(());
    }
    super::qolip_rework_availability::require_order_qolips_available_tx(
        &mut tx, &target.order_id,
    ).await?;
    let prepared = prepare(
        &mut tx,
        &ReactivationSelector {
            apparatus: target.apparatus.clone(),
            order_id: target.order_id.clone(),
            batch_id: target.batch_id.clone(),
        },
        actor,
        is_print,
    )
    .await?
    .map_err(|reason| ProductionMapError::ReactivationDenied(reason.into()))?;
    if serde_json::to_value(&prepared.target).ok() != serde_json::to_value(target).ok() {
        return Err(ProductionMapError::ReactivationDenied(
            "stale_target".into(),
        ));
    }
    let source = &prepared.session;
    let now = crate::core::production_map::stage_execution::report_now();
    if prepared.latest_started > now {
        return Err(ProductionMapError::ReactivationDenied(
            "unsupported_history".into(),
        ));
    }
    // Preserve wall time while making a continuation the latest execution when
    // several sessions start in the same second. Extending the maximum ID is
    // strictly greater than that ID under the projection's existing ordering.
    let continuation_id = if prepared.latest_started == now {
        format!(
            "{}:continuation:{}",
            prepared.latest_session_id, target.token
        )
    } else {
        format!("continuation:{}", target.token)
    };
    let mut payload = json!({"stage_work_protocol":1,"_reactivation_of_session_id":source.session_id,"_reactivation_reserved_outputs":prepared.outputs.iter().map(|b|b.batch_id.clone()).collect::<Vec<_>>(),"reactivation_token":target.token});
    let original = source
        .payload_json
        .get("_completion_reactivation_payload")
        .unwrap_or(&source.payload_json);
    for key in [
        "input_progress_batch_id",
        "input_progress_qr_payload",
        "input_progress_apparatus",
        "input_wip_source_kind",
        "contained_kadr_count",
        "input_lineage",
        "rezka_active_partial_rolls",
        "rezka_recorded_kadr_counts",
        "qolip_code",
        "qolip_codes",
        "qolip_set_id",
        "qolip_set_snapshot",
        "qolip_lock_owner",
    ] {
        if let Some(value) = original.get(key) {
            payload[key] = value.clone();
        }
    }
    let mut links = order_run_input_links_from_payload(&payload)
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
    for link in &mut links {
        if link.input_batch_id == field(&source.payload_json, "input_progress_batch_id") {
            link.status = OrderRunInputStatus::InUse;
            link.processed_at_unix = None;
        }
    }
    write_order_run_input_links(&mut payload, &links);
    let cycle_stamp = u128::from_str_radix(&target.token[..32], 16)
        .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
    payload["rezka_output_cycle"] = json!(format!("progress:{cycle_stamp}:reactivation"));
    payload["rezka_output_revision"] = json!(0);
    let mut session = OrderRunSession {
        session_id: continuation_id,
        apparatus: source.apparatus.clone(),
        order_id: source.order_id.clone(),
        stage_node_id: source.stage_node_id.clone(),
        status: OrderRunStatus::Paused,
        worker_role: actor.role.clone(),
        worker_ref: actor.ref_.clone(),
        worker_display_name: actor.display_name.clone(),
        started_at_unix: now,
        updated_at_unix: now,
        payload_json: payload,
    };
    let mut resources = ProductionQrSessionResources::for_session(source);
    resources.session_id = session.session_id.clone();
    resources.raw_materials.retain(|material| {
        prepared
            .raw
            .iter()
            .any(|row| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
    });
    resources.raw_materials_available = !resources.raw_materials.is_empty();
    if is_print && resources.qolip_codes.is_empty() {
        resources.qolip_codes = session
            .payload_json
            .get("qolip_codes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if resources.qolip_codes.is_empty()
            && !field(&session.payload_json, "qolip_code").is_empty()
        {
            resources
                .qolip_codes
                .push(field(&session.payload_json, "qolip_code").into());
        }
    }
    resources.qolip_available = is_print && !resources.qolip_codes.is_empty();
    if resources.qolip_available {
        session.payload_json["qolip_lock_owner"] = json!(true);
    }
    if !is_print {
        resources.qolip_codes.clear();
    }
    resources.write_to_session(&mut session);
    let assignments: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(a)-'updated_at' FROM mini_raw_material_assignments a WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY barcode FOR UPDATE")
        .bind(&target.order_id).bind(&target.apparatus).fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let map: Value = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&target.order_id).fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    session.payload_json["_reactivation_reclose"] = json!({
        "input":prepared.input, "opening_input":prepared.opening_input, "raw":prepared.raw,
        "assignments":assignments, "map":map, "payload":session.payload_json.clone(),
    });
    // Tool composition and physical checkout validation are the same as Start.
    sqlx::query(
        "SELECT pg_advisory_xact_lock_shared(hashtextextended('qolip:product-composition',0))",
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    qolip_session_helpers::reject_qolip_in_use_tx(&mut tx, &session).await?;
    if is_print {
        qolip_session_helpers::validate_qolip_set_tx(&mut tx, &session).await?;
    }
    for checkout in checkouts {
        if !is_print
            || !resources
                .qolip_codes
                .iter()
                .any(|code| code.eq_ignore_ascii_case(&checkout.qolip_code))
            || checkout.issued_to_ref != actor.ref_
        {
            return Err(ProductionMapError::QolipCodeMismatch);
        }
        crate::db::postgres_qolip::save_checkout_tx(&mut tx, checkout, Some(&session.session_id))
            .await
            .map_err(production_map_qolip_checkout_error)?;
    }
    if is_print {
        qolip_session_helpers::validate_reacquired_qolip_checkouts_tx(&mut tx, &session).await?;
    }
    for stock in &prepared.raw {
        reverse_consumption(&mut tx, stock, target, actor).await?;
    }
    if let Some(mut input) = prepared.input {
        input.wip_status = OrderProgressBatchWipStatus::InUse;
        input.used_by_session_id = session.session_id.clone();
        input.used_by_apparatus = session.apparatus.clone();
        input.processed_by_session_id.clear();
        input.processed_by_apparatus.clear();
        input.current_apparatus = session.apparatus.clone();
        input.current_location = session.apparatus.clone();
        put_order_progress_batch_tx(&mut tx, &input).await?;
    }
    if let Some(opening) = prepared.opening_input {
        let updated=sqlx::query("UPDATE mini_opening_wip_batches SET wip_status='in_use',used_by_session_id=$2,used_by_apparatus=$3,processed_by_session_id='',processed_by_apparatus='',updated_at=now() WHERE batch_id=$1 AND wip_status='processed' AND processed_by_session_id=$4")
            .bind(field(&opening,"batch_id")).bind(&session.session_id).bind(&session.apparatus).bind(&source.session_id).execute(&mut *tx).await.map_err(|_|ProductionMapError::StoreFailed)?.rows_affected();
        if updated != 1 {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
    }
    for mut output in prepared.outputs {
        output.wip_status = OrderProgressBatchWipStatus::InUse;
        output.used_by_session_id = session.session_id.clone();
        output.used_by_apparatus = session.apparatus.clone();
        output.current_apparatus = session.apparatus.clone();
        output.payload_json["_reactivation_reservation"] = json!(session.session_id);
        put_order_progress_batch_tx(&mut tx, &output).await?;
    }
    let event = ApparatusQueueActionEvent {
        event_id: event_id.clone(),
        apparatus: target.apparatus.clone(),
        order_id: target.order_id.clone(),
        stage_node_id: target.stage_node_id.clone(),
        action: crate::core::production_map::queue_state::ApparatusQueueAction::Resume,
        from_state: crate::core::production_map::queue_state::ApparatusQueueOrderState::parse(
            &prepared.queue_state,
        )
        .ok_or(ProductionMapError::QueueActionNotAllowed)?,
        to_state: crate::core::production_map::queue_state::ApparatusQueueOrderState::Paused,
        policy: prepared.policy,
        actor: actor.clone(),
        assigned_apparatus: assigned.to_vec(),
        payload_json: json!({"event":"reactivation","source_session_id":source.session_id,"session_id":session.session_id,"token":target.token,"batch_id":target.batch_id,"additional_output_only":true}),
    };
    put_queue_action_state_tx(&mut tx, &event).await?;
    insert_queue_action_event_tx(&mut tx, &event).await?;
    put_order_run_session_tx(&mut tx, &session).await?;
    let progress:OrderProgressEvent=serde_json::from_value(json!({"event_id":format!("progress:{event_id}"),"session_id":session.session_id,"batch_id":"","apparatus":target.apparatus,"order_id":target.order_id,"action":"resume","produced_qty":0.0,"uom":"","worker_role":actor.role,"worker_ref":actor.ref_,"worker_display_name":actor.display_name,"description":"","payload_json":event.payload_json})).map_err(|_|ProductionMapError::StoreFailed)?;
    put_order_progress_event_tx(&mut tx, &progress).await?;
    // Only this explicit, validated correction can lower an automatically derived lifecycle.
    let reopened_version:Option<i64>=sqlx::query_scalar("UPDATE mini_production_maps SET lifecycle_status='in_progress',production_completed_at=NULL,lifecycle_changed_at=now(),lifecycle_version=lifecycle_version+1 WHERE id=$1 AND lifecycle_status='production_completed' RETURNING lifecycle_version").bind(&target.order_id).fetch_optional(&mut *tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    if let Some(version) = reopened_version {
        sqlx::query("INSERT INTO mini_production_order_lifecycle_events(event_id,order_id,from_status,to_status,completion_outcome,actor_role,actor_ref,actor_display_name,source_event_id,reason,lifecycle_version) VALUES($1,$2,'production_completed','in_progress','',$3,$4,$5,$6,'premature_completion_reactivated',$7)")
            .bind(format!("lifecycle:{event_id}:in_progress")).bind(&target.order_id).bind(&actor.role).bind(&actor.ref_).bind(&actor.display_name).bind(&event_id).bind(version).execute(&mut *tx).await.map_err(|_|ProductionMapError::StoreFailed)?;
    }
    update_apparatus_schedule_reservation_status_tx(
        &mut tx,
        &target.order_id,
        &ApparatusId::new(target.apparatus.clone()).map_err(|_| ProductionMapError::StoreFailed)?,
        crate::core::production_map::ApparatusScheduleStatus::Paused,
        actor,
    )
    .await?;
    // Ordinary scheduling deliberately cannot reopen completed reservations.
    // Only this validated correction may put one back into paused work.
    sqlx::query("UPDATE mini_apparatus_schedule_reservations SET status='paused',actor_json=$3 WHERE order_id=$1 AND canonical_apparatus_id=$2 AND status='completed'")
        .bind(&target.order_id).bind(&target.apparatus).bind(json!(actor))
        .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    lifecycle::refresh_production_order_lifecycle_tx(
        &mut tx,
        &target.order_id,
        actor,
        &event_id,
        "premature_completion_reactivated",
    )
    .await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

async fn reverse_consumption(
    tx: &mut Transaction<'_, Postgres>,
    stock: &Value,
    target: &ReactivationTarget,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    if field(stock, "status") != "consumed" {
        return Ok(());
    }
    let (event_id,qty,owner_role,owner_ref,owner_display_name):(String,f64,String,String,String)=sqlx::query_as("SELECT e.event_id,e.qty_delta::float8,COALESCE(e.owner_role,''),COALESCE(e.owner_ref,''),COALESCE(e.owner_display_name,'') FROM mini_raw_material_events e WHERE lower(e.barcode)=lower($1) AND e.order_id=$2 AND e.canonical_apparatus_id=$3 AND e.event_type='consumption_posted' AND NOT EXISTS(SELECT 1 FROM mini_raw_material_events r WHERE r.event_type='consumption_reversed' AND r.payload_json->>'reversed_event_id'=e.event_id) ORDER BY e.id DESC LIMIT 1 FOR UPDATE")
        .bind(field(stock,"barcode")).bind(&target.order_id).bind(&target.apparatus).fetch_optional(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?.ok_or(ProductionMapError::RawMaterialStockUnavailable)?;
    let affected=sqlx::query("UPDATE mini_raw_material_stock SET status='in_use',payload_json=(payload_json-'consumed_order_id')||jsonb_build_object('in_use_order_id',$2::text),updated_at=now() WHERE id=$1 AND status='consumed' AND reserved_order_id=$2")
        .bind(field(stock,"id")).bind(&target.order_id).execute(&mut **tx).await.map_err(|_|ProductionMapError::StoreFailed)?.rows_affected();
    if affected != 1 {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    insert_raw_material_event_tx(tx,RawMaterialEventDraft {idempotency_key:format!("consumption-reversed:{event_id}"),event_type:"consumption_reversed".into(),warehouse:field(stock,"warehouse").into(),barcode:field(stock,"barcode").into(),item_code:field(stock,"item_code").into(),item_name:field(stock,"item_name").into(),qty_delta:-qty,uom:field(stock,"uom").into(),stock_status_before:Some("consumed".into()),stock_status_after:Some("in_use".into()),order_id:Some(target.order_id.clone()),apparatus:Some(target.apparatus.clone()),actor_role:actor.role.clone(),actor_ref:actor.ref_.clone(),actor_display_name:actor.display_name.clone(),owner_role,owner_ref,owner_display_name,source_type:"consumption".into(),source_id:target.order_id.clone(),source_line_ref:Some(field(stock,"barcode").into()),correlation_id:Some(target.token.clone()),payload_json:json!({"reason_code":"premature_wip_completion","reversed_event_id":event_id,"stock_id":field(stock,"id"),"session_id":target.session_id})}).await.map_err(|_|ProductionMapError::StoreFailed)?;
    Ok(())
}

/// The reservation belongs to continuation work, never to a downstream consumer.
/// Called for every session writer, including handoff, freeze and remainder paths.
pub(super) async fn release_reservations_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &mut OrderRunSession,
) -> Result<(), ProductionMapError> {
    let previous: Option<Value> = sqlx::query_scalar(
        "SELECT payload_json FROM mini_order_run_sessions WHERE session_id=$1 FOR UPDATE",
    )
    .bind(&session.session_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(previous) = previous else {
        return Ok(());
    };
    // This latch survives Resume -> re-pause and stale payload rewrites. Starting
    // work is irreversible for the no-work close, even if no quantity was saved.
    if previous.get("_reactivation_started").and_then(Value::as_bool) == Some(true)
        || (previous.get("_reactivation_reclose").is_some()
            && session.status != OrderRunStatus::Paused
            && session.payload_json.get("_reactivation_reclosed_without_work").and_then(Value::as_bool) != Some(true))
    {
        session.payload_json["_reactivation_started"] = json!(true);
        session.payload_json.as_object_mut().map(|p| p.remove("_reactivation_reclose"));
    }
    let Some(ids) = previous
        .get("_reactivation_reserved_outputs")
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    if crate::core::production_map::reactivation::reclose_control(session).is_some() {
        return Ok(());
    }
    let changed_input = field(&previous, "input_progress_batch_id")
        != field(&session.payload_json, "input_progress_batch_id");
    let changed_cycle =
        previous.get("rezka_output_cycle") != session.payload_json.get("rezka_output_cycle");
    if session.status == OrderRunStatus::Active
        && !changed_input
        && !changed_cycle
        && session
            .payload_json
            .get("lamination_astatka_shift_closed")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Ok(());
    }
    for id in ids.iter().filter_map(Value::as_str) {
        sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut batch = order_query_helpers::load_progress_batch(&mut **tx, id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if batch.wip_status != OrderProgressBatchWipStatus::InUse
            || batch.used_by_session_id != session.session_id
            || batch.used_by_apparatus != session.apparatus
            || field(&batch.payload_json, "_reactivation_reservation") != session.session_id
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        batch.wip_status = OrderProgressBatchWipStatus::Waiting;
        batch.used_by_session_id.clear();
        batch.used_by_apparatus.clear();
        batch.current_apparatus = batch.apparatus.clone();
        batch
            .payload_json
            .as_object_mut()
            .map(|p| p.remove("_reactivation_reservation"));
        put_order_progress_batch_tx(tx, &batch).await?;
    }
    session
        .payload_json
        .as_object_mut()
        .map(|p| p.remove("_reactivation_reserved_outputs"));
    Ok(())
}
