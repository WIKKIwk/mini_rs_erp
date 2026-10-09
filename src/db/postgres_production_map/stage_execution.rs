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
         WHERE order_id = $1 AND batch_count > 0 ORDER BY route_key",
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
    let (order_id, apparatus, report_id) = report.identity();
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    super::transaction_locks::lock_order_and_apparatuses_tx(&mut tx, order_id, &[apparatus])
        .await?;
    let control = sqlx::query_scalar::<_, String>(
        "SELECT state FROM mini_order_control_states WHERE order_id = $1 FOR UPDATE",
    )
    .bind(order_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if control.as_deref() == Some("frozen") {
        return Err(ProductionMapError::OrderFrozen);
    }
    let sessions = sessions_tx(&mut tx, order_id).await?;
    let current = sessions
        .iter()
        .filter(|s| s.apparatus == apparatus)
        .max_by(|a, b| (a.started_at_unix, &a.session_id).cmp(&(b.started_at_unix, &b.session_id)));
    if current != expected.as_ref() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    match &report {
        StageAstatkaReport::Laminate(r) => {
            super::astatka_helpers::put_laminatsiya_astatka_report_tx(&mut tx, r).await?
        }
        StageAstatkaReport::Cut(r) => {
            super::astatka_helpers::put_rezka_astatka_report_tx(&mut tx, r).await?
        }
        StageAstatkaReport::Bosma(r) => {
            sqlx::query("INSERT INTO mini_bosma_astatka_reports (report_id, order_id, apparatus, from_at_unix, to_at_unix, report_json)
                VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(&r.report_id).bind(&r.order_id).bind(&r.apparatus).bind(r.from_at_unix).bind(r.to_at_unix)
                .bind(serde_json::to_value(r).map_err(|_| ProductionMapError::StoreFailed)?)
                .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        }
    }
    if let Some(mut session) = expected.filter(|s| s.status == OrderRunStatus::Completed) {
        stamp_report_tx(&mut tx, &mut session, report_id, &actor).await?;
        super::progress_helpers::put_order_run_session_tx(&mut tx, &session).await?;
        super::lifecycle::refresh_production_order_lifecycle_tx(
            &mut tx,
            order_id,
            &actor,
            report_id,
            "stage_astatka",
        )
        .await?;
    }
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn validate_input_claims_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<(), ProductionMapError> {
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

async fn validate_wip_route_claim_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
    current: &OrderProgressBatch,
    proposed: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    if current.wip_status == OrderProgressBatchWipStatus::InUse {
        // A resumed input belongs to its existing session and route. Do not
        // re-resolve it into a different operation after a graph edit.
        if let Some(binding) = current.payload_json.get("wip_route_binding") {
            if proposed.payload_json.get("wip_route_binding") != Some(binding) {
                return Err(ProductionMapError::WipRouteChanged);
            }
            let binding: WipInputRoute = serde_json::from_value(binding.clone())
                .map_err(|_| ProductionMapError::WipRouteChanged)?;
            validate_claim_session(write, proposed, &binding)?;
        }
        return Ok(());
    }
    // Producer resumes use the original producer session rather than the
    // next-stage route. Start/Merge may revisit the same physical apparatus
    // at a different graph occurrence, so canonical apparatus equality alone
    // must never bypass their map and source witness checks.
    if current.apparatus == write.apparatus
        && !matches!(
            write.event.action,
            queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Merge
        )
    {
        return Ok(());
    }
    if !matches!(
        write.event.action,
        queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Merge
    ) {
        // Established Resume/handoff/recovery paths keep their original pin.
        // They cannot introduce a new route or replace one on an already
        // mounted input. Legacy unpinned sessions retain the ownership gates.
        if proposed.payload_json.get("wip_route_binding")
            != current.payload_json.get("wip_route_binding")
        {
            return Err(ProductionMapError::WipRouteChanged);
        }
        if let Some(binding) = current.payload_json.get("wip_route_binding") {
            let binding: WipInputRoute = serde_json::from_value(binding.clone())
                .map_err(|_| ProductionMapError::WipRouteChanged)?;
            validate_claim_session(write, proposed, &binding)?;
            let session = write
                .session
                .as_ref()
                .ok_or(ProductionMapError::WipRouteChanged)?;
            let linked = sqlx::query_scalar::<_, bool>(
                "SELECT COALESCE(payload_json->>'input_progress_batch_id', '') = $4
                 FROM mini_order_run_sessions
                 WHERE session_id = $1 AND order_id = $2
                   AND canonical_apparatus_id = $3 AND stage_node_id = $5
                 FOR UPDATE",
            )
            .bind(&session.session_id)
            .bind(&write.event.order_id)
            .bind(&write.apparatus)
            .bind(&current.batch_id)
            .bind(&binding.stage_node_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
            if linked != Some(true) {
                return Err(ProductionMapError::WipRouteChanged);
            }
        }
        return Ok(());
    }
    // Every new downstream claim needs a preview witness, including a legacy
    // self-consumed output recovered in memory. Absence cannot bypass the
    // transaction's map and batch revalidation.
    let expected = write
        .event
        .payload_json
        .get("wip_input_expected_batch")
        .ok_or(ProductionMapError::WipRouteChanged)?;
    let expected: OrderProgressBatch = serde_json::from_value(expected.clone())
        .map_err(|_| ProductionMapError::WipRouteChanged)?;
    if current != &expected {
        return Err(ProductionMapError::WipRouteChanged);
    }
    let expected_fingerprint = write
        .event
        .payload_json
        .get("wip_input_map_fingerprint")
        .and_then(serde_json::Value::as_str)
        .ok_or(ProductionMapError::WipRouteChanged)?;
    let map = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json FROM mini_production_maps WHERE id = $1 FOR UPDATE",
    )
    .bind(&write.event.order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::WipRouteChanged)?;
    let map: ProductionMapDefinition =
        serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    if production_map_fingerprint(&map) != expected_fingerprint {
        return Err(ProductionMapError::WipRouteChanged);
    }
    let normalized = normalized_route_batch(current);
    if !actionable_wip(&normalized) {
        return Err(ProductionMapError::ProgressBatchNotAccepted);
    }
    let route = resolve_wip_input_route(&map, &normalized)?;
    let binding: WipInputRoute = serde_json::from_value(
        proposed
            .payload_json
            .get("wip_route_binding")
            .cloned()
            .ok_or(ProductionMapError::WipRouteChanged)?,
    )
    .map_err(|_| ProductionMapError::WipRouteChanged)?;
    if binding.source_stage_node_id != route.source_stage_node_id
        || binding.consumer_apparatus_ids != route.consumer_apparatus_ids
        || binding.map_fingerprint != route.map_fingerprint
        || binding.remapped != route.remapped
        || !stage_node_ids_match_for_map(&map, &binding.stage_node_id, &route.stage_node_id)
        || !claim_preserves_source(&normalized, proposed)
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    validate_claim_session(write, proposed, &binding)
}

fn claim_preserves_source(source: &OrderProgressBatch, proposed: &OrderProgressBatch) -> bool {
    let mut restored = proposed.clone();
    restored.wip_status = source.wip_status;
    restored.current_apparatus = source.current_apparatus.clone();
    restored.current_location = source.current_location.clone();
    restored.used_by_apparatus = source.used_by_apparatus.clone();
    restored.used_by_session_id = source.used_by_session_id.clone();
    restored.processed_by_apparatus = source.processed_by_apparatus.clone();
    restored.processed_by_session_id = source.processed_by_session_id.clone();
    for key in [
        "wip_route_binding",
        "wip_in_use_at_unix",
        "wip_processed_at_unix",
    ] {
        if let Some(value) = source.payload_json.get(key) {
            restored.payload_json[key] = value.clone();
        } else if let Some(payload) = restored.payload_json.as_object_mut() {
            payload.remove(key);
        }
    }
    restored.refresh_status_detail();
    restored == *source
}

fn validate_claim_session(
    write: &QueueActionProgressWrite,
    proposed: &OrderProgressBatch,
    binding: &WipInputRoute,
) -> Result<(), ProductionMapError> {
    let session = write
        .session
        .as_ref()
        .ok_or(ProductionMapError::WipRouteChanged)?;
    if !binding.consumer_apparatus_ids.contains(&write.apparatus)
        || session.apparatus != write.apparatus
        || session.order_id != write.event.order_id
        || session.stage_node_id != binding.stage_node_id
        || proposed.used_by_apparatus != write.apparatus
        || proposed.used_by_session_id != session.session_id
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    Ok(())
}

#[cfg(test)]
#[path = "wip_route_tests.rs"]
mod wip_route_tests;
