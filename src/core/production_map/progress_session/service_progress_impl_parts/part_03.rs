impl ProductionMapService {
    async fn build_resumed_progress(
        &self,
        context: ProgressBuildContext<'_>,
        progress: QueueProgressInput,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
            ..
        } = context;
        let opening_record = if !progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
        {
            self.store
                .opening_wip_batch(
                    progress.progress_batch_id.trim(),
                    progress.qr_payload.trim(),
                )
                .await?
        } else {
            None
        };
        if let Some(record) = opening_record {
                let current = self
                    .store
                    .active_order_run_session(apparatus, order_id)
                    .await?
                    .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                let links = session_progress_links(&current);
                if record.intake.status != OpeningWipIntakeStatus::Confirmed
                    || record.intake.order_id.trim() != order_id.trim()
                    || record.batch.order_id.trim() != order_id.trim()
                    || record.batch.wip_status != OpeningWipBatchStatus::Waiting
                    || links.source_kind != "opening_wip"
                    || links.batch_id.trim() != record.batch.batch_id.trim()
                    || !matches!(
                        current.status,
                        OrderRunStatus::Paused | OrderRunStatus::RollDetached
                    )
                    || Self::opening_wip_target_stage(
                        order_map,
                        &record.intake,
                        apparatus,
                        &links.stage_node_id,
                    )
                    .is_none()
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                let session = OrderRunSession {
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json: resumed_handoff_session_payload(&current, &links),
                    ..current
                };
                let event = zero_quantity_event(
                    ProgressRecordContext {
                        session: &session,
                        apparatus,
                        order_id,
                        action,
                        actor,
                    },
                    record.batch.batch_id.clone(),
                    record.batch.qr_payload.clone(),
                    resume_event_payload(),
                );
                let opening_update = opening_wip_batch_in_use(
                    record.batch,
                    apparatus,
                    &session.session_id,
                    now,
                );
            return Ok(QueueProgressRecords {
                session: Some(session),
                progress_event: Some(event),
                progress_batch: None,
                progress_batches: Vec::new(),
                progress_batch_updates: Vec::new(),
                opening_wip_batch_updates: vec![opening_update],
            });
        }
        if progress.progress_batch_id.trim().is_empty()
            && progress.qr_payload.trim().is_empty()
        {
            let mut session = self
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
            let is_requeued = session
                .payload_json
                .get("requeued_at_tail")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
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
                    == Some(true)
                || is_requeued;
            let recorded_rezka_pause = apparatus::is_rezka_apparatus(canonical)
                && session.payload_json.get("rezka_recorded_output_closed")
                    .and_then(serde_json::Value::as_bool) == Some(true);
            if is_frozen || recorded_rezka_pause {
                let mut payload_json = std::mem::take(&mut session.payload_json);
                if !payload_json.is_object() {
                    payload_json = serde_json::json!({});
                }
                if let Some(payload) = payload_json.as_object_mut() {
                    payload.remove("requeued_at_tail");
                    payload.remove("rezka_recorded_output_closed");
                    payload.insert(
                        "resumed_after_freeze".to_string(),
                        serde_json::json!(is_frozen),
                    );
                    payload.insert(
                        "resumed_without_progress_qr".to_string(),
                        serde_json::json!(true),
                    );
                }
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
                    opening_wip_batch_updates: Vec::new(),
                });
            }
            if session_input_progress.source_kind == "opening_wip"
                && session
                    .payload_json
                    .get("worker_handoff")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                let record = self
                    .store
                    .opening_wip_batch(
                        &session_input_progress.batch_id,
                        &session_input_progress.qr_payload,
                    )
                    .await?
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                if record.intake.status != OpeningWipIntakeStatus::Confirmed
                    || record.intake.order_id.trim() != order_id.trim()
                    || record.batch.order_id.trim() != order_id.trim()
                    || record.batch.wip_status != OpeningWipBatchStatus::InUse
                    || record.batch.used_by_session_id.trim() != session.session_id.trim()
                    || Self::opening_wip_target_stage(
                        order_map,
                        &record.intake,
                        apparatus,
                        &session_input_progress.stage_node_id,
                    )
                    .is_none()
                    || !super::types::apparatus_ids_match(
                        &record.batch.used_by_apparatus,
                        apparatus,
                    )
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                let payload_json = resumed_handoff_session_payload(
                    &session,
                    &session_input_progress,
                );
                let session = OrderRunSession {
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json,
                    ..session
                };
                let event = zero_quantity_event(
                    ProgressRecordContext {
                        session: &session,
                        apparatus,
                        order_id,
                        action,
                        actor,
                    },
                    record.batch.batch_id,
                    record.batch.qr_payload,
                    resume_event_payload(),
                );
                return Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: None,
                    progress_batches: Vec::new(),
                    progress_batch_updates: Vec::new(),
                    opening_wip_batch_updates: Vec::new(),
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
            let mut resumed_batches = if let Some(batch) = handoff_batch {
                vec![batch]
            } else {
                let mut paused_batches = self
                    .store
                    .progress_batches_for_order(order_id)
                    .await?
                    .into_iter()
                    .filter(|batch| {
                        batch.session_id.trim() == session.session_id.trim()
                            && batch.action.creates_resumable_output()
                            && batch.status.is_resumable()
                            && batch.wip_status == OrderProgressBatchWipStatus::Waiting
                            && super::types::apparatus_ids_match(
                                &batch.apparatus,
                                apparatus,
                            )
                    })
                    .collect::<Vec<_>>();
                if apparatus::is_rezka_apparatus(canonical) {
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
                        batch.refresh_status_detail();
                        batch
                    })
                    .collect::<Vec<_>>()
            };
            let resumed_batch = resumed_batches
                .first()
                .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
            let payload_json = if is_handoff {
                resumed_handoff_session_payload(&session, &session_input_progress)
            } else {
                resumed_session_payload(&session, resumed_batch, true)
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
            };
            let event = zero_quantity_event(
                context,
                resumed_batch.batch_id.clone(),
                resumed_batch.qr_payload.clone(),
                resume_event_payload(),
            );
            let is_rezka = apparatus::is_rezka_apparatus(canonical);
            let (progress_batch, progress_batches, progress_batch_updates) = if is_handoff {
                let resumed_batch = resumed_batches
                    .pop()
                    .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                (
                    Some(resumed_batch.clone()),
                    Vec::new(),
                    vec![resumed_batch],
                )
            } else if is_rezka {
                (resumed_batches.first().cloned(), resumed_batches, Vec::new())
            } else {
                (resumed_batches.pop(), Vec::new(), Vec::new())
            };
            return Ok(QueueProgressRecords {
                session: Some(session),
                progress_event: Some(event),
                progress_batch,
                progress_batches,
                progress_batch_updates,
                opening_wip_batch_updates: Vec::new(),
            });
        }
        let mut batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
        if !batch.status.is_resumable()
            || !batch.action.creates_resumable_output()
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        if batch.order_id.trim() != order_id
            || !super::types::apparatus_ids_match(&batch.apparatus, apparatus)
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        batch.status = OrderProgressBatchStatus::Resumed;
        batch.payload_json = resumed_batch_payload(&batch, actor, now);
        batch.refresh_status_detail();
        let session = self
            .store
            .order_run_session(&batch.session_id)
            .await?
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
            opening_wip_batch_updates: Vec::new(),
        })
    }
}
