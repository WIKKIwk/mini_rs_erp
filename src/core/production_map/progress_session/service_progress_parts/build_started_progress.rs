impl ProductionMapService {
    async fn build_started_progress(
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
            canonical: _,
            now,
        } = context;
        let opening_wip_batch = self
            .opening_wip_start_batch(order_id, order_map, apparatus, &progress)
            .await?;
        let input_progress_batch = if opening_wip_batch.is_none() {
            self.previous_stage_start_progress_batch(order_id, order_map, apparatus, &progress)
                .await?
        } else {
            None
        };
        let opening_wip_target_stage_node_id = opening_wip_batch
            .as_ref()
            .and_then(|record| {
                Self::opening_wip_target_stage(order_map, &record.intake, apparatus, "")
            })
            .map(|stage| stage.node_id)
            .unwrap_or_default();
        let input_route = input_progress_batch.as_ref()
            .map(|batch| super::wip_route::resolve_wip_input_route(order_map, batch)).transpose()?;
        let expected_input = if let Some(batch) = &input_progress_batch {
            let raw = self.store.progress_batch(&batch.batch_id).await?
                .ok_or(ProductionMapError::ProgressBatchNotFound)?;
            if super::wip_route::normalized_route_batch(&raw) != *batch {
                return Err(ProductionMapError::WipRouteChanged);
            }
            Some(raw)
        } else { None };
        let mut input_progress = input_progress_batch
            .as_ref()
            .map(progress_links_from_batch)
            .or_else(|| {
                opening_wip_batch
                    .as_ref()
                    .map(|record| {
                        progress_links_from_opening_wip(
                            record,
                            &opening_wip_target_stage_node_id,
                        )
                    })
            })
            .unwrap_or_default();
        if let Some(route) = &input_route {
            input_progress.stage_node_id = route.stage_node_id.clone();
        }
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &input_progress.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let session_payload = start_session_payload(
            actor,
            &input_progress,
            input_progress_batch.as_ref(),
            &stage.node_id,
            now,
        );
        let session = OrderRunSession {
            session_id: progress_session_id(apparatus, order_id, actor),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            stage_node_id: stage.node_id.clone(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            started_at_unix: now,
            updated_at_unix: now,
            payload_json: session_payload,
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let mut event = zero_quantity_event(
            context,
            String::new(),
            String::new(),
            start_event_payload(
                &input_progress,
                input_progress_batch.as_ref(),
                &stage.node_id,
            ),
        );
        if let Some(expected) = &expected_input {
            event.payload_json["wip_input_map_fingerprint"] = serde_json::json!(
                super::wip_route::production_map_fingerprint(order_map));
            event.payload_json["wip_input_expected_batch"] = serde_json::to_value(expected)
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        let mut progress_batch_updates = Vec::new();
        if let Some(mut input_batch) = input_progress_batch {
            if let Some(mut route) = input_route {
                route.stage_node_id = stage.node_id.clone();
                input_batch.payload_json["wip_route_binding"] = serde_json::to_value(route)
                    .map_err(|_| ProductionMapError::StoreFailed)?;
            }
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
        let opening_wip_batch_updates = opening_wip_batch
            .map(|record| {
                vec![opening_wip_batch_in_use(
                    record.batch,
                    apparatus,
                    &session.session_id,
                    now,
                )]
            })
            .unwrap_or_default();
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates,
            opening_wip_batch_updates,
        })
    }
}

struct OutputProgressSources {
    session: OrderRunSession,
    stage: chain::ChainStage,
    opening_input_batch: Option<OpeningWipBatch>,
    input_batch: Option<OrderProgressBatch>,
    recovered_input: Option<RecoveredSessionInputBatch>,
    input_progress: SessionProgressLinks,
}

impl ProductionMapService {
    async fn output_progress_sources(
        &self,
        context: ProgressBuildContext<'_>,
        progress: &QueueProgressInput,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<OutputProgressSources, ProductionMapError> {
        let ProgressBuildContext { apparatus, order_id, order_map, action, canonical, now, .. } = context;
        let mut session = if let Some(read_snapshot) = read_snapshot {
            read_snapshot.active_session.clone()
        } else {
            self.store
                .active_order_run_session(apparatus, order_id)
                .await?
        }
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        ensure_wip_started_at(&mut session);
        let session_input_progress = session_progress_links(&session);
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &session_input_progress.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let session_uses_opening_wip = session_input_progress.source_kind == "opening_wip";
        if !session_uses_opening_wip
            && action != queue_state::ApparatusQueueAction::Freeze
            && apparatus::requires_previous_stage(canonical)
            && chain::previous_stage_resolution_is_unavailable(order_map, apparatus)
        {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let opening_input_batch = if session_uses_opening_wip {
            let record = if let Some(read_snapshot) = read_snapshot {
                read_snapshot.opening_wip_batch(
                    &session_input_progress.batch_id,
                    &session_input_progress.qr_payload,
                )
            } else {
                self.store
                    .opening_wip_batch(
                        &session_input_progress.batch_id,
                        &session_input_progress.qr_payload,
                    )
                    .await?
            }
                .ok_or(ProductionMapError::ProgressBatchNotFound)?;
            let explicit_batch_matches = progress.progress_batch_id.trim().is_empty()
                || record.batch.batch_id.trim() == progress.progress_batch_id.trim();
            let explicit_qr_matches = progress.qr_payload.trim().is_empty()
                || record
                    .batch
                    .qr_payload
                    .trim()
                    .eq_ignore_ascii_case(progress.qr_payload.trim());
            if record.intake.status != OpeningWipIntakeStatus::Confirmed
                || record.intake.order_id.trim() != order_id.trim()
                || record.batch.order_id.trim() != order_id.trim()
                || Self::opening_wip_target_stage(
                    order_map,
                    &record.intake,
                    apparatus,
                    &stage.node_id,
                )
                .is_none()
                || record.batch.wip_status != OpeningWipBatchStatus::InUse
                || !super::types::apparatus_ids_match(
                    &record.batch.used_by_apparatus,
                    apparatus,
                )
                || record.batch.used_by_session_id.trim() != session.session_id.trim()
                || !explicit_batch_matches
                || !explicit_qr_matches
            {
                return Err(ProductionMapError::ProgressBatchNotAccepted);
            }
            Some(record.batch)
        } else {
            None
        };
        let session_input_batch = if session_uses_opening_wip
            || session_input_progress.batch_id.trim().is_empty()
        {
            None
        } else if let Some(read_snapshot) = read_snapshot {
            read_snapshot.progress_batch(&session_input_progress.batch_id)
        } else {
            self.store
                .progress_batch(&session_input_progress.batch_id)
                .await?
        };
        let explicit_input_batch = if !session_uses_opening_wip
            && (!progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
            ) {
            self.previous_stage_active_progress_batch(
                order_id,
                order_map,
                apparatus,
                progress,
                &session.session_id,
                &stage.node_id,
                read_snapshot,
            )
            .await?
        } else {
            None
        };
        let previous_apparatus = chain::previous_work_stage_for_node(order_map, &stage.node_id)
            .and_then(|stage| stage.apparatus_id);
        let linked_input_batch = session_input_batch
            .as_ref()
            .filter(|batch| {
                let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
                    batch.current_apparatus.as_str()
                } else {
                    batch.used_by_apparatus.as_str()
                };
                previous_apparatus.as_ref().is_some_and(|_| {
                    batch.order_id.trim() == order_id.trim()
                        && super::wip_route::resolve_wip_input_route(order_map, batch).is_ok_and(|route|
                            route.consumer_apparatus_ids.iter().any(|id| id == apparatus)
                                && chain::stage_node_ids_match_for_map(order_map, &route.stage_node_id, &stage.node_id))
                        && batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && super::types::apparatus_ids_match(used_by_apparatus, apparatus)
                        && (batch.used_by_session_id.trim().is_empty()
                            || batch.used_by_session_id.trim() == session.session_id.trim())
                })
            })
            .cloned();
        let recovered_input =
            if explicit_input_batch.is_none() && linked_input_batch.is_none() {
                self.recoverable_session_input_batch(
                    apparatus,
                    order_id,
                    order_map,
                    &session,
                    now,
                    read_snapshot,
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
        } else if previous_apparatus.is_some() && !session_uses_opening_wip {
            return Err(ProductionMapError::ProgressQrRequired);
        } else {
            None
        };
        let mut input_progress = input_batch
            .as_ref()
            .map(progress_links_from_batch)
            .unwrap_or_else(|| session_input_progress.clone());
        if input_progress.stage_node_id.trim().is_empty() {
            input_progress.stage_node_id = stage.node_id.clone();
        }
        if input_progress.layout_lanes.is_empty() {
            input_progress.layout_lanes = session_input_progress.layout_lanes.clone();
        }
        if input_progress.contained_kadr_count.is_none() {
            input_progress.contained_kadr_count = session_input_progress.contained_kadr_count;
        }
        Ok(OutputProgressSources {
            session, stage, opening_input_batch, input_batch, recovered_input, input_progress,
        })
    }
}
