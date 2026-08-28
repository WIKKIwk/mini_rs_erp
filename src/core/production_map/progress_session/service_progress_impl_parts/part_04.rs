impl ProductionMapService {
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
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let input_progress = session_progress_links(&session);
        let metrics = ProgressMetrics::default();
        let mut session_payload = preserve_qolip_lineage(
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
            opening_wip_batch_updates: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
        canonical: &RuntimeApparatusConfiguration,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        if !apparatus::is_laminatsiya_apparatus(canonical)
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
        if input_progress.source_kind == "opening_wip" {
            return self
                .build_opening_wip_laminatsiya_worker_transition(
                    apparatus,
                    order_id,
                    action,
                    actor,
                    progress,
                    now,
                    canonical,
                    session,
                    input_progress,
                )
                .await;
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
            || !super::types::apparatus_ids_match(used_by_apparatus, apparatus)
            || previous_apparatus.as_ref().is_some_and(|previous| {
                !super::types::apparatus_ids_match(&input_batch.apparatus, previous)
            })
            || (!input_batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(
                    order_map,
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
            validated_laminatsiya_removed_roll_metrics(apparatus, canonical, &progress)?
        } else {
            validated_laminatsiya_worker_handoff_metrics(apparatus, canonical, &progress)?
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
            wip_batch_worker_handoff(input_batch.clone(), apparatus, &session.session_id, now)
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
            payload_json: preserve_qolip_lineage(&session, session_payload),
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
            opening_wip_batch_updates: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_opening_wip_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
        canonical: &RuntimeApparatusConfiguration,
        session: OrderRunSession,
        input_progress: SessionProgressLinks,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let record = self
            .store
            .opening_wip_batch(&input_progress.batch_id, &input_progress.qr_payload)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if record.intake.status != OpeningWipIntakeStatus::Confirmed
            || record.intake.order_id.trim() != order_id.trim()
            || record.batch.order_id.trim() != order_id.trim()
            || record.batch.wip_status != OpeningWipBatchStatus::InUse
            || !super::types::apparatus_ids_match(
                &record.intake.resume_apparatus,
                apparatus,
            )
            || !super::types::apparatus_ids_match(
                &record.batch.used_by_apparatus,
                apparatus,
            )
            || record.batch.used_by_session_id.trim() != session.session_id.trim()
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let remove_roll = progress.remove_roll_from_apparatus;
        if remove_roll
            && session
                .payload_json
                .get("worker_handoff")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let metrics = if remove_roll {
            validated_laminatsiya_removed_roll_metrics(apparatus, canonical, &progress)?
        } else {
            validated_laminatsiya_worker_handoff_metrics(apparatus, canonical, &progress)?
        };
        let description = progress.description.trim().to_string();
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
            payload_json: preserve_qolip_lineage(&session, session_payload),
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
            record.batch.batch_id.clone(),
            record.batch.qr_payload.clone(),
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
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: if remove_roll {
                vec![opening_wip_batch_waiting(record.batch, now)]
            } else {
                Vec::new()
            },
        })
    }
}
