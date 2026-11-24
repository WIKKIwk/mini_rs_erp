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

pub(super) async fn validate_opening_remainder_resume_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &crate::core::production_map::QueueActionProgressWrite,
    proposed: &OpeningWipBatch,
) -> Result<(), ProductionMapError> {
    let Some(expected) = write
        .event
        .payload_json
        .get("opening_wip_input_expected_record")
    else {
        return Ok(());
    };
    let session = write
        .session
        .as_ref()
        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
    if write.event.action != crate::core::production_map::queue_state::ApparatusQueueAction::Resume
        || proposed.wip_status != OpeningWipBatchStatus::InUse
        || proposed.used_by_session_id != session.session_id
        || proposed.used_by_apparatus != write.apparatus
        || proposed.order_id != write.event.order_id
    {
        return Err(ProductionMapError::ProgressBatchNotResumable);
    }
    let released: Option<bool> = sqlx::query_scalar(
        "SELECT status IN ('paused','roll_detached')
            AND COALESCE(payload_json->>'input_progress_batch_id','')=''
            AND (COALESCE(payload_json->>'lamination_astatka_shift_closed','')='true'
                OR ($4<>'' AND COALESCE(payload_json->>'layer_remainder_detached_input_batch_id','')=$4))
         FROM mini_order_run_sessions WHERE session_id=$1 AND order_id=$2
            AND canonical_apparatus_id=$3 AND stage_node_id=$5 FOR UPDATE",
    )
    .bind(&session.session_id)
    .bind(&write.event.order_id)
    .bind(&write.apparatus)
    .bind(&proposed.parent_batch_id)
    .bind(&session.stage_node_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let (record, _) = load_opening_wip_batch_tx(tx, &proposed.batch_id)
        .await?
        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
    if released != Some(true)
        || serde_json::json!(record) != *expected
        || record.intake.status != OpeningWipIntakeStatus::Confirmed
    {
        return Err(ProductionMapError::ProgressBatchNotResumable);
    }
    let map: serde_json::Value =
        sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
            .bind(&write.event.order_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    let map: crate::core::production_map::ProductionMapDefinition =
        serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    if write.event.payload_json["wip_input_map_fingerprint"]
        != serde_json::json!(
            crate::core::production_map::wip_route::production_map_fingerprint(&map)
        )
        || crate::core::production_map::ProductionMapService::opening_wip_target_stage(
            &map,
            &record.intake,
            &write.apparatus,
            &session.stage_node_id,
        )
        .is_none()
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    Ok(())
}

async fn load_opening_wip_record(
    pool: &PgPool,
    intake_row: OpeningWipIntakeRow,
) -> Result<OpeningWipRecord, ProductionMapError> {
    let batch_query = format!("{OPENING_WIP_BATCH_SELECT_PREFIX}
        WHERE intake_id = $1 ORDER BY sequence_no ASC");
    let batches = sqlx::query_as::<_, OpeningWipBatchRow>(&batch_query)
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
        parent_batch_id: row.parent_batch_id.unwrap_or_default(),
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

const OPENING_WIP_BATCH_SELECT_PREFIX: &str =
    "SELECT batch_id, parent_batch_id, (EXTRACT(EPOCH FROM updated_at)*1000000)::BIGINT AS revision, intake_id, order_id, sequence_no, qr_payload,
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
     FROM mini_opening_wip_batches";
