#[derive(sqlx::FromRow)]
pub(super) struct ProgressSessionRow {
    pub(super) session_id: String,
    pub(super) apparatus: String,
    pub(super) order_id: String,
    pub(super) status: String,
    pub(super) worker_role: String,
    pub(super) worker_ref: String,
    pub(super) worker_display_name: String,
    pub(super) started_at_unix: i64,
    pub(super) updated_at_unix: i64,
    pub(super) payload_json: serde_json::Value,
}

#[derive(sqlx::FromRow)]
pub(super) struct ProgressBatchRow {
    pub(super) batch_id: String,
    pub(super) revision: i64,
    pub(super) session_id: String,
    pub(super) started_at_unix: i64,
    pub(super) completed_at_unix: i64,
    pub(super) apparatus: String,
    pub(super) order_id: String,
    pub(super) action: String,
    pub(super) status: String,
    pub(super) produced_qty: f64,
    pub(super) uom: String,
    pub(super) qr_payload: String,
    pub(super) label_item_code: String,
    pub(super) label_item_name: String,
    pub(super) executor_name: String,
    pub(super) worker_role: String,
    pub(super) worker_ref: String,
    pub(super) worker_display_name: String,
    pub(super) wip_status: String,
    pub(super) current_apparatus: String,
    pub(super) current_apparatus_key: String,
    pub(super) current_location: String,
    pub(super) next_apparatus: String,
    pub(super) parent_batch_id: String,
    pub(super) used_by_session_id: String,
    pub(super) used_by_apparatus: String,
    pub(super) processed_by_session_id: String,
    pub(super) processed_by_apparatus: String,
    pub(super) return_ink_kg: Option<f64>,
    pub(super) lamination_print_leftover_rolls: Option<f64>,
    pub(super) lamination_film_leftover_rolls: Option<f64>,
    pub(super) rezka_bosma_waste: Option<f64>,
    pub(super) rezka_lamination_waste: Option<f64>,
    pub(super) rezka_edge_waste: Option<f64>,
    pub(super) total_waste: Option<f64>,
    pub(super) finished_goods_kg: Option<f64>,
    pub(super) bobina_kg: Option<f64>,
    pub(super) finished_goods_meter: Option<f64>,
    pub(super) diameter: Option<f64>,
    pub(super) description: String,
    pub(super) payload_json: serde_json::Value,
}

#[derive(sqlx::FromRow)]
pub(super) struct QueueActionLogRow {
    pub(super) event_id: String,
    pub(super) apparatus: String,
    pub(super) order_id: String,
    pub(super) action: String,
    pub(super) from_state: String,
    pub(super) to_state: String,
    pub(super) actor_role: String,
    pub(super) actor_ref: String,
    pub(super) actor_display_name: String,
    pub(super) created_at_unix: i64,
    pub(super) completed_with_issue: bool,
    pub(super) issue_note: String,
}

#[derive(sqlx::FromRow)]
pub(super) struct ProgressBatchCorrectionRow {
    pub(super) batch_id: String,
    pub(super) previous_revision: i64,
    pub(super) new_revision: i64,
    pub(super) reason: String,
    pub(super) actor_role: String,
    pub(super) actor_ref: String,
    pub(super) actor_display_name: String,
    pub(super) old_values: serde_json::Value,
    pub(super) new_values: serde_json::Value,
    pub(super) created_at_unix: i64,
}

pub(super) fn progress_batch_correction_from_row(
    row: ProgressBatchCorrectionRow,
) -> Result<ProgressBatchCorrectionRecord, ProductionMapError> {
    Ok(ProgressBatchCorrectionRecord {
        batch_id: row.batch_id,
        previous_revision: u64::try_from(row.previous_revision)
            .map_err(|_| ProductionMapError::StoreFailed)?,
        new_revision: u64::try_from(row.new_revision)
            .map_err(|_| ProductionMapError::StoreFailed)?,
        reason: row.reason,
        actor: QueueActionActor {
            role: row.actor_role,
            ref_: row.actor_ref,
            display_name: row.actor_display_name,
        },
        old_values: row.old_values,
        new_values: row.new_values,
        created_at_unix: row.created_at_unix,
    })
}

pub(super) fn queue_action_log_from_row(
    row: QueueActionLogRow,
) -> Result<ProductionOrderLogEntry, ProductionMapError> {
    Ok(ProductionOrderLogEntry {
        event_id: row.event_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        action: queue_action_from_str(&row.action).ok_or(ProductionMapError::StoreFailed)?,
        from_state: queue_state::ApparatusQueueOrderState::parse(&row.from_state)
            .ok_or(ProductionMapError::StoreFailed)?,
        to_state: queue_state::ApparatusQueueOrderState::parse(&row.to_state)
            .ok_or(ProductionMapError::StoreFailed)?,
        actor_role: row.actor_role,
        actor_ref: row.actor_ref,
        actor_display_name: row.actor_display_name,
        created_at_unix: row.created_at_unix,
        completed_with_issue: row.completed_with_issue,
        issue_note: row.issue_note,
        transfer: None,
        freeze: None,
    })
}

pub(super) fn progress_session_from_row(
    row: ProgressSessionRow,
) -> Result<OrderRunSession, ProductionMapError> {
    Ok(OrderRunSession {
        session_id: row.session_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        status: OrderRunStatus::parse(&row.status).ok_or(ProductionMapError::StoreFailed)?,
        worker_role: row.worker_role,
        worker_ref: row.worker_ref,
        worker_display_name: row.worker_display_name,
        started_at_unix: row.started_at_unix,
        updated_at_unix: row.updated_at_unix,
        payload_json: row.payload_json,
    })
}

pub(super) fn progress_batch_from_row(
    row: ProgressBatchRow,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let current_apparatus_key = if row.current_apparatus_key.trim().is_empty() {
        queue_state::apparatus_search_key(&row.current_apparatus)
    } else {
        row.current_apparatus_key
    };
    let mut batch = OrderProgressBatch {
        batch_id: row.batch_id,
        revision: u64::try_from(row.revision).map_err(|_| ProductionMapError::StoreFailed)?,
        session_id: row.session_id,
        started_at_unix: row.started_at_unix,
        completed_at_unix: row.completed_at_unix,
        apparatus: row.apparatus,
        order_id: row.order_id,
        action: queue_action_from_str(&row.action).ok_or(ProductionMapError::StoreFailed)?,
        status: OrderProgressBatchStatus::parse(&row.status)
            .ok_or(ProductionMapError::StoreFailed)?,
        produced_qty: row.produced_qty,
        uom: row.uom,
        qr_payload: row.qr_payload,
        label_item_code: row.label_item_code,
        label_item_name: row.label_item_name,
        executor_name: row.executor_name,
        worker_role: row.worker_role,
        worker_ref: row.worker_ref,
        worker_display_name: row.worker_display_name,
        wip_status: OrderProgressBatchWipStatus::parse(&row.wip_status)
            .ok_or(ProductionMapError::StoreFailed)?,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: row.current_apparatus,
        current_apparatus_key,
        current_location: row.current_location,
        next_apparatus: row.next_apparatus,
        parent_batch_id: row.parent_batch_id,
        used_by_session_id: row.used_by_session_id,
        used_by_apparatus: row.used_by_apparatus,
        processed_by_session_id: row.processed_by_session_id,
        processed_by_apparatus: row.processed_by_apparatus,
        return_ink_kg: row.return_ink_kg,
        lamination_print_leftover_rolls: row.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: row.lamination_film_leftover_rolls,
        rezka_bosma_waste: row.rezka_bosma_waste,
        rezka_lamination_waste: row.rezka_lamination_waste,
        rezka_edge_waste: row.rezka_edge_waste,
        total_waste: row.total_waste,
        finished_goods_kg: row.finished_goods_kg,
        bobina_kg: row.bobina_kg,
        finished_goods_meter: row.finished_goods_meter,
        diameter: row.diameter,
        description: row.description,
        payload_json: row.payload_json,
    };
    batch.refresh_status_detail();
    Ok(batch)
}
