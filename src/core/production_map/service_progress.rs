use super::*;

use super::progress::{legacy_order_run_session, progress_session_id, unix_seconds};
use super::service::QueueProgressRecords;
use super::service_progress_metrics::validated_progress_metrics;
use super::service_progress_support::*;

impl ProductionMapService {
    pub async fn progress_batch_for_qr(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        let progress_batch_id = progress_batch_id.trim();
        let qr_payload = qr_payload.trim();
        let batch = if !progress_batch_id.is_empty() {
            let batch = self.store.progress_batch(progress_batch_id).await?;
            if let Some(batch) = batch {
                if !qr_payload.is_empty()
                    && !batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload)
                {
                    return Err(ProductionMapError::ProgressBatchNotFound);
                }
                Some(batch)
            } else {
                None
            }
        } else if !qr_payload.is_empty() {
            self.store.progress_batch_by_qr(qr_payload).await?
        } else {
            return Err(ProductionMapError::ProgressInputInvalid);
        };
        let mut batch = batch.ok_or(ProductionMapError::ProgressBatchNotFound)?;
        batch.refresh_status_detail();
        Ok(batch)
    }

    pub(in crate::core::production_map) async fn previous_stage_start_progress_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let Some(previous_apparatus) = chain::previous_work_stage_station(order_map, apparatus)
        else {
            return Ok(None);
        };
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
        if batch.order_id.trim() != order_id
            || !queue_state::apparatus_titles_match(&batch.apparatus, &previous_apparatus)
            || !matches!(
                batch.action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::RollComplete
                    | queue_state::ApparatusQueueAction::Complete
            )
            || !matches!(
                batch.status,
                OrderProgressBatchStatus::Paused
                    | OrderProgressBatchStatus::Completed
                    | OrderProgressBatchStatus::Resumed
            )
            || (!batch.next_apparatus.trim().is_empty()
                && !queue_state::next_stage_title_matches_apparatus(
                    &batch.next_apparatus,
                    apparatus,
                ))
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        Ok(Some(batch))
    }

    pub(in crate::core::production_map) async fn previous_stage_active_progress_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
        session_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let Some(previous_apparatus) = chain::previous_work_stage_station(order_map, apparatus)
        else {
            return Ok(None);
        };
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
        let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
            batch.current_apparatus.as_str()
        } else {
            batch.used_by_apparatus.as_str()
        };
        let source_wip_is_usable = batch.wip_status == OrderProgressBatchWipStatus::Waiting
            || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                && queue_state::apparatus_titles_match(used_by_apparatus, apparatus)
                && (batch.used_by_session_id.trim().is_empty()
                    || batch.used_by_session_id.trim() == session_id.trim()));
        if batch.order_id.trim() != order_id
            || !queue_state::apparatus_titles_match(&batch.apparatus, &previous_apparatus)
            || !matches!(
                batch.action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::RollComplete
                    | queue_state::ApparatusQueueAction::Complete
            )
            || !matches!(
                batch.status,
                OrderProgressBatchStatus::Paused
                    | OrderProgressBatchStatus::Completed
                    | OrderProgressBatchStatus::Resumed
            )
            || !source_wip_is_usable
            || (!batch.next_apparatus.trim().is_empty()
                && !queue_state::next_stage_title_matches_apparatus(
                    &batch.next_apparatus,
                    apparatus,
                ))
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        Ok(Some(batch))
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
                let input_progress_batch = self
                    .previous_stage_start_progress_batch(order_id, order_map, apparatus, &progress)
                    .await?;
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
                    payload_json: start_session_payload(actor, input_progress_batch.as_ref()),
                };
                let context = ProgressRecordContext {
                    session: &session,
                    apparatus,
                    order_id,
                    action,
                    actor,
                    now,
                };
                let event = zero_quantity_event(
                    context,
                    String::new(),
                    String::new(),
                    start_event_payload(input_progress_batch.as_ref()),
                );
                let progress_batch_updates = input_progress_batch
                    .map(|batch| wip_batch_in_use(batch, apparatus, &session.session_id, now))
                    .into_iter()
                    .collect();
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: None,
                    progress_batches: Vec::new(),
                    progress_batch_updates,
                })
            }
            queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete => {
                if action == queue_state::ApparatusQueueAction::RollComplete
                    && !apparatus::is_rezka_title(apparatus)
                {
                    return Err(ProductionMapError::ProgressInputInvalid);
                }
                let metrics = validated_progress_metrics(apparatus, action, &progress)?;
                let quantity = progress_quantity(&progress, metrics)?;
                let description = progress.description.trim().to_string();
                let session = self
                    .store
                    .active_order_run_session(apparatus, order_id)
                    .await?
                    .unwrap_or_else(|| legacy_order_run_session(apparatus, order_id, actor, now));
                let session_input_progress = session_progress_links(&session);
                let session_input_batch = if session_input_progress.batch_id.trim().is_empty() {
                    None
                } else {
                    self.store
                        .progress_batch(&session_input_progress.batch_id)
                        .await?
                };
                let explicit_input_batch = if !progress.progress_batch_id.trim().is_empty()
                    || !progress.qr_payload.trim().is_empty()
                {
                    self.previous_stage_active_progress_batch(
                        order_id,
                        order_map,
                        apparatus,
                        &progress,
                        &session.session_id,
                    )
                    .await?
                } else {
                    None
                };
                let input_batch = if let Some(batch) = explicit_input_batch {
                    if session_input_batch.as_ref().is_some_and(|session_batch| {
                        session_batch.wip_status == OrderProgressBatchWipStatus::InUse
                            && session_batch.batch_id.trim() != batch.batch_id.trim()
                    }) {
                        return Err(ProductionMapError::ProgressBatchNotAccepted);
                    }
                    Some(batch)
                } else if !session_input_progress.batch_id.trim().is_empty() {
                    let batch = session_input_batch
                        .filter(|batch| batch.wip_status == OrderProgressBatchWipStatus::InUse);
                    if batch.is_none()
                        && chain::previous_work_stage_station(order_map, apparatus).is_some()
                    {
                        return Err(ProductionMapError::ProgressQrRequired);
                    }
                    batch
                } else if chain::previous_work_stage_station(order_map, apparatus).is_some() {
                    return Err(ProductionMapError::ProgressQrRequired);
                } else {
                    None
                };
                let input_progress = input_batch
                    .as_ref()
                    .map(progress_links_from_batch)
                    .unwrap_or(session_input_progress);
                let payload_json = preserve_qolip_code(
                    &session,
                    progress_session_payload(
                        action,
                        quantity.produced_qty,
                        &quantity.uom,
                        metrics,
                        &description,
                        &input_progress,
                    ),
                );
                let session = OrderRunSession {
                    status: run_status_for_progress_action(action),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json,
                    ..session
                };
                let output_identities = if apparatus::is_rezka_title(apparatus) {
                    rezka_output_identities(apparatus, order_id, action, now, order_map)?
                } else {
                    vec![progress_output_identity(
                        apparatus,
                        order_id,
                        action,
                        now,
                        &progress,
                        &input_progress,
                    )]
                };
                let context = ProgressRecordContext {
                    session: &session,
                    apparatus,
                    order_id,
                    action,
                    actor,
                    now,
                };
                let mut batches = Vec::with_capacity(output_identities.len());
                for (index, identity) in output_identities.iter().enumerate() {
                    let mut batch = progress_batch_record(ProgressBatchRecordInput {
                        order_map,
                        context,
                        quantity: &quantity,
                        output_identity: identity,
                        input_progress: &input_progress,
                        metrics,
                        description: &description,
                    })?;
                    if apparatus::is_rezka_title(apparatus) {
                        apply_rezka_frame_metadata(&mut batch, identity, order_map, apparatus);
                        if index > 0 {
                            clear_rezka_duplicate_metrics(&mut batch);
                        }
                    }
                    batches.push(batch);
                }
                let mut progress_batch_updates = Vec::new();
                if let Some(input_batch) = input_batch {
                    progress_batch_updates.push(wip_batch_processed(
                        input_batch,
                        apparatus,
                        &session.session_id,
                        now,
                    ));
                }
                let output_identity = output_identities
                    .first()
                    .ok_or(ProductionMapError::ProgressInputInvalid)?;
                let mut event = progress_event_record(ProgressEventRecordInput {
                    context,
                    quantity,
                    output_identity: ProgressOutputIdentity {
                        batch_id: output_identity.batch_id.clone(),
                        qr_payload: output_identity.qr_payload.clone(),
                        frame_index: output_identity.frame_index,
                        frame_count: output_identity.frame_count,
                    },
                    metrics,
                    description: &description,
                });
                if batches.len() > 1 {
                    event.payload_json["rezka_output_batches"] = serde_json::Value::Array(
                        batches
                            .iter()
                            .map(|batch| {
                                serde_json::json!({
                                    "batch_id": batch.batch_id,
                                    "qr_payload": batch.qr_payload,
                                    "frame_index": batch
                                        .payload_json
                                        .get("rezka_frame_index")
                                        .and_then(serde_json::Value::as_u64),
                                    "frame_count": batch
                                        .payload_json
                                        .get("rezka_frame_count")
                                        .and_then(serde_json::Value::as_u64),
                                })
                            })
                            .collect(),
                    );
                }
                let progress_batch = batches.first().cloned();
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch,
                    progress_batches: batches,
                    progress_batch_updates,
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
                    let payload_json =
                        preserve_qolip_code(&session, resume_without_progress_payload());
                    let session = OrderRunSession {
                        status: OrderRunStatus::Active,
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        updated_at_unix: now,
                        payload_json,
                        ..session
                    };
                    let context = ProgressRecordContext {
                        session: &session,
                        apparatus,
                        order_id,
                        action,
                        actor,
                        now,
                    };
                    let event = zero_quantity_event(
                        context,
                        String::new(),
                        String::new(),
                        resume_event_payload(),
                    );
                    return Ok(QueueProgressRecords {
                        session: Some(session),
                        progress_event: Some(event),
                        progress_batch: None,
                        progress_batches: Vec::new(),
                        progress_batch_updates: Vec::new(),
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
                let batch_session_id = batch.session_id.clone();
                batch = wip_batch_in_use(batch, apparatus, &batch_session_id, now);
                batch.payload_json = resumed_batch_payload(actor, now);
                sync_wip_payload_fields(&mut batch);
                let session = self
                    .store
                    .order_run_session(&batch.session_id)
                    .await?
                    .or_else(|| Some(legacy_order_run_session(apparatus, order_id, actor, now)))
                    .map(|session| {
                        let payload_json =
                            preserve_qolip_code(&session, resumed_session_payload(&batch));
                        OrderRunSession {
                            status: OrderRunStatus::Active,
                            worker_role: actor.role.trim().to_string(),
                            worker_ref: actor.ref_.trim().to_string(),
                            worker_display_name: actor.display_name.trim().to_string(),
                            updated_at_unix: now,
                            payload_json,
                            ..session
                        }
                    })
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                let context = ProgressRecordContext {
                    session: &session,
                    apparatus,
                    order_id,
                    action,
                    actor,
                    now,
                };
                let event = zero_quantity_event(
                    context,
                    batch.batch_id.clone(),
                    batch.qr_payload.clone(),
                    resume_event_payload(),
                );
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: Some(batch),
                    progress_batches: Vec::new(),
                    progress_batch_updates: Vec::new(),
                })
            }
        }
    }
}
