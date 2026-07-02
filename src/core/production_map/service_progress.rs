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
                    | queue_state::ApparatusQueueAction::Complete
            )
            || !matches!(
                batch.status,
                OrderProgressBatchStatus::Paused
                    | OrderProgressBatchStatus::Completed
                    | OrderProgressBatchStatus::Resumed
            )
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
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
                    progress_batch_updates,
                })
            }
            queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::Complete => {
                let metrics = validated_progress_metrics(apparatus, action, &progress)?;
                let quantity = progress_quantity(&progress, metrics)?;
                let description = progress.description.trim().to_string();
                let session = self
                    .store
                    .active_order_run_session(apparatus, order_id)
                    .await?
                    .unwrap_or_else(|| legacy_order_run_session(apparatus, order_id, actor, now));
                let input_progress = session_progress_links(&session);
                let session = OrderRunSession {
                    status: run_status_for_progress_action(action),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json: progress_session_payload(
                        action,
                        quantity.produced_qty,
                        &quantity.uom,
                        metrics,
                        &description,
                        &input_progress,
                    ),
                    ..session
                };
                let output_identity = progress_output_identity(
                    apparatus,
                    order_id,
                    action,
                    now,
                    &progress,
                    &input_progress,
                );
                let context = ProgressRecordContext {
                    session: &session,
                    apparatus,
                    order_id,
                    action,
                    actor,
                    now,
                };
                let batch = progress_batch_record(ProgressBatchRecordInput {
                    order_map,
                    context,
                    quantity: &quantity,
                    output_identity: &output_identity,
                    input_progress: &input_progress,
                    metrics,
                    description: &description,
                })?;
                let mut progress_batch_updates = Vec::new();
                if !input_progress.batch_id.trim().is_empty()
                    && let Some(input_batch) =
                        self.store.progress_batch(&input_progress.batch_id).await?
                {
                    progress_batch_updates.push(wip_batch_processed(
                        input_batch,
                        apparatus,
                        &session.session_id,
                        now,
                    ));
                }
                let event = progress_event_record(ProgressEventRecordInput {
                    context,
                    quantity,
                    output_identity,
                    metrics,
                    description: &description,
                });
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: Some(batch),
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
                    let session = OrderRunSession {
                        status: OrderRunStatus::Active,
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        updated_at_unix: now,
                        payload_json: resume_without_progress_payload(),
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
                    .map(|session| OrderRunSession {
                        status: OrderRunStatus::Active,
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        updated_at_unix: now,
                        payload_json: resumed_session_payload(&batch),
                        ..session
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
                    progress_batch_updates: Vec::new(),
                })
            }
        }
    }
}
