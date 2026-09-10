use super::progress_helpers::{ProgressSessionRow, progress_session_from_row};
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
    let rows = sqlx::query_as::<_, (String, String, String, String, bool)>(
        "SELECT COALESCE(payload_json->>'stage_node_id', ''), COALESCE(payload_json->>'next_stage_node_id', ''),
         canonical_apparatus_id, COALESCE(canonical_next_apparatus_id, ''), wip_status = 'waiting' FROM mini_progress_batches
         WHERE order_id = $1 AND wip_status IN ('waiting', 'in_use')
         UNION ALL
         SELECT CASE WHEN i.source_apparatus <> '' THEN i.resume_stage_node_id ELSE '' END,
                CASE WHEN i.source_apparatus = '' THEN i.resume_stage_node_id ELSE '' END,
                i.source_apparatus, COALESCE(i.resume_apparatus, ''), b.wip_status = 'waiting'
         FROM mini_opening_wip_intakes i JOIN mini_opening_wip_batches b ON b.intake_id = i.intake_id
         WHERE i.order_id = $1 AND i.status = 'confirmed' AND b.wip_status IN ('waiting', 'in_use')"
    ).bind(order_id).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(
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
        )
        .collect())
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
    }
    Ok(())
}
