use super::*;

use super::progress::{
    actor_display_name, legacy_order_run_session, non_empty_or, progress_batch_id,
    progress_event_id, progress_label_item_name, progress_qr_payload, progress_session_id,
    queue_action_str, unix_seconds, valid_progress_qty,
};
use super::service::QueueProgressRecords;

impl ProductionMapService {
    pub async fn progress_batch_for_qr(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        let batch = if !progress_batch_id.trim().is_empty() {
            self.store.progress_batch(progress_batch_id).await?
        } else if !qr_payload.trim().is_empty() {
            self.store.progress_batch_by_qr(qr_payload).await?
        } else {
            return Err(ProductionMapError::ProgressInputInvalid);
        };
        batch.ok_or(ProductionMapError::ProgressBatchNotFound)
    }

    pub(super) async fn build_progress_records(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let now = unix_seconds();
        match action {
            queue_state::ApparatusQueueAction::Start => {
                let session = OrderRunSession {
                    session_id: progress_session_id(apparatus, order_id, actor, now),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    started_at_unix: now,
                    updated_at_unix: now,
                    payload_json: serde_json::json!({
                        "started_by": actor,
                    }),
                };
                let event = OrderProgressEvent {
                    event_id: progress_event_id(&session.session_id, order_id, action, now),
                    session_id: session.session_id.clone(),
                    batch_id: String::new(),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    produced_qty: 0.0,
                    uom: String::new(),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    qr_payload: String::new(),
                    return_ink_kg: None,
                    total_waste: None,
                    finished_goods_kg: None,
                    finished_goods_meter: None,
                    description: String::new(),
                    payload_json: serde_json::json!({"event": "start"}),
                };
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: None,
                })
            }
            queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::Complete => {
                let return_ink_kg = if action == queue_state::ApparatusQueueAction::Complete {
                    valid_optional_progress_qty(progress.return_ink_kg)?
                } else {
                    None
                };
                let total_waste = valid_optional_progress_qty(progress.total_waste)?;
                let finished_goods_kg = valid_optional_progress_qty(progress.finished_goods_kg)?;
                let finished_goods_meter =
                    valid_optional_progress_qty(progress.finished_goods_meter)?;
                if action == queue_state::ApparatusQueueAction::Complete
                    && pechat::pechat_color_count(apparatus).is_some()
                    && !bosma_completion_metrics_are_complete(
                        return_ink_kg,
                        total_waste,
                        finished_goods_kg,
                        finished_goods_meter,
                    )
                {
                    return Err(ProductionMapError::BosmaCompletionMetricsRequired);
                }
                let produced_qty =
                    valid_progress_qty(progress.produced_qty.or(finished_goods_meter))?;
                let uom = if progress.produced_qty.is_none() && finished_goods_meter.is_some() {
                    non_empty_or(&progress.uom, "m")
                } else {
                    non_empty_or(&progress.uom, "kg")
                };
                let description = progress.description.trim().to_string();
                let session = self
                    .store
                    .active_order_run_session(apparatus, order_id)
                    .await?
                    .unwrap_or_else(|| legacy_order_run_session(apparatus, order_id, actor, now));
                let status = match action {
                    queue_state::ApparatusQueueAction::Pause => OrderRunStatus::Paused,
                    queue_state::ApparatusQueueAction::Complete => OrderRunStatus::Completed,
                    _ => OrderRunStatus::Active,
                };
                let session = OrderRunSession {
                    status,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json: serde_json::json!({
                        "last_action": queue_action_str(action),
                        "last_qty": produced_qty,
                        "last_uom": uom,
                        "return_ink_kg": return_ink_kg,
                        "total_waste": total_waste,
                        "finished_goods_kg": finished_goods_kg,
                        "finished_goods_meter": finished_goods_meter,
                        "description": description,
                    }),
                    ..session
                };
                let batch_id = non_empty_or(
                    &progress.progress_batch_id,
                    &progress_batch_id(apparatus, order_id, action, now),
                );
                let qr_payload =
                    non_empty_or(&progress.qr_payload, &progress_qr_payload(&batch_id));
                let label_item_name = progress_label_item_name(order_map, apparatus, action);
                let batch = OrderProgressBatch {
                    batch_id: batch_id.clone(),
                    session_id: session.session_id.clone(),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    status: match action {
                        queue_state::ApparatusQueueAction::Pause => {
                            OrderProgressBatchStatus::Paused
                        }
                        queue_state::ApparatusQueueAction::Complete => {
                            OrderProgressBatchStatus::Completed
                        }
                        _ => return Err(ProductionMapError::ProgressInputInvalid),
                    },
                    produced_qty,
                    uom: uom.clone(),
                    qr_payload: qr_payload.clone(),
                    label_item_code: order_id.to_string(),
                    label_item_name,
                    executor_name: actor_display_name(actor),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    return_ink_kg,
                    total_waste,
                    finished_goods_kg,
                    finished_goods_meter,
                    description: description.clone(),
                    payload_json: serde_json::json!({
                        "order_title": order_map.title.trim(),
                        "apparatus": apparatus,
                        "action": queue_action_str(action),
                        "return_ink_kg": return_ink_kg,
                        "total_waste": total_waste,
                        "finished_goods_kg": finished_goods_kg,
                        "finished_goods_meter": finished_goods_meter,
                        "description": description.clone(),
                    }),
                };
                let event = OrderProgressEvent {
                    event_id: progress_event_id(&session.session_id, order_id, action, now),
                    session_id: session.session_id.clone(),
                    batch_id,
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    produced_qty,
                    uom,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    qr_payload,
                    return_ink_kg,
                    total_waste,
                    finished_goods_kg,
                    finished_goods_meter,
                    description: description.clone(),
                    payload_json: serde_json::json!({
                        "event": queue_action_str(action),
                        "return_ink_kg": return_ink_kg,
                        "total_waste": total_waste,
                        "finished_goods_kg": finished_goods_kg,
                        "finished_goods_meter": finished_goods_meter,
                        "description": description,
                    }),
                };
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: Some(batch),
                })
            }
            queue_state::ApparatusQueueAction::Resume => {
                if progress.progress_batch_id.trim().is_empty()
                    && progress.qr_payload.trim().is_empty()
                {
                    let session = self
                        .store
                        .active_order_run_session(apparatus, order_id)
                        .await?
                        .unwrap_or_else(|| {
                            legacy_order_run_session(apparatus, order_id, actor, now)
                        });
                    let session = OrderRunSession {
                        status: OrderRunStatus::Active,
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        updated_at_unix: now,
                        payload_json: serde_json::json!({
                            "resumed_without_progress_qr": true,
                        }),
                        ..session
                    };
                    let event = OrderProgressEvent {
                        event_id: progress_event_id(&session.session_id, order_id, action, now),
                        session_id: session.session_id.clone(),
                        batch_id: String::new(),
                        apparatus: apparatus.to_string(),
                        order_id: order_id.to_string(),
                        action,
                        produced_qty: 0.0,
                        uom: String::new(),
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        qr_payload: String::new(),
                        return_ink_kg: None,
                        total_waste: None,
                        finished_goods_kg: None,
                        finished_goods_meter: None,
                        description: String::new(),
                        payload_json: serde_json::json!({"event": "resume"}),
                    };
                    return Ok(QueueProgressRecords {
                        session: Some(session),
                        progress_event: Some(event),
                        progress_batch: None,
                    });
                }
                let mut batch = self
                    .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
                    .await?;
                if batch.status != OrderProgressBatchStatus::Paused
                    || batch.action != queue_state::ApparatusQueueAction::Pause
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                if batch.order_id.trim() != order_id
                    || !queue_state::apparatus_titles_match(&batch.apparatus, apparatus)
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                batch.status = OrderProgressBatchStatus::Resumed;
                batch.payload_json = serde_json::json!({
                    "resumed_by": actor,
                    "resumed_at_unix": now,
                });
                let session = self
                    .store
                    .order_run_session(&batch.session_id)
                    .await?
                    .or_else(|| Some(legacy_order_run_session(apparatus, order_id, actor, now)))
                    .map(|session| OrderRunSession {
                        status: OrderRunStatus::Active,
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        updated_at_unix: now,
                        payload_json: serde_json::json!({
                            "resumed_batch_id": batch.batch_id,
                            "resumed_qr_payload": batch.qr_payload,
                        }),
                        ..session
                    })
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                let event = OrderProgressEvent {
                    event_id: progress_event_id(&session.session_id, order_id, action, now),
                    session_id: session.session_id.clone(),
                    batch_id: batch.batch_id.clone(),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    produced_qty: 0.0,
                    uom: String::new(),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    qr_payload: batch.qr_payload.clone(),
                    return_ink_kg: None,
                    total_waste: None,
                    finished_goods_kg: None,
                    finished_goods_meter: None,
                    description: String::new(),
                    payload_json: serde_json::json!({"event": "resume"}),
                };
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: Some(batch),
                })
            }
        }
    }
}

fn valid_optional_progress_qty(value: Option<f64>) -> Result<Option<f64>, ProductionMapError> {
    match value {
        Some(value) if value.is_finite() && value > 0.0 => Ok(Some(value)),
        Some(_) => Err(ProductionMapError::ProgressInputInvalid),
        None => Ok(None),
    }
}

fn bosma_completion_metrics_are_complete(
    return_ink_kg: Option<f64>,
    total_waste: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
) -> bool {
    return_ink_kg.is_some()
        && total_waste.is_some()
        && finished_goods_kg.is_some()
        && finished_goods_meter.is_some()
}
