use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    OpeningWipBatch, OpeningWipBatchRecord, OpeningWipBatchStatus, OpeningWipCreateWrite,
    OpeningWipDeleteWrite, OpeningWipIntake, OpeningWipIntakeStatus, OpeningWipQuantityBasis,
    OpeningWipQuery, OpeningWipRecord, ProductionMapError, QueueActionActor,
};

#[derive(sqlx::FromRow)]
struct OpeningWipIntakeRow {
    intake_id: String,
    idempotency_key: String,
    request_fingerprint: String,
    order_id: String,
    entry_apparatus: String,
    source_operation: String,
    source_apparatus: String,
    current_location: String,
    resume_apparatus: Option<String>,
    resume_stage_node_id: String,
    history_status: String,
    status: String,
    note: String,
    actor_role: String,
    actor_ref: String,
    actor_display_name: String,
    created_at_unix: i64,
    updated_at_unix: i64,
}

#[derive(sqlx::FromRow)]
struct OpeningWipBatchRow {
    batch_id: String,
    intake_id: String,
    order_id: String,
    sequence_no: i32,
    qr_payload: String,
    quantity: Option<f64>,
    uom: String,
    finished_goods_meter: Option<f64>,
    finished_goods_kg: Option<f64>,
    bobina_kg: Option<f64>,
    diameter: Option<f64>,
    quantity_basis: String,
    wip_status: String,
    used_by_session_id: String,
    used_by_apparatus: String,
    processed_by_session_id: String,
    processed_by_apparatus: String,
    label_item_code: String,
    label_item_name: String,
    created_at_unix: i64,
    updated_at_unix: i64,
}

pub(super) async fn load_opening_wip_by_idempotency_key(
    pool: &PgPool,
    idempotency_key: &str,
) -> Result<Option<OpeningWipRecord>, ProductionMapError> {
    let row = sqlx::query_as::<_, OpeningWipIntakeRow>(OPENING_WIP_INTAKE_SELECT)
        .bind(idempotency_key.trim())
        .fetch_optional(pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(row) = row else {
        return Ok(None);
    };
    load_opening_wip_record(pool, row).await.map(Some)
}

pub(super) async fn load_opening_wip_records(
    pool: &PgPool,
    query: OpeningWipQuery,
) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
    let status = query.wip_status.map(|status| status.as_str().to_string());
    let rows = sqlx::query_as::<_, OpeningWipIntakeRow>(
        "SELECT intake.intake_id, intake.idempotency_key, intake.request_fingerprint,
                intake.order_id, intake.entry_apparatus, intake.source_operation,
                intake.source_apparatus, intake.current_location, intake.resume_apparatus,
                intake.resume_stage_node_id, intake.history_status, intake.status,
                intake.note, intake.actor_role, intake.actor_ref,
                intake.actor_display_name,
                EXTRACT(EPOCH FROM intake.created_at)::BIGINT AS created_at_unix,
                EXTRACT(EPOCH FROM intake.updated_at)::BIGINT AS updated_at_unix
         FROM mini_opening_wip_intakes AS intake
         WHERE ($1 = '' OR intake.order_id = $1)
           AND ($2::TEXT IS NULL OR EXISTS (
               SELECT 1 FROM mini_opening_wip_batches AS batch
               WHERE batch.intake_id = intake.intake_id AND batch.wip_status = $2
           ))
         ORDER BY intake.created_at DESC, intake.intake_id DESC
         LIMIT $3",
    )
    .bind(query.order_id.trim())
    .bind(status)
    .bind(i64::try_from(query.limit.max(1)).unwrap_or(i64::MAX))
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        records.push(load_opening_wip_record(pool, row).await?);
    }
    Ok(records)
}

pub(super) async fn load_opening_wip_batch(
    pool: &PgPool,
    batch_id: &str,
    qr_payload: &str,
) -> Result<Option<OpeningWipBatchRecord>, ProductionMapError> {
    let batch = sqlx::query_as::<_, OpeningWipBatchRow>(
        "SELECT batch_id, intake_id, order_id, sequence_no, qr_payload,
                quantity::DOUBLE PRECISION AS quantity, uom,
                finished_goods_meter::DOUBLE PRECISION AS finished_goods_meter,
                finished_goods_kg::DOUBLE PRECISION AS finished_goods_kg,
                bobina_kg::DOUBLE PRECISION AS bobina_kg,
                diameter::DOUBLE PRECISION AS diameter,
                quantity_basis, wip_status,
                used_by_session_id, used_by_apparatus, processed_by_session_id,
                processed_by_apparatus, label_item_code, label_item_name,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
                EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
         FROM mini_opening_wip_batches
         WHERE ($1 <> '' AND batch_id = $1) OR ($2 <> '' AND qr_payload = $2)
         LIMIT 1",
    )
    .bind(batch_id.trim())
    .bind(qr_payload.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(batch) = batch else {
        return Ok(None);
    };
    let intake = sqlx::query_as::<_, OpeningWipIntakeRow>(
        "SELECT intake_id, idempotency_key, request_fingerprint, order_id, entry_apparatus,
                source_operation, source_apparatus, current_location, resume_apparatus,
                resume_stage_node_id, history_status, status, note, actor_role, actor_ref,
                actor_display_name,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
                EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
         FROM mini_opening_wip_intakes WHERE intake_id = $1",
    )
    .bind(batch.intake_id.trim())
    .fetch_one(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(Some(OpeningWipBatchRecord {
        intake: opening_wip_intake_from_row(intake)?,
        batch: opening_wip_batch_from_row(batch)?,
    }))
}

pub(super) async fn create_opening_wip(
    pool: &PgPool,
    write: OpeningWipCreateWrite,
) -> Result<OpeningWipRecord, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let intake = &write.record.intake;
    let inserted = sqlx::query(
        "INSERT INTO mini_opening_wip_intakes (
             intake_id, idempotency_key, request_fingerprint, order_id, entry_apparatus,
             source_operation, source_apparatus, current_location, resume_apparatus,
             resume_stage_node_id, history_status, status, note, actor_role, actor_ref,
             actor_display_name, created_at, updated_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
             $15, $16, to_timestamp($17), to_timestamp($18)
         )
         ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(intake.intake_id.trim())
    .bind(intake.idempotency_key.trim())
    .bind(intake.request_fingerprint.as_str())
    .bind(intake.order_id.trim())
    .bind(intake.entry_apparatus.trim())
    .bind(intake.source_operation.trim())
    .bind(intake.source_apparatus.trim())
    .bind(intake.current_location.trim())
    .bind((!intake.resume_apparatus.trim().is_empty()).then_some(intake.resume_apparatus.trim()))
    .bind(intake.resume_stage_node_id.trim())
    .bind(intake.history_status.trim())
    .bind(intake.status.as_str())
    .bind(intake.note.trim())
    .bind(intake.actor.role.trim())
    .bind(intake.actor.ref_.trim())
    .bind(intake.actor.display_name.trim())
    .bind(intake.created_at_unix)
    .bind(intake.updated_at_unix)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if inserted.rows_affected() == 0 {
        tx.rollback()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let existing =
            load_opening_wip_by_idempotency_key(pool, write.record.intake.idempotency_key.as_str())
                .await?
                .ok_or(ProductionMapError::StoreFailed)?;
        return if existing.intake.request_fingerprint == write.record.intake.request_fingerprint {
            Ok(existing)
        } else {
            Err(ProductionMapError::OpeningWipIdempotencyConflict)
        };
    }
    for batch in &write.record.batches {
        insert_opening_wip_batch_tx(&mut tx, batch).await?;
    }
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(write.record)
}

pub(super) async fn delete_opening_wip_batch(
    pool: &PgPool,
    write: OpeningWipDeleteWrite,
) -> Result<OpeningWipBatchRecord, ProductionMapError> {
    let batch_id = write.batch_id.trim();
    if batch_id.is_empty() {
        return Err(ProductionMapError::OpeningWipInvalidInput);
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let batch = sqlx::query_as::<_, OpeningWipBatchRow>(
        "SELECT batch_id, intake_id, order_id, sequence_no, qr_payload,
                quantity::DOUBLE PRECISION AS quantity, uom,
                finished_goods_meter::DOUBLE PRECISION AS finished_goods_meter,
                finished_goods_kg::DOUBLE PRECISION AS finished_goods_kg,
                bobina_kg::DOUBLE PRECISION AS bobina_kg,
                diameter::DOUBLE PRECISION AS diameter,
                quantity_basis, wip_status,
                used_by_session_id, used_by_apparatus, processed_by_session_id,
                processed_by_apparatus, label_item_code, label_item_name,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
                EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
         FROM mini_opening_wip_batches
         WHERE batch_id = $1
         FOR UPDATE",
    )
    .bind(batch_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
    let intake = sqlx::query_as::<_, OpeningWipIntakeRow>(
        "SELECT intake_id, idempotency_key, request_fingerprint, order_id, entry_apparatus,
                source_operation, source_apparatus, current_location, resume_apparatus,
                resume_stage_node_id, history_status, status, note, actor_role, actor_ref,
                actor_display_name,
                EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
                EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
         FROM mini_opening_wip_intakes
         WHERE intake_id = $1
         FOR UPDATE",
    )
    .bind(batch.intake_id.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    let parsed_batch = opening_wip_batch_from_row(batch)?;
    let parsed_intake = opening_wip_intake_from_row(intake)?;
    if parsed_batch.wip_status == OpeningWipBatchStatus::Void {
        tx.rollback()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        return Ok(OpeningWipBatchRecord {
            intake: parsed_intake,
            batch: parsed_batch,
        });
    }
    if parsed_intake.status != OpeningWipIntakeStatus::Confirmed
        || parsed_batch.wip_status != OpeningWipBatchStatus::Waiting
        || !parsed_batch.used_by_session_id.trim().is_empty()
        || !parsed_batch.used_by_apparatus.trim().is_empty()
        || !parsed_batch.processed_by_session_id.trim().is_empty()
        || !parsed_batch.processed_by_apparatus.trim().is_empty()
    {
        return Err(ProductionMapError::OpeningWipDeleteLocked);
    }

    let has_lineage = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM mini_order_run_sessions
             WHERE payload_json ->> 'input_wip_source_kind' = 'opening_wip'
               AND payload_json ->> 'input_progress_batch_id' = $1
             UNION ALL
             SELECT 1
             FROM mini_order_progress_events
             WHERE payload_json ->> 'input_wip_source_kind' = 'opening_wip'
               AND payload_json ->> 'input_progress_batch_id' = $1
             UNION ALL
             SELECT 1
             FROM mini_progress_batches
             WHERE parent_batch_id = $1
         )",
    )
    .bind(batch_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if has_lineage {
        return Err(ProductionMapError::OpeningWipDeleteLocked);
    }

    let updated = sqlx::query(
        "UPDATE mini_opening_wip_batches
         SET wip_status = 'void',
             voided_at = to_timestamp($2),
             voided_by_role = $3,
             voided_by_ref = $4,
             voided_by_display_name = $5,
             updated_at = to_timestamp($2)
         WHERE batch_id = $1
           AND wip_status = 'waiting'
           AND btrim(used_by_session_id) = ''
           AND btrim(used_by_apparatus) = ''
           AND btrim(processed_by_session_id) = ''
           AND btrim(processed_by_apparatus) = ''",
    )
    .bind(batch_id)
    .bind(write.deleted_at_unix as f64)
    .bind(write.actor.role.trim())
    .bind(write.actor.ref_.trim())
    .bind(write.actor.display_name.trim())
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if updated.rows_affected() != 1 {
        return Err(ProductionMapError::OpeningWipDeleteLocked);
    }
    sqlx::query(
        "UPDATE mini_opening_wip_intakes AS intake
         SET status = 'cancelled',
             updated_at = to_timestamp($2)
         WHERE intake.intake_id = $1
           AND NOT EXISTS (
               SELECT 1
               FROM mini_opening_wip_batches AS batch
               WHERE batch.intake_id = intake.intake_id
                 AND batch.wip_status <> 'void'
           )",
    )
    .bind(parsed_intake.intake_id.trim())
    .bind(write.deleted_at_unix as f64)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    load_opening_wip_batch(pool, batch_id, "")
        .await?
        .ok_or(ProductionMapError::ProgressBatchNotFound)
}

async fn insert_opening_wip_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OpeningWipBatch,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_opening_wip_batches (
             batch_id, intake_id, order_id, sequence_no, qr_payload, quantity, uom,
             finished_goods_meter, finished_goods_kg, bobina_kg, diameter,
             quantity_basis, wip_status, used_by_session_id, used_by_apparatus,
             processed_by_session_id, processed_by_apparatus, label_item_code,
             label_item_name, created_at, updated_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
             $15, $16, $17, $18, $19, to_timestamp($20), to_timestamp($21)
         )",
    )
    .bind(batch.batch_id.trim())
    .bind(batch.intake_id.trim())
    .bind(batch.order_id.trim())
    .bind(batch.sequence_no)
    .bind(batch.qr_payload.trim())
    .bind(batch.quantity)
    .bind(batch.uom.trim())
    .bind(batch.finished_goods_meter)
    .bind(batch.finished_goods_kg)
    .bind(batch.bobina_kg)
    .bind(batch.diameter)
    .bind(batch.quantity_basis.as_str())
    .bind(batch.wip_status.as_str())
    .bind(batch.used_by_session_id.trim())
    .bind(batch.used_by_apparatus.trim())
    .bind(batch.processed_by_session_id.trim())
    .bind(batch.processed_by_apparatus.trim())
    .bind(batch.label_item_code.trim())
    .bind(batch.label_item_name.trim())
    .bind(batch.created_at_unix)
    .bind(batch.updated_at_unix)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn update_opening_wip_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OpeningWipBatch,
) -> Result<(), ProductionMapError> {
    let expected_status = match batch.wip_status {
        OpeningWipBatchStatus::InUse => OpeningWipBatchStatus::Waiting,
        OpeningWipBatchStatus::Processed | OpeningWipBatchStatus::Waiting => {
            OpeningWipBatchStatus::InUse
        }
        OpeningWipBatchStatus::Void => return Err(ProductionMapError::StoreFailed),
    };
    let updated = sqlx::query(
        "UPDATE mini_opening_wip_batches
         SET wip_status = $3,
             used_by_session_id = $4,
             used_by_apparatus = $5,
             processed_by_session_id = $6,
             processed_by_apparatus = $7,
             updated_at = to_timestamp($8)
         WHERE batch_id = $1
           AND order_id = $2
           AND wip_status = $9
           AND (
               $9 <> 'in_use'
               OR used_by_session_id = $4
               OR $3 = 'waiting'
           )",
    )
    .bind(batch.batch_id.trim())
    .bind(batch.order_id.trim())
    .bind(batch.wip_status.as_str())
    .bind(batch.used_by_session_id.trim())
    .bind(batch.used_by_apparatus.trim())
    .bind(batch.processed_by_session_id.trim())
    .bind(batch.processed_by_apparatus.trim())
    .bind(batch.updated_at_unix)
    .bind(expected_status.as_str())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if updated.rows_affected() != 1 {
        return Err(ProductionMapError::ProgressBatchNotAccepted);
    }
    Ok(())
}

async fn load_opening_wip_record(
    pool: &PgPool,
    intake_row: OpeningWipIntakeRow,
) -> Result<OpeningWipRecord, ProductionMapError> {
    let batches = sqlx::query_as::<_, OpeningWipBatchRow>(OPENING_WIP_BATCH_SELECT)
        .bind(intake_row.intake_id.trim())
        .fetch_all(pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .into_iter()
        .map(opening_wip_batch_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OpeningWipRecord {
        intake: opening_wip_intake_from_row(intake_row)?,
        batches,
    })
}

fn opening_wip_intake_from_row(
    row: OpeningWipIntakeRow,
) -> Result<OpeningWipIntake, ProductionMapError> {
    Ok(OpeningWipIntake {
        intake_id: row.intake_id,
        idempotency_key: row.idempotency_key,
        request_fingerprint: row.request_fingerprint,
        order_id: row.order_id,
        entry_apparatus: row.entry_apparatus,
        source_operation: row.source_operation,
        source_apparatus: row.source_apparatus,
        current_location: row.current_location,
        resume_apparatus: row.resume_apparatus.unwrap_or_default(),
        resume_stage_node_id: row.resume_stage_node_id,
        history_status: row.history_status,
        status: OpeningWipIntakeStatus::parse(&row.status)
            .ok_or(ProductionMapError::StoreFailed)?,
        note: row.note,
        actor: QueueActionActor {
            role: row.actor_role,
            ref_: row.actor_ref,
            display_name: row.actor_display_name,
        },
        created_at_unix: row.created_at_unix,
        updated_at_unix: row.updated_at_unix,
    })
}

fn opening_wip_batch_from_row(
    row: OpeningWipBatchRow,
) -> Result<OpeningWipBatch, ProductionMapError> {
    Ok(OpeningWipBatch {
        batch_id: row.batch_id,
        intake_id: row.intake_id,
        order_id: row.order_id,
        sequence_no: row.sequence_no,
        qr_payload: row.qr_payload,
        quantity_basis: OpeningWipQuantityBasis::parse(&row.quantity_basis)
            .ok_or(ProductionMapError::StoreFailed)?,
        quantity: row.quantity,
        uom: row.uom,
        finished_goods_meter: row.finished_goods_meter,
        finished_goods_kg: row.finished_goods_kg,
        bobina_kg: row.bobina_kg,
        diameter: row.diameter,
        wip_status: OpeningWipBatchStatus::parse(&row.wip_status)
            .ok_or(ProductionMapError::StoreFailed)?,
        used_by_session_id: row.used_by_session_id,
        used_by_apparatus: row.used_by_apparatus,
        processed_by_session_id: row.processed_by_session_id,
        processed_by_apparatus: row.processed_by_apparatus,
        label_item_code: row.label_item_code,
        label_item_name: row.label_item_name,
        created_at_unix: row.created_at_unix,
        updated_at_unix: row.updated_at_unix,
    })
}

const OPENING_WIP_INTAKE_SELECT: &str =
    "SELECT intake_id, idempotency_key, request_fingerprint, order_id, entry_apparatus,
            source_operation, source_apparatus, current_location, resume_apparatus,
            resume_stage_node_id, history_status, status, note, actor_role, actor_ref,
            actor_display_name,
            EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
            EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
     FROM mini_opening_wip_intakes
     WHERE idempotency_key = $1";

const OPENING_WIP_BATCH_SELECT: &str =
    "SELECT batch_id, intake_id, order_id, sequence_no, qr_payload,
            quantity::DOUBLE PRECISION AS quantity, uom,
            finished_goods_meter::DOUBLE PRECISION AS finished_goods_meter,
            finished_goods_kg::DOUBLE PRECISION AS finished_goods_kg,
            bobina_kg::DOUBLE PRECISION AS bobina_kg,
            diameter::DOUBLE PRECISION AS diameter,
            quantity_basis, wip_status,
            used_by_session_id, used_by_apparatus, processed_by_session_id,
            processed_by_apparatus, label_item_code, label_item_name,
            EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
            EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
     FROM mini_opening_wip_batches
     WHERE intake_id = $1
     ORDER BY sequence_no ASC";
