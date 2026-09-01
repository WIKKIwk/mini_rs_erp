impl ProductionMapService {

    async fn completion_progress_build_snapshot(
        &self,
        order_id: &str,
        progress: &QueueProgressInput,
        active_session: Option<OrderRunSession>,
    ) -> Result<ProgressBuildReadSnapshot, ProductionMapError> {
        let (progress_batches, opening_wip_records) = tokio::join!(
            self.store.progress_batches_for_order(order_id),
            self.store.opening_wip_records(OpeningWipQuery {
                order_id: order_id.trim().to_string(),
                wip_status: None,
                limit: 10_000,
            }),
        );
        let progress_batches = progress_batches?;
        let opening_wip_records = opening_wip_records?;
        let mut input_progress_batch = if !progress.progress_batch_id.trim().is_empty() {
            progress_batches
                .iter()
                .find(|batch| batch.batch_id.trim() == progress.progress_batch_id.trim())
                .cloned()
        } else if !progress.qr_payload.trim().is_empty() {
            progress_batches
                .iter()
                .find(|batch| {
                    batch
                        .qr_payload
                        .trim()
                        .eq_ignore_ascii_case(progress.qr_payload.trim())
                })
                .cloned()
        } else {
            None
        };
        if input_progress_batch.is_none() && !progress.progress_batch_id.trim().is_empty() {
            input_progress_batch = self
                .store
                .progress_batch(progress.progress_batch_id.trim())
                .await?;
        } else if input_progress_batch.is_none() && !progress.qr_payload.trim().is_empty() {
            input_progress_batch = self
                .store
                .progress_batch_by_qr(progress.qr_payload.trim())
                .await?;
        }
        let mut input_opening_wip_batch = if !progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
        {
            opening_wip_records
                .iter()
                .find_map(|record| {
                    record.batches.iter().find(|batch| {
                        (!progress.progress_batch_id.trim().is_empty()
                            && batch.batch_id.trim() == progress.progress_batch_id.trim())
                            || (!progress.qr_payload.trim().is_empty()
                                && batch.qr_payload.trim() == progress.qr_payload.trim())
                    }).map(|batch| OpeningWipBatchRecord {
                        intake: record.intake.clone(),
                        batch: batch.clone(),
                    })
                })
        } else {
            None
        };
        let session_uses_opening_wip = active_session
            .as_ref()
            .is_some_and(|session| session_progress_links(session).source_kind == "opening_wip");
        if input_opening_wip_batch.is_none()
            && (session_uses_opening_wip || input_progress_batch.is_none())
            && (!progress.progress_batch_id.trim().is_empty()
                || !progress.qr_payload.trim().is_empty())
        {
            input_opening_wip_batch = self
                .store
                .opening_wip_batch(
                    progress.progress_batch_id.trim(),
                    progress.qr_payload.trim(),
                )
                .await?;
        }
        Ok(ProgressBuildReadSnapshot {
            active_session,
            progress_batches,
            opening_wip_records,
            input_progress_batch,
            input_opening_wip_batch,
        })
    }

    fn completion_input_batch_id_from_snapshot(
        progress: &QueueProgressInput,
        snapshot: &ProgressBuildReadSnapshot,
    ) -> String {
        if !progress.progress_batch_id.trim().is_empty() {
            return progress.progress_batch_id.trim().to_string();
        }
        if !progress.qr_payload.trim().is_empty() {
            if let Some(batch) = snapshot.input_progress_batch.as_ref() {
                return batch.batch_id.clone();
            }
            return snapshot
                .input_opening_wip_batch
                .as_ref()
                .map(|record| record.batch.batch_id.clone())
                .unwrap_or_default();
        }
        snapshot
            .active_session
            .as_ref()
            .map(session_progress_links)
            .map(|links| links.batch_id)
            .unwrap_or_default()
    }

    pub(crate) async fn commit_prepared_queue_action(
        &self,
        prepared: PreparedApparatusQueueAction,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.commit_prepared_queue_action_with_raw_material_stock(
            prepared,
            Vec::new(),
            Vec::new(),
            None,
        )
        .await
    }

    pub(crate) async fn commit_prepared_queue_action_with_raw_material_stock(
        &self,
        prepared: PreparedApparatusQueueAction,
        raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
        qolip_checkouts: Vec<crate::core::qolip::QolipCheckout>,
        returned_paint_report: Option<crate::core::returned_paint::ReturnedPaintRequest>,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let write = QueueActionProgressWrite {
            apparatus: prepared.apparatus,
            map_update: prepared.claimed_alternative_map,
            states: prepared.states,
            sequence_updates: prepared.sequence_updates,
            schedule_reservation_status: schedule_reservation_status_for_action(
                prepared.event.action,
            ),
            event: prepared.event,
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
            progress_batches: prepared.progress_batches,
            progress_batch_updates: prepared.progress_batch_updates,
            opening_wip_batch_updates: prepared.opening_wip_batch_updates,
            raw_material_stock_transitions,
            qolip_checkouts,
            returned_paint_report,
            order_control_update: prepared.order_control_update,
        };
        let write_result = self
            .store
            .put_apparatus_queue_states_with_event_and_progress(&write)
            .await?;
        let order_status = self.order_status_detail(&write.event.order_id).await?;
        self.notify_live();
        let QueueActionProgressWrite {
            states,
            session,
            progress_event,
            progress_batch,
            progress_batches,
            order_control_update,
            ..
        } = write;
        Ok(ApparatusQueueActionResult {
            states,
            order_status,
            order_control: order_control_update,
            session,
            progress_event,
            progress_batch,
            progress_batches,
            raw_material_stock_warehouses: write_result.raw_material_stock_warehouses,
            raw_material_stock_committed: write_result.raw_material_stock_committed,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn has_unprocessed_previous_wips_from_sources(
    order_id: &str,
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
    all_states: &ApparatusQueueStateMap,
    progress_batches: &[OrderProgressBatch],
    progress_batch_updates: &[OrderProgressBatch],
    opening_wip_records: &[OpeningWipRecord],
    opening_wip_batch_updates: &[OpeningWipBatch],
    ignored_batch_id: &str,
    stage_node_id: &str,
) -> bool {
    let mut opening_batches = opening_wip_records
        .iter()
        .filter(|record| {
            record.intake.status == OpeningWipIntakeStatus::Confirmed
                && ProductionMapService::opening_wip_target_stage(
                    order_map,
                    &record.intake,
                    apparatus,
                    stage_node_id,
                )
                .is_some()
        })
        .flat_map(|record| record.batches.iter())
        .map(|batch| (batch.batch_id.trim(), batch))
        .collect::<BTreeMap<_, _>>();
    for batch in opening_wip_batch_updates {
        opening_batches.insert(batch.batch_id.trim(), batch);
    }
    if !opening_batches.is_empty() {
        return opening_batches.values().any(|batch| {
            batch.batch_id.trim() != ignored_batch_id.trim()
                && matches!(
                    batch.wip_status,
                    OpeningWipBatchStatus::Waiting | OpeningWipBatchStatus::InUse
                )
        });
    }

    let mut batches = progress_batches
        .iter()
        .map(|batch| (batch.batch_id.trim(), batch))
        .collect::<BTreeMap<_, _>>();
    for batch in progress_batch_updates {
        batches.insert(batch.batch_id.trim(), batch);
    }
    has_unprocessed_previous_wips_from_batches(
        order_id,
        order_map,
        apparatus,
        canonical,
        all_states,
        batches.into_values(),
        ignored_batch_id,
        stage_node_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn has_unprocessed_previous_wips_from_batches<'a>(
    order_id: &str,
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
    all_states: &ApparatusQueueStateMap,
    batches: impl IntoIterator<Item = &'a OrderProgressBatch>,
    ignored_batch_id: &str,
    stage_node_id: &str,
) -> bool {
    let previous_apparatus = if stage_node_id.trim().is_empty() {
        chain::previous_work_stage_station(order_map, apparatus)
    } else {
        chain::previous_work_stage_for_node(order_map, stage_node_id)
            .and_then(|stage| stage.apparatus_id)
    };
    let Some(previous_apparatus) = previous_apparatus else {
        return false;
    };
    let requires_previous_stage_completion = apparatus::is_laminatsiya_apparatus(canonical)
        || apparatus::is_rezka_apparatus(canonical);
    let previous_stage_completed = all_states.iter().any(|(candidate, states)| {
        super::super::types::apparatus_ids_match(candidate, &previous_apparatus)
            && states
                .get(order_id)
                .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                == Some(queue_state::ApparatusQueueOrderState::Completed)
    });
    if requires_previous_stage_completion && !previous_stage_completed {
        return true;
    }
    batches
        .into_iter()
        .filter(|batch| {
            batch.order_id.trim() == order_id.trim()
                && super::super::types::apparatus_ids_match(
                    &batch.apparatus,
                    &previous_apparatus,
                )
                && chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus)
                && (stage_node_id.trim().is_empty()
                    || progress_batch_next_stage_node_id(batch).is_empty()
                    || progress_batch_next_stage_node_id(batch) == stage_node_id.trim())
        })
        .any(|batch| {
            if !ignored_batch_id.trim().is_empty()
                && batch.batch_id.trim() == ignored_batch_id.trim()
            {
                return false;
            }
            batch.wip_status == OrderProgressBatchWipStatus::Waiting
                || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                    && super::super::types::apparatus_ids_match(
                        &batch.used_by_apparatus,
                        apparatus,
                    ))
                || wip_batch_was_consumed_by_producer(batch)
        })
}
