use super::progress_helpers::{ProgressSessionRow, progress_session_from_row};
use crate::core::production_map::chain::stage_node_ids_match_for_map;
use crate::core::production_map::wip_route::{
    WipInputRoute, actionable_wip, normalized_route_batch, production_map_fingerprint,
    resolve_wip_input_route,
};
use crate::core::production_map::{stage_execution::*, *};
use sqlx::{PgPool, Postgres, Transaction};

pub(super) async fn sessions_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_id: &str,
) -> Result<Vec<OrderRunSession>, ProductionMapError> {
    sqlx::query_as::<_, ProgressSessionRow>(
        "SELECT session_id, canonical_apparatus_id AS apparatus, order_id, stage_node_id, status,
         worker_role, worker_ref, worker_display_name,
         extract(epoch FROM started_at)::bigint AS started_at_unix,
         extract(epoch FROM updated_at)::bigint AS updated_at_unix, payload_json
         FROM mini_order_run_sessions WHERE order_id = $1 ORDER BY started_at, session_id",
    )
    .bind(order_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .into_iter()
    .map(progress_session_from_row)
    .collect()
}

pub(super) async fn inputs_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_id: &str,
) -> Result<Vec<StageWorkInput>, ProductionMapError> {
    let map = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json FROM mini_production_maps WHERE id = $1",
    )
    .bind(order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::StoreFailed)?;
    let map: ProductionMapDefinition =
        serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    let routes = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT route_json FROM mini_progress_batch_work_inputs
         WHERE order_id = $1 AND batch_count > 0
         UNION
         SELECT mini_progress_batch_work_route(b)
         FROM mini_layer_remainder_operations r
         JOIN mini_progress_batches b
           ON b.batch_id = r.response_json #>> '{progress_batch,batch_id}'
         WHERE r.order_id = $1 AND r.source_kind = 'progress_batch'
           AND b.order_id = $1 AND b.wip_status IN ('waiting', 'in_use')",
    )
    .bind(order_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    // One representative per exact routing class is enough: stage closure
    // depends on existence/availability, never the number or weight of rolls.
    // Keep the same effective-route resolver, including edited maps and pins.
    let batches = routes.into_iter().map(|route| route_projection_batch(order_id, route))
        .collect::<Result<Vec<_>, _>>()?;
    let mut inputs = work_inputs(&map, &batches, &[]);
    // Opening WIP has its own authoritative intake route. Preserve its existing
    // projection while standard produced inputs share the effective-route
    // resolver with scan, preview, Start and the in-memory lifecycle.
    let rows = sqlx::query_as::<_, (String, String, String, String, bool)>(
        "SELECT DISTINCT CASE WHEN i.source_apparatus <> '' THEN i.resume_stage_node_id ELSE '' END,
                CASE WHEN i.source_apparatus = '' THEN i.resume_stage_node_id ELSE '' END,
                i.source_apparatus, COALESCE(i.resume_apparatus, ''), b.wip_status = 'waiting'
         FROM mini_opening_wip_intakes i JOIN mini_opening_wip_batches b ON b.intake_id = i.intake_id
         WHERE i.order_id = $1 AND i.status = 'confirmed' AND b.wip_status IN ('waiting', 'in_use')"
    ).bind(order_id).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    inputs.extend(rows.into_iter().map(
        |(source_node, target_node, source_apparatus, target_apparatus, available)| {
            StageWorkInput {
                source_node,
                target_node,
                source_apparatus,
                target_apparatus,
                outstanding: true,
                available,
            }
        },
    ));
    Ok(inputs)
}

fn route_projection_batch(
    order_id: &str,
    route: serde_json::Value,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let mut batch = serde_json::json!({
        "batch_id": "", "session_id": "", "order_id": order_id,
        "started_at_unix": 0, "completed_at_unix": 0,
        "status": "completed", "produced_qty": 0.0, "uom": "",
        "qr_payload": "", "label_item_code": "", "label_item_name": "",
        "executor_name": "", "worker_role": "", "worker_ref": "", "worker_display_name": ""
    });
    let fields = route.as_object().ok_or(ProductionMapError::StoreFailed)?;
    batch.as_object_mut().expect("object").extend(fields.clone());
    // Match the existing SQL row parser's legacy case/whitespace tolerance.
    let action = batch.get("action").and_then(serde_json::Value::as_str)
        .and_then(queue_state::ApparatusQueueAction::parse).ok_or(ProductionMapError::StoreFailed)?;
    batch["action"] = serde_json::json!(action.as_str());
    serde_json::from_value(batch).map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn stamp_report_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &mut OrderRunSession,
    report_id: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let sessions = sessions_tx(tx, &session.order_id).await?;
    let sequence = sessions
        .iter()
        .filter_map(work_report)
        .map(|r| r.sequence)
        .max()
        .unwrap_or(0)
        + 1;
    stamp_work_report(
        session,
        report_id,
        sequence,
        actor,
        crate::core::production_map::stage_execution::report_now(),
    );
    Ok(())
}

pub(super) async fn commit_report(
    pool: &PgPool,
    report: StageAstatkaReport,
    expected: Option<OrderRunSession>,
    actor: QueueActionActor,
) -> Result<(), ProductionMapError> {
    let (order_id, apparatus, _) = report.identity();
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    super::transaction_locks::lock_order_and_apparatuses_tx(&mut tx, order_id, &[apparatus])
        .await?;
    commit_report_tx(&mut tx, &report, expected, &actor).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

async fn commit_report_tx(
    tx: &mut Transaction<'_, Postgres>,
    report: &StageAstatkaReport,
    expected: Option<OrderRunSession>,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let (order_id, apparatus, report_id) = report.identity();
    let control = sqlx::query_scalar::<_, String>(
        "SELECT state FROM mini_order_control_states WHERE order_id = $1 FOR UPDATE",
    )
    .bind(order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if control.as_deref() == Some("frozen") {
        return Err(ProductionMapError::OrderFrozen);
    }
    let sessions = sessions_tx(tx, order_id).await?;
    let current = sessions
        .iter()
        .filter(|s| s.apparatus == apparatus)
        .max_by(|a, b| (a.started_at_unix, &a.session_id).cmp(&(b.started_at_unix, &b.session_id)));
    if current != expected.as_ref() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    match report {
        StageAstatkaReport::Laminate(r) => {
            super::astatka_helpers::put_laminatsiya_astatka_report_tx(tx, r).await?
        }
        StageAstatkaReport::Cut(r) => {
            super::astatka_helpers::put_rezka_astatka_report_tx(tx, r).await?
        }
        StageAstatkaReport::Bosma(r) => {
            sqlx::query("INSERT INTO mini_bosma_astatka_reports (report_id, order_id, apparatus, from_at_unix, to_at_unix, report_json)
                VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(&r.report_id).bind(&r.order_id).bind(&r.apparatus).bind(r.from_at_unix).bind(r.to_at_unix)
                .bind(serde_json::to_value(r).map_err(|_| ProductionMapError::StoreFailed)?)
                .execute(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        }
    }
    super::waste::record_astatka(
        tx,
        report,
        expected.as_ref().map(|session| session.session_id.as_str()),
        actor,
    ).await?;
    // Legacy report clients may still submit output metrics in an astatka.
    // Report-only waste/paint is allowed during a reopened pause; actual output
    // irrevocably removes the no-work reclose permission.
    let reports_output = match report {
        StageAstatkaReport::Bosma(r) => r.finished_goods_meter.is_some() || r.finished_goods_kg.is_some() || r.bobina_kg.is_some(),
        StageAstatkaReport::Laminate(r) => r.finished_goods_meter.is_some() || r.finished_goods_kg.is_some() || r.bobina_kg.is_some(),
        StageAstatkaReport::Cut(r) => r.finished_goods_meter.is_some() || r.finished_goods_kg.is_some() || r.bobina_kg.is_some(),
    };
    if reports_output && let Some(mut session) = expected.clone().filter(|s|
        crate::core::production_map::reactivation::reclose_control(s).is_some()) {
        session.payload_json["_reactivation_started"] = serde_json::json!(true);
        session.payload_json.as_object_mut().map(|p| p.remove("_reactivation_reclose"));
        super::progress_helpers::put_order_run_session_tx(tx, &session).await?;
    }
    if let Some(mut session) = expected.filter(|s| s.status == OrderRunStatus::Completed) {
        stamp_report_tx(tx, &mut session, report_id, actor).await?;
        super::progress_helpers::put_order_run_session_tx(tx, &session).await?;
        super::lifecycle::refresh_production_order_lifecycle_tx(
            tx,
            order_id,
            actor,
            report_id,
            "stage_astatka",
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn rezka_receipt(
    pool: &PgPool,
    actor: &QueueActionActor,
    request_id: &str,
) -> Result<Option<(serde_json::Value, RezkaAstatkaReport)>, ProductionMapError> {
    let row: Option<(serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT request_json, report_json FROM mini_rezka_astatka_operations
         WHERE actor_role=$1 AND actor_ref=$2 AND request_id=$3",
    )
    .bind(&actor.role)
    .bind(&actor.ref_)
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(|(request, report)| {
        serde_json::from_value(report)
            .map(|report| (request, report))
            .map_err(|_| ProductionMapError::StoreFailed)
    })
    .transpose()
}

pub(super) async fn commit_rezka_report(
    pool: &PgPool,
    request_id: String,
    request_json: serde_json::Value,
    report: RezkaAstatkaReport,
    expected: OrderRunSession,
    actor: QueueActionActor,
) -> Result<RezkaAstatkaReport, ProductionMapError> {
    if !(8..=128).contains(&request_id.len())
        || actor.ref_.trim().is_empty()
        || expected.session_id.is_empty()
        || expected.order_id != report.order_id
        || expected.apparatus != report.apparatus
        || request_json
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            != Some(expected.session_id.as_str())
    {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    super::transaction_locks::lock_order_and_apparatuses_tx(
        &mut tx,
        &report.order_id,
        &[&report.apparatus],
    )
    .await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "rezka-astatka:{}:{}:{}",
            actor.role, actor.ref_, request_id
        ))
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let replay: Option<(serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT request_json, report_json FROM mini_rezka_astatka_operations
         WHERE actor_role=$1 AND actor_ref=$2 AND request_id=$3",
    )
    .bind(&actor.role)
    .bind(&actor.ref_)
    .bind(&request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if let Some((previous, report)) = replay {
        if previous != request_json {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        return serde_json::from_value(report).map_err(|_| ProductionMapError::StoreFailed);
    }
    let session_id = expected.session_id.clone();
    commit_report_tx(
        &mut tx,
        &StageAstatkaReport::Cut(report.clone()),
        Some(expected),
        &actor,
    )
    .await?;
    sqlx::query("INSERT INTO mini_rezka_astatka_operations
        (actor_role, actor_ref, request_id, order_id, apparatus, session_id, report_id, request_json, report_json)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(&actor.role).bind(&actor.ref_).bind(request_id).bind(&report.order_id)
        .bind(&report.apparatus).bind(session_id).bind(&report.report_id).bind(request_json)
        .bind(serde_json::to_value(&report).map_err(|_| ProductionMapError::StoreFailed)?)
        .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(report)
}

pub(super) async fn validate_input_claims_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<(), ProductionMapError> {
    if write.event.action == queue_state::ApparatusQueueAction::Resume
        && let Some(session) = &write.session
    {
        let raw_released = sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(payload_json->>'lamination_astatka_raw_released', '') = 'true'
             FROM mini_order_run_sessions
             WHERE session_id = $1 AND order_id = $2 AND canonical_apparatus_id = $3
             FOR UPDATE",
        ).bind(&session.session_id).bind(&write.event.order_id).bind(&write.apparatus)
            .fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if raw_released == Some(true)
            && (write.event.payload_json.get("lamination_raw_reacquisition")
                .and_then(serde_json::Value::as_bool) != Some(true)
                || !write.raw_material_stock_transitions.iter().any(|transition| {
                    transition.kind == RawMaterialStockTransitionKind::InUse
                        && transition.order_id == write.event.order_id
                        && !transition.barcodes.is_empty()
                }))
        {
            return Err(ProductionMapError::RawMaterialScanRequired);
        }
    }
    for batch in &write.progress_batch_updates {
        if batch.wip_status != OrderProgressBatchWipStatus::InUse {
            continue;
        }
        let current = sqlx::query_as::<_, (String, String, bool, String, String)>(
            "SELECT order_id, wip_status,
             COALESCE((wip_status = 'processed' AND action IN ('pause', 'detach_roll')
              AND canonical_processed_by_apparatus_id = canonical_apparatus_id
              AND (COALESCE(canonical_used_by_apparatus_id, '') = '' OR canonical_used_by_apparatus_id = canonical_apparatus_id)
              AND (COALESCE(processed_by_session_id, '') = '' OR processed_by_session_id = session_id)), false),
             COALESCE(canonical_used_by_apparatus_id, ''), COALESCE(used_by_session_id, '')
             FROM mini_progress_batches WHERE batch_id = $1 FOR UPDATE")
            .bind(&batch.batch_id).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if current
            .as_ref()
            .is_none_or(|(order, state, recovered, owner, session)| {
                order != &write.event.order_id
                    || (state != "waiting"
                        && !recovered
                        && !(state == "in_use"
                            && owner == &batch.used_by_apparatus
                            && session == &batch.used_by_session_id))
            })
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let locked_batch =
            super::order_query_helpers::load_progress_batch(&mut **tx, &batch.batch_id)
                .await?
                .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        validate_wip_route_claim_tx(tx, write, &locked_batch, batch).await?;
    }
    Ok(())
}

include!("stage_execution_parts/validate_wip_route_claim_tx.rs");

#[cfg(test)]
#[path = "wip_route_tests.rs"]
mod wip_route_tests;
