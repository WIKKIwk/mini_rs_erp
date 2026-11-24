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
        let has_bulk_progress_batch = if !progress.progress_batch_id.trim().is_empty() {
            progress_batches
                .iter()
                .any(|batch| batch.batch_id.trim() == progress.progress_batch_id.trim())
        } else if !progress.qr_payload.trim().is_empty() {
            progress_batches
                .iter()
                .any(|batch| {
                    batch
                        .qr_payload
                        .trim()
                        .eq_ignore_ascii_case(progress.qr_payload.trim())
                })
        } else {
            false
        };
        let input_progress_batch = if has_bulk_progress_batch {
            None
        } else if !progress.progress_batch_id.trim().is_empty() {
            self
                .store
                .progress_batch(progress.progress_batch_id.trim())
                .await?
        } else if !progress.qr_payload.trim().is_empty() {
            self
                .store
                .progress_batch_by_qr(progress.qr_payload.trim())
                .await?
        } else {
            None
        };
        let has_progress_input = has_bulk_progress_batch || input_progress_batch.is_some();
        let has_bulk_opening_wip_batch = if !progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
        {
            opening_wip_records
                .iter()
                .any(|record| {
                    record.batches.iter().any(|batch| {
                        (!progress.progress_batch_id.trim().is_empty()
                            && batch.batch_id.trim() == progress.progress_batch_id.trim())
                            || (!progress.qr_payload.trim().is_empty()
                                && batch.qr_payload.trim() == progress.qr_payload.trim())
                    })
                })
        } else {
            false
        };
        let session_uses_opening_wip = active_session
            .as_ref()
            .is_some_and(|session| session_progress_links(session).source_kind == "opening_wip");
        let input_opening_wip_batch = if has_bulk_opening_wip_batch {
            None
        } else if (session_uses_opening_wip || !has_progress_input)
            && (!progress.progress_batch_id.trim().is_empty()
                || !progress.qr_payload.trim().is_empty())
        {
            self
                .store
                .opening_wip_batch(
                    progress.progress_batch_id.trim(),
                    progress.qr_payload.trim(),
                )
                .await?
        } else {
            None
        };
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
            if let Some(batch) = snapshot
                .progress_batches
                .iter()
                .find(|batch| {
                    batch
                        .qr_payload
                        .trim()
                        .eq_ignore_ascii_case(progress.qr_payload.trim())
                })
                .or_else(|| {
                    snapshot.input_progress_batch.as_ref().filter(|batch| {
                        batch
                            .qr_payload
                            .trim()
                            .eq_ignore_ascii_case(progress.qr_payload.trim())
                    })
                })
            {
                return batch.batch_id.clone();
            }
            return snapshot
                .opening_wip_batch_id("", progress.qr_payload.trim())
                .unwrap_or_default()
                .to_string();
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
        mut prepared: PreparedApparatusQueueAction,
        raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
        qolip_checkouts: Vec<crate::core::qolip::QolipCheckout>,
        returned_paint_report: Option<crate::core::returned_paint::ReturnedPaintRequest>,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let raw_material_stock_transitions = raw_material_stock_transitions
            .into_iter()
            .filter(|transition| {
                transition.kind != super::super::RawMaterialStockTransitionKind::Complete
                    || prepared.settles_completion_materials()
            })
            .collect();
        if let Some(session) = prepared.session.as_mut() {
            session.release_frozen_qolips();
        }
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
            print_preflight_hold_id: prepared.print_preflight_hold_id,
            print_preflight_cancel_hold_id: prepared.print_preflight_cancel_hold_id,
        };
        let write_result = self
            .store
            .put_apparatus_queue_states_with_event_and_progress(&write)
            .await?;
        // The write is durable. Invalidate before response reads can fail or
        // be cancelled, so live consumers cannot retain the pre-commit view.
        self.notify_live();
        let order_status = self.order_status_detail(&write.event.order_id).await?;
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
    let batches = progress_batches
        .iter()
        .chain(progress_batch_updates)
        .map(|batch| (batch.batch_id.trim(), batch))
        .collect::<BTreeMap<_, _>>();
    if apparatus::uses_lamination_workflow(canonical)
        && batches.values().any(|batch| {
            batch.is_layer_remainder()
                && batch.order_id.trim() == order_id.trim()
                && batch.batch_id.trim() != ignored_batch_id.trim()
                && (super::super::wip_route::actionable_wip(batch)
                    || batch.wip_status == OrderProgressBatchWipStatus::InUse)
                && super::super::wip_route::resolve_wip_input_route(order_map, batch)
                    .is_ok_and(|route| {
                        chain::stage_node_ids_match_for_map(
                            order_map, &route.stage_node_id, stage_node_id,
                        )
                    })
        })
    {
        return true;
    }
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
fn has_available_previous_wips_for_local_report(
    order_id: &str, order_map: &ProductionMapDefinition, apparatus: &str,
    canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
    all_states: &ApparatusQueueStateMap, progress_batches: &[OrderProgressBatch],
    progress_batch_updates: &[OrderProgressBatch], opening_wip_records: &[OpeningWipRecord],
    opening_wip_batch_updates: &[OpeningWipBatch], ignored_batch_id: &str, stage_node_id: &str,
    upstream_closed: Option<bool>,
) -> bool {
    // Only available returns on this machine defer its local final report.
    // A return in use at a peer still blocks the shared stage from closing.
    let latest_batches = progress_batches
        .iter()
        .chain(progress_batch_updates)
        .map(|batch| (batch.batch_id.trim(), batch))
        .collect::<BTreeMap<_, _>>();
    if latest_batches.values().any(|batch| {
        batch.is_layer_remainder()
            && batch.order_id.trim() == order_id.trim()
            && batch.batch_id.trim() != ignored_batch_id.trim()
            && super::super::wip_route::actionable_wip(batch)
            && super::super::wip_route::resolve_wip_input_route(order_map, batch)
                .is_ok_and(|route| {
                    route.consumer_apparatus_ids.iter().any(|id| id == apparatus)
                        && chain::stage_node_ids_match_for_map(
                            order_map,
                            &route.stage_node_id,
                            stage_node_id,
                        )
                })
    }) {
        return true;
    }
    // A peer's claimed roll keeps the operation open, but is not work this
    // machine can scan next and must not suppress its own final accounting.
    let batches = latest_batches.values().copied()
        .filter(|b| !b.is_layer_remainder()
            && (b.wip_status != OrderProgressBatchWipStatus::InUse || b.used_by_apparatus == apparatus))
        .cloned().collect::<Vec<_>>();
    let opening = opening_wip_records.iter().cloned().map(|mut r| {
        r.batches.retain(|b| b.wip_status != OpeningWipBatchStatus::InUse || b.used_by_apparatus == apparatus); r
    }).collect::<Vec<_>>();
    let mut states = all_states.clone();
    if let Some(closed) = upstream_closed {
        for p in chain::previous_work_stages_for_node(order_map, stage_node_id) {
            if let Some(id) = p.apparatus_id { states.entry(id).or_default().insert(order_id.into(), if closed { "completed" } else { "pending" }.into()); }
        }
    }
    has_unprocessed_previous_wips_from_sources(order_id, order_map, apparatus, canonical,
        &states, &batches, &[], &opening, opening_wip_batch_updates,
        ignored_batch_id, stage_node_id)
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
    let previous_stage = chain::previous_work_stage_for_node(order_map, stage_node_id);
    let previous_apparatus = if stage_node_id.trim().is_empty() {
        chain::previous_work_stage_station(order_map, apparatus)
    } else {
        previous_stage.as_ref().and_then(|stage| stage.apparatus_id.clone())
    };
    let Some(previous_apparatus) = previous_apparatus else {
        return false;
    };
    let requires_previous_stage_completion = apparatus::uses_lamination_workflow(canonical)
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
        .filter(|batch| !batch.is_layer_remainder())
        .filter(|batch| {
            let source_node = batch.payload_json.get("stage_node_id")
                .and_then(serde_json::Value::as_str).unwrap_or_default().trim();
            let route = super::super::wip_route::resolve_wip_input_route(order_map, batch).ok();
            let target_node = route.as_ref().map(|r| r.stage_node_id.as_str())
                .unwrap_or_else(|| progress_batch_next_stage_node_id(batch));
            batch.order_id.trim() == order_id.trim()
                // A preceding operation may have produced its output on more
                // than one alternative. Concrete stage history outranks the
                // currently assigned apparatus, without merging later reentry.
                && if let Some(previous) = previous_stage.as_ref().filter(|_| !source_node.is_empty()) {
                    chain::stage_node_ids_match_for_map(order_map, source_node, &previous.node_id)
                } else {
                    super::super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
                }
                // Output made before a downstream map extension has no stored
                // destination. It is still outstanding input from this stage.
                && if !stage_node_id.trim().is_empty() && !target_node.is_empty() {
                    chain::stage_node_ids_match_for_map(order_map, target_node, stage_node_id)
                } else {
                    batch.next_apparatus.trim().is_empty()
                        || chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus)
                }
        })
        .any(|batch| {
            if !ignored_batch_id.trim().is_empty()
                && batch.batch_id.trim() == ignored_batch_id.trim()
            {
                return false;
            }
            batch.wip_status == OrderProgressBatchWipStatus::Waiting
                // Required input still being worked on at a peer alternative
                // is not a finished operation either.
                || batch.wip_status == OrderProgressBatchWipStatus::InUse
                || wip_batch_was_consumed_by_producer(batch)
        })
}
