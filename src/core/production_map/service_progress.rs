use super::*;

use super::progress::{legacy_order_run_session, non_empty_or, progress_session_id, unix_seconds};
use super::service::QueueProgressRecords;
use super::service_progress_metrics::{
    ProgressMetrics, validated_laminatsiya_removed_roll_metrics,
    validated_laminatsiya_worker_handoff_metrics, validated_progress_metrics,
};
use super::service_progress_support::*;

struct RecoveredSessionInputBatch {
    input_batch: OrderProgressBatch,
    output_update: OrderProgressBatch,
}

fn progress_values_for_outputs(
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    progress: &QueueProgressInput,
    output_identities: &[ProgressOutputIdentity],
) -> Result<Vec<(ProgressQuantity, ProgressMetrics)>, ProductionMapError> {
    if apparatus::is_rezka_title(apparatus) && !progress.rezka_frames.is_empty() {
        if progress.rezka_frames.len() != output_identities.len() {
            return Err(ProductionMapError::RezkaFrameCountMismatch);
        }
        return progress
            .rezka_frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let has_explicit_waste = frame.has_explicit_waste();
                let frame_progress =
                    frame.to_queue_progress(progress, !has_explicit_waste);
                let mut metrics = validated_progress_metrics(apparatus, action, &frame_progress)?;
                if index > 0 && !has_explicit_waste {
                    metrics.rezka_bosma_waste = None;
                    metrics.rezka_lamination_waste = None;
                    metrics.rezka_edge_waste = None;
                    metrics.total_waste = None;
                }
                let quantity = progress_quantity(&frame_progress, metrics)?;
                Ok((quantity, metrics))
            })
            .collect();
    }

    let metrics = validated_progress_metrics(apparatus, action, progress)?;
    let quantity = progress_quantity(progress, metrics)?;
    Ok(vec![(quantity, metrics)])
}

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
        restore_self_consumed_wip(&mut batch);
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
                    | queue_state::ApparatusQueueAction::DetachRoll
                    | queue_state::ApparatusQueueAction::RollComplete
                    | queue_state::ApparatusQueueAction::Complete
            )
            || !matches!(
                batch.status,
                OrderProgressBatchStatus::Paused
                    | OrderProgressBatchStatus::RollDetached
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
                    | queue_state::ApparatusQueueAction::DetachRoll
                    | queue_state::ApparatusQueueAction::RollComplete
                    | queue_state::ApparatusQueueAction::Complete
            )
            || !matches!(
                batch.status,
                OrderProgressBatchStatus::Paused
                    | OrderProgressBatchStatus::RollDetached
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

    async fn recoverable_session_input_batch(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        session: &OrderRunSession,
        now: i64,
    ) -> Result<Option<RecoveredSessionInputBatch>, ProductionMapError> {
        let Some(previous_apparatus) = chain::previous_work_stage_station(order_map, apparatus)
        else {
            return Ok(None);
        };
        let batches = self.store.progress_batches_for_order(order_id).await?;
        let linked_batch_id = session_progress_links(session).batch_id;
        let mut output_candidates = batches
            .iter()
            .filter(|batch| {
                let linked_candidate = !linked_batch_id.trim().is_empty()
                    && batch.batch_id.trim() == linked_batch_id.trim();
                let unlinked_candidate = linked_batch_id.trim().is_empty()
                    && (matches!(
                        batch.status,
                        OrderProgressBatchStatus::Paused
                            | OrderProgressBatchStatus::RollDetached
                    )
                        || batch.wip_status == OrderProgressBatchWipStatus::InUse);
                batch.order_id.trim() == order_id.trim()
                    && batch.session_id.trim() == session.session_id.trim()
                    && matches!(
                        batch.action,
                        queue_state::ApparatusQueueAction::Pause
                            | queue_state::ApparatusQueueAction::DetachRoll
                    )
                    && queue_state::apparatus_titles_match(&batch.apparatus, apparatus)
                    && !batch.parent_batch_id.trim().is_empty()
                    && (linked_candidate || unlinked_candidate)
            })
            .cloned()
            .collect::<Vec<_>>();
        if output_candidates.len() > 1 {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let Some(output_batch) = output_candidates.pop() else {
            return Ok(None);
        };
        let Some(parent_batch) = batches.into_iter().find(|batch| {
            batch.batch_id.trim() == output_batch.parent_batch_id.trim()
                && batch.order_id.trim() == order_id.trim()
                && queue_state::apparatus_titles_match(&batch.apparatus, &previous_apparatus)
                && (batch.next_apparatus.trim().is_empty()
                    || queue_state::next_stage_title_matches_apparatus(
                        &batch.next_apparatus,
                        apparatus,
                    ))
        }) else {
            return Ok(None);
        };
        let used_by_apparatus = if parent_batch.used_by_apparatus.trim().is_empty() {
            parent_batch.current_apparatus.as_str()
        } else {
            parent_batch.used_by_apparatus.as_str()
        };
        let owned_in_use = parent_batch.wip_status == OrderProgressBatchWipStatus::InUse
            && queue_state::apparatus_titles_match(used_by_apparatus, apparatus)
            && (parent_batch.used_by_session_id.trim().is_empty()
                || parent_batch.used_by_session_id.trim() == session.session_id.trim());
        let prematurely_processed = parent_batch.wip_status
            == OrderProgressBatchWipStatus::Processed
            && queue_state::apparatus_titles_match(
                &parent_batch.processed_by_apparatus,
                apparatus,
            )
            && (parent_batch.processed_by_session_id.trim().is_empty()
                || parent_batch.processed_by_session_id.trim() == session.session_id.trim());
        if parent_batch.wip_status != OrderProgressBatchWipStatus::Waiting
            && !owned_in_use
            && !prematurely_processed
        {
            return Ok(None);
        }
        let mut input_batch = wip_batch_in_use(
            parent_batch,
            apparatus,
            &session.session_id,
            now,
        );
        input_batch.payload_json["recovered_original_input_link"] = serde_json::json!(true);
        input_batch.payload_json["recovered_at_unix"] = serde_json::json!(now);
        sync_wip_payload_fields(&mut input_batch);
        Ok(Some(RecoveredSessionInputBatch {
            input_batch,
            output_update: restore_misbound_output_wip(output_batch, now),
        }))
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
        if action == queue_state::ApparatusQueueAction::Freeze {
            return self
                .build_frozen_progress(apparatus, order_id, actor, progress, now)
                .await;
        }
        if (action == queue_state::ApparatusQueueAction::Pause && progress.worker_handoff)
            || (action == queue_state::ApparatusQueueAction::DetachRoll
                && progress.remove_roll_from_apparatus)
        {
            return self
                .build_laminatsiya_worker_transition(
                    apparatus, order_id, order_map, action, actor, progress, now,
                )
                .await;
        }
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
                let mut progress_batch_updates = Vec::new();
                if let Some(input_batch) = input_progress_batch {
                    let recovered_self_consumed = input_batch
                        .payload_json
                        .get("recovered_self_consumed_wip")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true);
                    if recovered_self_consumed {
                        for mut sibling in self.store.progress_batches_for_order(order_id).await? {
                            if repair_self_consumed_sibling_lineage(&mut sibling, &input_batch) {
                                progress_batch_updates.push(sibling);
                            }
                        }
                    }
                    progress_batch_updates.push(wip_batch_in_use(
                        input_batch,
                        apparatus,
                        &session.session_id,
                        now,
                    ));
                }
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: None,
                    progress_batches: Vec::new(),
                    progress_batch_updates,
                })
            }
            queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete => {
                if action == queue_state::ApparatusQueueAction::RollComplete
                    && !apparatus::is_rezka_title(apparatus)
                {
                    return Err(ProductionMapError::ProgressInputInvalid);
                }
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
                let previous_apparatus =
                    chain::previous_work_stage_station(order_map, apparatus);
                let linked_input_batch = session_input_batch
                    .as_ref()
                    .filter(|batch| {
                        let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
                            batch.current_apparatus.as_str()
                        } else {
                            batch.used_by_apparatus.as_str()
                        };
                        previous_apparatus.as_ref().is_some_and(|previous| {
                            batch.order_id.trim() == order_id.trim()
                                && queue_state::apparatus_titles_match(
                                    &batch.apparatus,
                                    previous,
                                )
                                && (batch.next_apparatus.trim().is_empty()
                                    || queue_state::next_stage_title_matches_apparatus(
                                        &batch.next_apparatus,
                                        apparatus,
                                    ))
                                && batch.wip_status == OrderProgressBatchWipStatus::InUse
                                && queue_state::apparatus_titles_match(
                                    used_by_apparatus,
                                    apparatus,
                                )
                                && (batch.used_by_session_id.trim().is_empty()
                                    || batch.used_by_session_id.trim()
                                        == session.session_id.trim())
                        })
                    })
                    .cloned();
                let recovered_input = if explicit_input_batch.is_none()
                    && linked_input_batch.is_none()
                {
                    self.recoverable_session_input_batch(
                        apparatus,
                        order_id,
                        order_map,
                        &session,
                        now,
                    )
                    .await?
                } else {
                    None
                };
                let input_batch = if let Some(batch) = explicit_input_batch {
                    if linked_input_batch.as_ref().is_some_and(|session_batch| {
                        session_batch.batch_id.trim() != batch.batch_id.trim()
                    }) {
                        return Err(ProductionMapError::ProgressBatchNotAccepted);
                    }
                    Some(batch)
                } else if let Some(batch) = linked_input_batch {
                    Some(batch)
                } else if let Some(recovered) = recovered_input.as_ref() {
                    Some(recovered.input_batch.clone())
                } else if previous_apparatus.is_some() {
                    return Err(ProductionMapError::ProgressQrRequired);
                } else {
                    None
                };
                let input_progress = input_batch
                    .as_ref()
                    .map(progress_links_from_batch)
                    .unwrap_or(session_input_progress);
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
                let frame_values =
                    progress_values_for_outputs(apparatus, action, &progress, &output_identities)?;
                let quantity = &frame_values[0].0;
                let metrics = frame_values[0].1;
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
                    let (frame_quantity, frame_metrics) =
                        frame_values.get(index).unwrap_or(&frame_values[0]);
                    let mut batch = progress_batch_record(ProgressBatchRecordInput {
                        order_map,
                        context,
                        quantity: frame_quantity,
                        output_identity: identity,
                        input_progress: &input_progress,
                        metrics: *frame_metrics,
                        frame_gross_qty: progress
                            .rezka_frames
                            .get(index)
                            .and_then(|frame| frame.gross_qty),
                        description: &description,
                    })?;
                    if apparatus::is_rezka_title(apparatus) {
                        apply_rezka_frame_metadata(&mut batch, identity, order_map, apparatus);
                        if index > 0 && progress.rezka_frames.is_empty() {
                            clear_rezka_duplicate_metrics(&mut batch);
                        }
                    }
                    batches.push(batch);
                }
                let input_was_recovered = recovered_input.is_some();
                let mut progress_batch_updates = recovered_input
                    .into_iter()
                    .map(|recovered| recovered.output_update)
                    .collect::<Vec<_>>();
                if let Some(input_batch) = input_batch {
                    if matches!(
                        action,
                        queue_state::ApparatusQueueAction::Pause
                            | queue_state::ApparatusQueueAction::DetachRoll
                    ) {
                        if input_was_recovered {
                            progress_batch_updates.push(input_batch);
                        }
                    } else {
                        progress_batch_updates.push(wip_batch_processed(
                            input_batch,
                            apparatus,
                            &session.session_id,
                            now,
                        ));
                    }
                }
                let output_identity = output_identities
                    .first()
                    .ok_or(ProductionMapError::ProgressInputInvalid)?;
                let mut event = progress_event_record(ProgressEventRecordInput {
                    context,
                    quantity: quantity.clone(),
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
            queue_state::ApparatusQueueAction::Freeze => {
                unreachable!("freeze is handled before progress action dispatch")
            }
            queue_state::ApparatusQueueAction::Resume => {
                if progress.progress_batch_id.trim().is_empty()
                    && progress.qr_payload.trim().is_empty()
                {
                    let session = self
                        .store
                        .active_order_run_session(apparatus, order_id)
                        .await?
                        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                    if !matches!(
                        session.status,
                        OrderRunStatus::Paused
                            | OrderRunStatus::Frozen
                            | OrderRunStatus::RollDetached
                    ) {
                        return Err(ProductionMapError::ProgressBatchNotResumable);
                    }
                    let session_input_progress = session_progress_links(&session);
                    let is_frozen = session.status == OrderRunStatus::Frozen
                        || session
                            .payload_json
                            .get("frozen_order")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true)
                        || session
                            .payload_json
                            .get("freeze_with_issue")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true);
                    if is_frozen {
                        let mut payload_json = session.payload_json.clone();
                        if !payload_json.is_object() {
                            payload_json = serde_json::json!({});
                        }
                        payload_json["resumed_after_freeze"] = serde_json::json!(true);
                        payload_json["resumed_without_progress_qr"] = serde_json::json!(true);
                        let session = OrderRunSession {
                            status: OrderRunStatus::Active,
                            worker_role: actor.role.trim().to_string(),
                            worker_ref: actor.ref_.trim().to_string(),
                            worker_display_name: actor.display_name.trim().to_string(),
                            updated_at_unix: now,
                            payload_json: preserve_qolip_code(&session, payload_json),
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
                    let handoff_batch = if !session_input_progress.batch_id.trim().is_empty() {
                        self.store
                            .progress_batch(&session_input_progress.batch_id)
                            .await?
                            .and_then(|batch| {
                                let is_worker_handoff = batch
                                    .payload_json
                                    .get("worker_handoff")
                                    .and_then(serde_json::Value::as_bool)
                                    == Some(true);
                                let is_removed_roll = batch
                                    .payload_json
                                    .get("roll_removed_from_apparatus")
                                    .and_then(serde_json::Value::as_bool)
                                    == Some(true);
                                let can_claim = is_worker_handoff
                                    || (is_removed_roll
                                        && batch.wip_status
                                            == OrderProgressBatchWipStatus::Waiting);
                                can_claim.then(|| {
                                    wip_batch_claimed_after_handoff(
                                        batch,
                                        apparatus,
                                        &session.session_id,
                                        now,
                                    )
                                })
                            })
                    } else {
                        None
                    };
                    let is_handoff = handoff_batch.is_some();
                    let resumed_batches = if let Some(batch) = handoff_batch {
                        vec![batch]
                    } else {
                        let mut paused_batches = self
                            .store
                            .progress_batches_for_order(order_id)
                            .await?
                            .into_iter()
                            .filter(|batch| {
                                batch.session_id.trim() == session.session_id.trim()
                                    && matches!(
                                        batch.action,
                                        queue_state::ApparatusQueueAction::Pause
                                            | queue_state::ApparatusQueueAction::DetachRoll
                                    )
                                    && matches!(
                                        batch.status,
                                        OrderProgressBatchStatus::Paused
                                            | OrderProgressBatchStatus::RollDetached
                                    )
                                    && batch.wip_status == OrderProgressBatchWipStatus::Waiting
                                    && queue_state::apparatus_titles_match(
                                        &batch.apparatus,
                                        apparatus,
                                    )
                            })
                            .collect::<Vec<_>>();
                        if apparatus::is_rezka_title(apparatus) {
                            let source_batch_id = session_input_progress.batch_id.trim();
                            if !source_batch_id.is_empty() {
                                paused_batches.retain(|batch| {
                                    batch.parent_batch_id.trim() == source_batch_id
                                });
                            }
                            if paused_batches.is_empty() {
                                return Err(ProductionMapError::ProgressBatchNotResumable);
                            }
                        } else if paused_batches.len() != 1 {
                            return Err(ProductionMapError::ProgressBatchNotResumable);
                        }
                        paused_batches
                            .into_iter()
                            .map(|mut batch| {
                                batch.status = OrderProgressBatchStatus::Resumed;
                                batch.payload_json = resumed_batch_payload(&batch, actor, now);
                                sync_wip_payload_fields(&mut batch);
                                batch
                            })
                            .collect::<Vec<_>>()
                    };
                    let resumed_batch = resumed_batches
                        .first()
                        .cloned()
                        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                    let payload_json = if is_handoff {
                        resumed_handoff_session_payload(&session, &session_input_progress)
                    } else {
                        resumed_session_payload(&session, &resumed_batch, true)
                    };
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
                        resumed_batch.batch_id.clone(),
                        resumed_batch.qr_payload.clone(),
                        resume_event_payload(),
                    );
                    let progress_batches = if !is_handoff
                        && apparatus::is_rezka_title(apparatus)
                    {
                        resumed_batches.clone()
                    } else {
                        Vec::new()
                    };
                    return Ok(QueueProgressRecords {
                        session: Some(session),
                        progress_event: Some(event),
                        progress_batch: Some(resumed_batch.clone()),
                        progress_batches,
                        progress_batch_updates: if is_handoff {
                            vec![resumed_batch]
                        } else {
                            Vec::new()
                        },
                    });
                }
                let mut batch = self
                    .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
                    .await?;
                if !matches!(
                    batch.status,
                    OrderProgressBatchStatus::Paused
                        | OrderProgressBatchStatus::RollDetached
                ) || !matches!(
                    batch.action,
                    queue_state::ApparatusQueueAction::Pause
                        | queue_state::ApparatusQueueAction::DetachRoll
                )
                    || batch.wip_status != OrderProgressBatchWipStatus::Waiting
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                if batch.order_id.trim() != order_id
                    || !queue_state::apparatus_titles_match(&batch.apparatus, apparatus)
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                batch.status = OrderProgressBatchStatus::Resumed;
                batch.payload_json = resumed_batch_payload(&batch, actor, now);
                sync_wip_payload_fields(&mut batch);
                let session = self
                    .store
                    .order_run_session(&batch.session_id)
                    .await?
                    .or_else(|| Some(legacy_order_run_session(apparatus, order_id, actor, now)))
                    .map(|session| {
                        let payload_json = resumed_session_payload(&session, &batch, false);
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

    async fn build_frozen_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let description = progress.description.trim().to_string();
        let session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .unwrap_or_else(|| legacy_order_run_session(apparatus, order_id, actor, now));
        let input_progress = session_progress_links(&session);
        let metrics = ProgressMetrics::default();
        let mut session_payload = preserve_qolip_code(
            &session,
            progress_session_payload(
                queue_state::ApparatusQueueAction::Freeze,
                0.0,
                &non_empty_or(&progress.uom, "kg"),
                metrics,
                &description,
                &input_progress,
            ),
        );
        session_payload["frozen_order"] = serde_json::json!(true);
        if progress.freeze_with_issue {
            session_payload["freeze_with_issue"] = serde_json::json!(true);
            session_payload["issue_note"] = serde_json::json!(&description);
        }
        let session = OrderRunSession {
            status: OrderRunStatus::Frozen,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: session_payload,
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action: queue_state::ApparatusQueueAction::Freeze,
            actor,
            now,
        };
        let mut progress_event = zero_quantity_event(
            context,
            String::new(),
            String::new(),
            progress_event_payload(
                queue_state::ApparatusQueueAction::Freeze,
                metrics,
                &description,
            ),
        );
        progress_event.description = description;
        progress_event.payload_json["frozen_order"] = serde_json::json!(true);
        if progress.freeze_with_issue {
            progress_event.payload_json["freeze_with_issue"] = serde_json::json!(true);
            let issue_note = progress_event.description.clone();
            progress_event.payload_json["issue_note"] = serde_json::json!(issue_note);
        }
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(progress_event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
        })
    }

    async fn build_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        if !apparatus::is_laminatsiya_title(apparatus)
            || (progress.worker_handoff && progress.remove_roll_from_apparatus)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let remove_roll = progress.remove_roll_from_apparatus;
        let session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if (!remove_roll && session.status != OrderRunStatus::Active)
            || (remove_roll && session.status != OrderRunStatus::Paused)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let input_progress = session_progress_links(&session);
        if input_progress.batch_id.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let input_batch = self
            .store
            .progress_batch(&input_progress.batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let previous_apparatus = chain::previous_work_stage_station(order_map, apparatus);
        let used_by_apparatus = if input_batch.used_by_apparatus.trim().is_empty() {
            input_batch.current_apparatus.as_str()
        } else {
            input_batch.used_by_apparatus.as_str()
        };
        if input_batch.order_id.trim() != order_id.trim()
            || input_batch.wip_status != OrderProgressBatchWipStatus::InUse
            || !queue_state::apparatus_titles_match(used_by_apparatus, apparatus)
            || previous_apparatus.as_ref().is_some_and(|previous| {
                !queue_state::apparatus_titles_match(&input_batch.apparatus, previous)
            })
            || (!input_batch.next_apparatus.trim().is_empty()
                && !queue_state::next_stage_title_matches_apparatus(
                    &input_batch.next_apparatus,
                    apparatus,
                ))
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        if remove_roll
            && input_batch
                .payload_json
                .get("worker_handoff")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let metrics = if remove_roll {
            validated_laminatsiya_removed_roll_metrics(apparatus, &progress)?
        } else {
            validated_laminatsiya_worker_handoff_metrics(apparatus, &progress)?
        };
        let description = progress.description.trim().to_string();
        let updated_input_batch = if remove_roll {
            wip_batch_removed_from_apparatus(
                input_batch.clone(),
                apparatus,
                metrics
                    .finished_goods_meter
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                metrics
                    .finished_goods_kg
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                metrics
                    .bobina_kg
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                now,
            )
        } else {
            wip_batch_worker_handoff(
                input_batch.clone(),
                apparatus,
                &session.session_id,
                now,
            )
        };
        let session_payload = if remove_roll {
            removed_roll_session_payload(metrics, &description, &input_progress)
        } else {
            worker_handoff_session_payload(metrics, &description, &input_progress)
        };
        let session = OrderRunSession {
            status: if remove_roll {
                OrderRunStatus::RollDetached
            } else {
                OrderRunStatus::Paused
            },
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: preserve_qolip_code(&session, session_payload),
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
        let event = progress_metrics_event(
            context,
            input_batch.batch_id,
            input_batch.qr_payload,
            metrics,
            &description,
            if remove_roll {
                "roll_removed_from_apparatus"
            } else {
                "worker_handoff"
            },
        );
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: vec![updated_input_batch],
        })
    }
}
