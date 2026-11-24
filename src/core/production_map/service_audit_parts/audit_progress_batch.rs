fn audit_progress_batch(
    known_orders: &BTreeSet<String>,
    maps_by_id: &BTreeMap<String, &ProductionMapDefinition>,
    sessions_by_id: &BTreeMap<String, &OrderRunSession>,
    batches_by_id: &BTreeMap<String, OrderProgressBatch>,
    batch: &OrderProgressBatch,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    let order_id = batch.order_id.trim();
    let batch_id = batch.batch_id.trim();
    let apparatus = batch.apparatus.trim();
    if !known_orders.contains(order_id) {
        violations.push(ProductionWorkflowAuditViolation::new(
            "unknown_order_progress_batch",
            order_id,
            batch_id,
            "progress batch references an order that is not present in production maps",
        ));
    }
    if apparatus.is_empty() {
        violations.push(ProductionWorkflowAuditViolation::new(
            "blank_progress_batch_apparatus",
            order_id,
            batch_id,
            "every progress batch must identify its producing apparatus",
        ));
    }
    if batch.session_id.trim().is_empty() {
        violations.push(ProductionWorkflowAuditViolation::new(
            "blank_progress_batch_session",
            order_id,
            batch_id,
            "every progress batch must link to a run session",
        ));
    } else if let Some(session) = sessions_by_id.get(batch.session_id.trim()) {
        if session.order_id.trim() != order_id {
            violations.push(ProductionWorkflowAuditViolation::new(
                "progress_batch_session_order_mismatch",
                order_id,
                batch_id,
                "progress batch and run session reference different orders",
            ));
        }
    } else {
        violations.push(ProductionWorkflowAuditViolation::new(
            "progress_batch_session_not_found",
            order_id,
            batch_id,
            "progress batch references a missing run session",
        ));
    }
    if let Some(map) = maps_by_id.get(order_id)
        && !chain::map_has_work_stage_for_station(map, apparatus)
    {
        violations.push(ProductionWorkflowAuditViolation::new(
            "progress_batch_apparatus_mismatch",
            order_id,
            batch_id,
            "progress batch apparatus is not a stage of the order route",
        ));
    }

    if !batch.has_consistent_action_status() {
        violations.push(ProductionWorkflowAuditViolation::new(
            "progress_batch_status_action_mismatch",
            order_id,
            batch_id,
            "progress batch status and action do not describe the same execution transition",
        ));
    }

    match batch.wip_status {
        OrderProgressBatchWipStatus::Waiting | OrderProgressBatchWipStatus::Peremotka => {
            if !batch.used_by_session_id.trim().is_empty()
                || !batch.used_by_apparatus.trim().is_empty()
                || !batch.processed_by_session_id.trim().is_empty()
                || !batch.processed_by_apparatus.trim().is_empty()
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "waiting_wip_has_owner",
                    order_id,
                    batch_id,
                    "waiting WIP cannot still carry an in-use or processed owner",
                ));
            }
            if batch.current_apparatus.trim().is_empty() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "waiting_wip_missing_location",
                    order_id,
                    batch_id,
                    "waiting WIP must identify its current apparatus",
                ));
            }
        }
        OrderProgressBatchWipStatus::InUse => {
            if batch.used_by_session_id.trim().is_empty()
                || batch.used_by_apparatus.trim().is_empty()
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "in_use_wip_missing_usage",
                    order_id,
                    batch_id,
                    "in-use WIP must record used_by_session_id and used_by_apparatus",
                ));
            }
            if !batch.current_apparatus.trim().is_empty()
                && !queue_state::apparatus_ids_match(
                    &batch.current_apparatus,
                    &batch.used_by_apparatus,
                )
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "in_use_wip_location_mismatch",
                    order_id,
                    batch_id,
                    "in-use WIP current apparatus must match its usage owner",
                ));
            }
            if !batch.used_by_session_id.trim().is_empty()
                && !sessions_by_id.contains_key(batch.used_by_session_id.trim())
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "in_use_wip_session_not_found",
                    order_id,
                    batch_id,
                    "in-use WIP references a missing run session",
                ));
            }
        }
        OrderProgressBatchWipStatus::Processed => {
            if batch.processed_by_session_id.trim().is_empty()
                || batch.processed_by_apparatus.trim().is_empty()
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "processed_wip_missing_processor",
                    order_id,
                    batch_id,
                    "processed WIP must record processed_by_session_id and processed_by_apparatus",
                ));
            }
            let warehouse_processed = batch
                .processed_by_apparatus
                .trim()
                .to_ascii_lowercase()
                .starts_with("warehouse:");
            if !warehouse_processed
                && !batch.processed_by_session_id.trim().is_empty()
                && !sessions_by_id.contains_key(batch.processed_by_session_id.trim())
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "processed_wip_session_not_found",
                    order_id,
                    batch_id,
                    "processed WIP references a missing processing session",
                ));
            }
            if warehouse_processed
                && batch
                    .payload_json
                    .get("finished_goods_stock_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "accepted_wip_missing_stock_id",
                    order_id,
                    batch_id,
                    "warehouse-accepted WIP must reference finished_goods_stock_id",
                ));
            }
        }
    }

    let parent_id = batch.parent_batch_id.trim();
    if !parent_id.is_empty() {
        if parent_id == batch_id {
            violations.push(ProductionWorkflowAuditViolation::new(
                "progress_batch_self_parent",
                order_id,
                batch_id,
                "a progress batch cannot be its own parent",
            ));
        } else if let Some(parent) = batches_by_id.get(parent_id) {
            if parent.order_id.trim() != order_id {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "progress_batch_parent_order_mismatch",
                    order_id,
                    batch_id,
                    "progress batch lineage cannot cross order boundaries",
                ));
            }
            if batch.is_layer_remainder() {
                if !queue_state::apparatus_ids_match(&parent.apparatus, apparatus)
                    || parent.worker_ref != batch.worker_ref
                {
                    violations.push(ProductionWorkflowAuditViolation::new(
                        "layer_remainder_producer_mismatch",
                        order_id,
                        batch_id,
                        "returned input must retain the source roll's producer apparatus and worker",
                    ));
                }
            } else if !parent.next_apparatus.trim().is_empty()
                && !queue_state::apparatus_ids_match(&parent.next_apparatus, apparatus)
            {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "progress_batch_parent_apparatus_mismatch",
                    order_id,
                    batch_id,
                    "child progress batch must enter the parent batch's next apparatus",
                ));
            }
        } else {
            violations.push(ProductionWorkflowAuditViolation::new(
                "progress_batch_parent_not_found",
                order_id,
                batch_id,
                "progress batch references a missing parent batch",
            ));
        }
    }
}

fn audit_paused_session_progress(
    sessions: &[OrderRunSession],
    batches_by_id: &BTreeMap<String, OrderProgressBatch>,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    for session in sessions {
        if session.is_print_transfer_continuation() || super::temporary_pause::is_temporarily_paused(session) {
            continue;
        }
        if session
            .payload_json
            .get("requeued_at_tail")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            continue;
        }
        let is_laminatsiya_handoff = session
            .payload_json
            .get("worker_handoff")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let is_removed_handoff_roll = session
            .payload_json
            .get("roll_removed_from_apparatus")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        if is_laminatsiya_handoff || is_removed_handoff_roll {
            continue;
        }
        let (expected_action, expected_status) = match session.status {
            OrderRunStatus::Paused => (
                queue_state::ApparatusQueueAction::Pause,
                OrderProgressBatchStatus::Paused,
            ),
            OrderRunStatus::RollDetached => (
                queue_state::ApparatusQueueAction::DetachRoll,
                OrderProgressBatchStatus::RollDetached,
            ),
            _ => continue,
        };
        let matching = batches_by_id
            .values()
            .filter(|batch| {
                batch.session_id.trim() == session.session_id.trim()
                    && batch.order_id.trim() == session.order_id.trim()
                    && batch.action == expected_action
                    && batch.status == expected_status
                    && queue_state::apparatus_ids_match(&batch.apparatus, &session.apparatus)
            })
            .count();
        if matching == 0 {
            violations.push(ProductionWorkflowAuditViolation::new(
                "paused_session_progress_mismatch",
                session.order_id.trim(),
                session.session_id.trim(),
                "an interrupted session must have a matching progress batch",
            ));
        }
    }
}

fn audit_transfers(
    known_orders: &BTreeSet<String>,
    maps_by_id: &BTreeMap<String, &ProductionMapDefinition>,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    transfers: &[ProductionMapApparatusTransferRecord],
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    for transfer in transfers {
        let order_id = transfer.order_id.trim();
        let transfer_id = transfer.transfer_id.trim();
        if transfer_id.is_empty() || transfer.idempotency_key.trim().is_empty() {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_apparatus_transfer_receipt",
                order_id,
                transfer_id,
                "a transfer receipt must contain transfer_id and idempotency_key",
            ));
        }
        if !known_orders.contains(order_id) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "unknown_order_apparatus_transfer",
                order_id,
                transfer_id,
                "transfer receipt references an order that is not present in production maps",
            ));
        }
        if transfer.from_apparatus.trim().is_empty()
            || transfer.to_apparatus.trim().is_empty()
            || queue_state::apparatus_ids_match(&transfer.from_apparatus, &transfer.to_apparatus)
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_apparatus_transfer_route",
                order_id,
                transfer_id,
                "transfer receipt must identify two different apparatuses",
            ));
        }
        if transfer.reason.trim().is_empty() {
            violations.push(ProductionWorkflowAuditViolation::new(
                "apparatus_transfer_missing_reason",
                order_id,
                transfer_id,
                "emergency transfer must retain an operational reason",
            ));
        }
        if let Some(map) = maps_by_id.get(order_id)
            && (map.id.trim() != transfer.map.id.trim()
                || !chain::map_has_work_stage_for_station(map, &transfer.to_apparatus))
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "apparatus_transfer_map_mismatch",
                order_id,
                transfer_id,
                "transfer receipt map must be the order map and contain the target apparatus",
            ));
        }
        if transfer.session.order_id.trim() != order_id
            || transfer.session.status != OrderRunStatus::Paused
            || !matches!(transfer.queue_state, ApparatusQueueOrderState::Paused | ApparatusQueueOrderState::Pending)
            || (transfer.queue_state == ApparatusQueueOrderState::Pending
                && !transfer.session.is_requeued_at_tail())
            || !queue_state::apparatus_ids_match(
                &transfer.session.apparatus,
                &transfer.to_apparatus,
            )
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "apparatus_transfer_session_mismatch",
                order_id,
                transfer_id,
                "transfer receipt session must remain paused on the target apparatus",
            ));
        }
        let progress_matches = match &transfer.progress_batch {
            Some(batch) => {
                batch.order_id.trim() == order_id
                    && batch.status == OrderProgressBatchStatus::Paused
                    && queue_state::apparatus_ids_match(&batch.apparatus, &transfer.to_apparatus)
                    && batch.batch_id.trim() == transfer.progress_batch_id.trim()
            }
            None => {
                transfer.progress_batch_id.is_empty()
                    && ((transfer.preserves_output_history && transfer.session.is_print_transfer_continuation()
                        && transfer.progress_batch_updates.iter().all(|batch|
                            batch.used_by_session_id == transfer.session_id
                                && batch.used_by_apparatus == transfer.to_apparatus
                                && batch.wip_status == OrderProgressBatchWipStatus::InUse))
                        || (transfer.queue_state == ApparatusQueueOrderState::Pending
                            && transfer.session.is_requeued_at_tail()))
            }
        };
        if !progress_matches {
            violations.push(ProductionWorkflowAuditViolation::new(
                "apparatus_transfer_progress_mismatch",
                order_id,
                transfer_id,
                "transfer receipt progress batch must be the paused batch on the target apparatus",
            ));
        }
        // A receipt describes the committed transfer, not the live queue after
        // subsequent Resume/Complete or a second failover.
        if transfer.preserves_output_history { continue; }
        let source_state =
            queue_state_for_apparatus_order(queue_states, &transfer.from_apparatus, order_id);
        let target_state =
            queue_state_for_apparatus_order(queue_states, &transfer.to_apparatus, order_id);
        if matches!(
            source_state,
            Some(ApparatusQueueOrderState::InProgress | ApparatusQueueOrderState::Paused)
        )
            || target_state != Some(transfer.queue_state)
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "apparatus_transfer_queue_mismatch",
                order_id,
                transfer_id,
                "transfer receipt must leave the source free and preserve the target queue state",
            ));
        }
    }
}
