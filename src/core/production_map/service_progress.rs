use super::*;

use super::progress::{legacy_order_run_session, progress_session_id, unix_seconds};
use super::service::QueueProgressRecords;
use super::service_progress_metrics::{
    validated_laminatsiya_removed_roll_metrics, validated_laminatsiya_worker_handoff_metrics,
    validated_progress_metrics,
};
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

    async fn recoverable_session_input_batch(
        &self,
        apparatus: &str,
        order_id: &str,
        session: &OrderRunSession,
        actor: &QueueActionActor,
        now: i64,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let mut candidates = self
            .store
            .progress_batches_for_order(order_id)
            .await?
            .into_iter()
            .filter(|batch| {
                let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
                    batch.current_apparatus.as_str()
                } else {
                    batch.used_by_apparatus.as_str()
                };
                let paused_waiting = batch.status == OrderProgressBatchStatus::Paused
                    && batch.wip_status == OrderProgressBatchWipStatus::Waiting;
                let resumed_in_use = batch.status == OrderProgressBatchStatus::Resumed
                    && batch.wip_status == OrderProgressBatchWipStatus::InUse
                    && queue_state::apparatus_titles_match(used_by_apparatus, apparatus)
                    && (batch.used_by_session_id.trim().is_empty()
                        || batch.used_by_session_id.trim() == session.session_id.trim());
                batch.order_id.trim() == order_id.trim()
                    && batch.session_id.trim() == session.session_id.trim()
                    && batch.action == queue_state::ApparatusQueueAction::Pause
                    && queue_state::apparatus_titles_match(&batch.apparatus, apparatus)
                    && (paused_waiting || resumed_in_use)
            })
            .collect::<Vec<_>>();
        if candidates.len() > 1 {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let Some(mut batch) = candidates.pop() else {
            return Ok(None);
        };
        if batch.status == OrderProgressBatchStatus::Paused {
            batch.status = OrderProgressBatchStatus::Resumed;
            batch.payload_json = resumed_batch_payload(&batch, actor, now);
        }
        if !batch.payload_json.is_object() {
            batch.payload_json = serde_json::json!({});
        }
        batch.payload_json["recovered_missing_session_progress_link"] = serde_json::json!(true);
        batch.payload_json["recovered_at_unix"] = serde_json::json!(now);
        Ok(Some(wip_batch_in_use(
            batch,
            apparatus,
            &session.session_id,
            now,
        )))
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
        if action == queue_state::ApparatusQueueAction::Pause
            && (progress.worker_handoff || progress.remove_roll_from_apparatus)
        {
            return self
                .build_laminatsiya_worker_transition(
                    apparatus, order_id, order_map, actor, progress, now,
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
                let linked_input_batch = session_input_batch
                    .as_ref()
                    .filter(|batch| batch.wip_status == OrderProgressBatchWipStatus::InUse)
                    .cloned();
                let recovered_input_batch = if explicit_input_batch.is_none()
                    && linked_input_batch.is_none()
                {
                    self.recoverable_session_input_batch(
                        apparatus,
                        order_id,
                        &session,
                        actor,
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
                } else if let Some(batch) = recovered_input_batch {
                    Some(batch)
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
                        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                    if session.status != OrderRunStatus::Paused {
                        return Err(ProductionMapError::ProgressBatchNotResumable);
                    }
                    let session_input_progress = session_progress_links(&session);
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
                    let resumed_batch = if let Some(batch) = handoff_batch {
                        batch
                    } else {
                        let mut paused_batches = self
                            .store
                            .progress_batches_for_order(order_id)
                            .await?
                            .into_iter()
                            .filter(|batch| {
                                batch.session_id.trim() == session.session_id.trim()
                                    && batch.action == queue_state::ApparatusQueueAction::Pause
                                    && batch.status == OrderProgressBatchStatus::Paused
                                    && batch.wip_status == OrderProgressBatchWipStatus::Waiting
                                    && queue_state::apparatus_titles_match(
                                        &batch.apparatus,
                                        apparatus,
                                    )
                            })
                            .collect::<Vec<_>>();
                        if paused_batches.len() != 1 {
                            return Err(ProductionMapError::ProgressBatchNotResumable);
                        }
                        let mut batch = paused_batches
                            .pop()
                            .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                        batch.status = OrderProgressBatchStatus::Resumed;
                        batch.payload_json = resumed_batch_payload(&batch, actor, now);
                        wip_batch_in_use(batch, apparatus, &session.session_id, now)
                    };
                    let payload_json = if is_handoff {
                        resumed_handoff_session_payload(&session, &session_input_progress)
                    } else {
                        let mut payload = resumed_session_payload(&resumed_batch);
                        payload["resumed_without_progress_qr"] = serde_json::json!(true);
                        preserve_qolip_code(&session, payload)
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
                    return Ok(QueueProgressRecords {
                        session: Some(session),
                        progress_event: Some(event),
                        progress_batch: Some(resumed_batch.clone()),
                        progress_batches: Vec::new(),
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
                batch.payload_json = resumed_batch_payload(&batch, actor, now);
                batch = wip_batch_in_use(batch, apparatus, &batch_session_id, now);
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

    async fn build_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
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
            status: OrderRunStatus::Paused,
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
            action: queue_state::ApparatusQueueAction::Pause,
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
