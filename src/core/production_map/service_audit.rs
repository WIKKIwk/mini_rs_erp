use std::collections::{BTreeMap, BTreeSet};

use super::apparatus::visible_order_ids_for_apparatus;
use super::chain;
use super::queue_state;
use super::queue_state::ApparatusQueueOrderState;
use super::*;


impl ProductionMapService {
    pub async fn audit_production_workflow(
        &self,
    ) -> Result<ProductionWorkflowAuditReport, ProductionMapError> {
        let maps = self.store.maps().await?;
        let maps_by_id = maps
            .iter()
            .filter_map(|map| {
                let id = map.id.trim();
                (!id.is_empty()).then(|| (id.to_string(), map))
            })
            .collect::<BTreeMap<_, _>>();
        let known_orders = maps_by_id.keys().cloned().collect::<BTreeSet<_>>();
        let queue_states = self.store.apparatus_queue_states().await?;
        let sequences = self.store.apparatus_sequences().await?;
        let mut violations = Vec::new();
        let mut qr_owners = BTreeMap::<String, (String, Vec<(String, String)>)>::new();
        let mut active_sessions = BTreeMap::<(String, String), Vec<String>>::new();
        let mut active_queue_orders = BTreeMap::<String, Vec<String>>::new();

        audit_queue_states(
            &known_orders,
            &maps,
            &queue_states,
            &mut active_queue_orders,
            &mut violations,
        );
        audit_sequences(&known_orders, &maps, &sequences, &mut violations);

        for (order_id, apparatuses) in active_queue_orders {
            if apparatuses.len() > 1 {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "duplicate_active_queue_assignment",
                    &order_id,
                    &apparatuses.join(","),
                    "an order is active or paused on more than one apparatus",
                ));
            }
        }

        let sessions = self.store.order_run_sessions_for_audit().await?;
        let sessions_by_id = sessions
            .iter()
            .filter_map(|session| {
                let id = session.session_id.trim();
                (!id.is_empty()).then(|| (id.to_string(), session))
            })
            .collect::<BTreeMap<_, _>>();
        let mut checked_session_count = 0;
        for session in &sessions {
            checked_session_count += 1;
            audit_session(
                &known_orders,
                &maps_by_id,
                &queue_states,
                session,
                &mut active_sessions,
                &mut violations,
            );
        }

        let stored_batches = self.store.progress_batches_for_audit().await?;
        let mut batches_by_id = BTreeMap::<String, OrderProgressBatch>::new();
        for batch in stored_batches {
            let batch_id = batch.batch_id.trim().to_string();
            if batch_id.is_empty() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "blank_progress_batch_id",
                    batch.order_id.trim(),
                    "",
                    "every progress batch must have a stable batch_id",
                ));
                continue;
            }
            if batches_by_id.insert(batch_id.clone(), batch).is_some() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "duplicate_progress_batch_id",
                    "",
                    &batch_id,
                    "progress batch ids must be unique in the audit source",
                ));
            }
        }

        for batch in batches_by_id.values() {
            audit_progress_batch(
                &known_orders,
                &maps_by_id,
                &sessions_by_id,
                &batches_by_id,
                batch,
                &mut violations,
            );
            let qr = batch.qr_payload.trim();
            if !qr.is_empty() {
                qr_owners
                    .entry(qr.to_ascii_lowercase())
                    .or_insert_with(|| (qr.to_string(), Vec::new()))
                    .1
                    .push((
                        batch.order_id.trim().to_string(),
                        batch.batch_id.trim().to_string(),
                    ));
            }
        }

        audit_paused_session_progress(&sessions, &batches_by_id, &mut violations);

        for (qr_payload, owners) in qr_owners.values() {
            if owners.len() <= 1 {
                continue;
            }
            let batches = owners
                .iter()
                .map(|(_, batch_id)| batch_id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let order_id = owners
                .iter()
                .map(|(order_id, _)| order_id.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(",");
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_qr_payload",
                &order_id,
                qr_payload,
                &format!("duplicate progress QR is used by batches: {batches}"),
            ));
        }

        for ((apparatus, order_id), sessions) in active_sessions {
            if sessions.len() <= 1 {
                continue;
            }
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_active_order_session",
                &order_id,
                &apparatus,
                &format!(
                    "more than one active or paused session exists: {}",
                    sessions.join(",")
                ),
            ));
        }

        audit_transfers(
            &known_orders,
            &maps_by_id,
            &queue_states,
            &self.store.apparatus_transfers_for_audit().await?,
            &mut violations,
        );
        let capacity_snapshot = self.apparatus_capacity_snapshot().await?;
        audit_capacity(
            &known_orders,
            &capacity_snapshot.profiles,
            &capacity_snapshot.downtimes,
            &capacity_snapshot.reservations,
            &mut violations,
        );

        Ok(ProductionWorkflowAuditReport {
            ok: violations.is_empty(),
            checked_order_count: known_orders.len(),
            checked_batch_count: batches_by_id.len(),
            checked_session_count,
            violations,
        })
    }
}

fn audit_queue_states(
    known_orders: &BTreeSet<String>,
    maps: &[ProductionMapDefinition],
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    active_orders: &mut BTreeMap<String, Vec<String>>,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    for (apparatus, states) in queue_states {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            violations.push(ProductionWorkflowAuditViolation::new(
                "blank_queue_apparatus",
                "",
                "",
                "queue state groups must identify an apparatus",
            ));
        }
        let visible_order_ids = visible_order_ids_for_apparatus(maps, apparatus)
            .into_iter()
            .collect::<BTreeSet<_>>();
        for (order_id, raw_state) in states {
            let order_id = order_id.trim();
            if order_id.is_empty() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "blank_queue_order",
                    "",
                    apparatus,
                    "queue states must not contain an empty order id",
                ));
                continue;
            }
            if !known_orders.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "unknown_order_queue_state",
                    order_id,
                    apparatus,
                    "queue state references an order that is not present in production maps",
                ));
            }
            let Some(state) = ApparatusQueueOrderState::parse(raw_state) else {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "invalid_queue_state",
                    order_id,
                    apparatus,
                    "queue state must be pending, in_progress, paused, frozen, or completed",
                ));
                continue;
            };
            if known_orders.contains(order_id) && !visible_order_ids.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "queue_order_apparatus_mismatch",
                    order_id,
                    apparatus,
                    "queue state is stored on an apparatus that is not a stage of the order",
                ));
            }
            if state.is_active() {
                active_orders
                    .entry(order_id.to_string())
                    .or_default()
                    .push(apparatus.to_string());
            }
        }
    }
}

fn audit_sequences(
    known_orders: &BTreeSet<String>,
    maps: &[ProductionMapDefinition],
    sequences: &BTreeMap<String, Vec<String>>,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    for (apparatus, sequence) in sequences {
        let visible_order_ids = visible_order_ids_for_apparatus(maps, apparatus)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        for order_id in sequence {
            let order_id = order_id.trim();
            if order_id.is_empty() {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "blank_queue_sequence_order",
                    "",
                    apparatus,
                    "queue sequence must not contain an empty order id",
                ));
                continue;
            }
            if !seen.insert(order_id.to_string()) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "duplicate_queue_sequence_order",
                    order_id,
                    apparatus,
                    "an order appears more than once in an apparatus sequence",
                ));
            }
            if !known_orders.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "unknown_order_queue_sequence",
                    order_id,
                    apparatus,
                    "queue sequence references an order that is not present in production maps",
                ));
            } else if !visible_order_ids.contains(order_id) {
                violations.push(ProductionWorkflowAuditViolation::new(
                    "queue_sequence_apparatus_mismatch",
                    order_id,
                    apparatus,
                    "queue sequence contains an order that is not a stage of the order",
                ));
            }
        }
    }
}

fn audit_session(
    known_orders: &BTreeSet<String>,
    maps_by_id: &BTreeMap<String, &ProductionMapDefinition>,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    session: &OrderRunSession,
    active_sessions: &mut BTreeMap<(String, String), Vec<String>>,
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    let order_id = session.order_id.trim();
    let session_id = session.session_id.trim();
    let apparatus = session.apparatus.trim();
    if session_id.is_empty() {
        violations.push(ProductionWorkflowAuditViolation::new(
            "blank_run_session_id",
            order_id,
            apparatus,
            "every run session must have a stable session_id",
        ));
    }
    if apparatus.is_empty() {
        violations.push(ProductionWorkflowAuditViolation::new(
            "blank_run_session_apparatus",
            order_id,
            session_id,
            "every run session must identify an apparatus",
        ));
    }
    if !known_orders.contains(order_id) {
        violations.push(ProductionWorkflowAuditViolation::new(
            "unknown_order_run_session",
            order_id,
            session_id,
            "run session references an order that is not present in production maps",
        ));
    }
    let is_requeued = session.status == OrderRunStatus::Paused
        && session
            .payload_json
            .get("requeued_at_tail")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    if !is_requeued
        && matches!(
            session.status,
            OrderRunStatus::Active
                | OrderRunStatus::Paused
                | OrderRunStatus::Frozen
                | OrderRunStatus::RollDetached
        )
    {
        active_sessions
            .entry((apparatus.to_ascii_lowercase(), order_id.to_string()))
            .or_default()
            .push(session_id.to_string());
    }
    let Some(map) = maps_by_id.get(order_id) else {
        return;
    };
    if !is_requeued
        && !chain::map_has_work_stage_for_station(map, apparatus)
        && matches!(
            session.status,
            OrderRunStatus::Active
                | OrderRunStatus::Paused
                | OrderRunStatus::Frozen
                | OrderRunStatus::RollDetached
        )
    {
        violations.push(ProductionWorkflowAuditViolation::new(
            "run_session_apparatus_mismatch",
            order_id,
            apparatus,
            "active or paused run session is attached to an apparatus outside the order route",
        ));
    }
    let state = queue_state_for_apparatus_order(queue_states, apparatus, order_id);
    let expected = match session.status {
        OrderRunStatus::Active => Some(ApparatusQueueOrderState::InProgress),
        OrderRunStatus::Paused if is_requeued => None,
        OrderRunStatus::Paused | OrderRunStatus::RollDetached => {
            Some(ApparatusQueueOrderState::Paused)
        }
        OrderRunStatus::Frozen => Some(ApparatusQueueOrderState::Frozen),
        OrderRunStatus::Completed => None,
    };
    if let Some(expected) = expected {
        if state != Some(expected) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "session_queue_state_mismatch",
                order_id,
                apparatus,
                &format!(
                    "{} session requires queue state {}",
                    session.status.as_str(),
                    expected.as_str()
                ),
            ));
        }
    } else if state.is_some_and(ApparatusQueueOrderState::is_active) {
        violations.push(ProductionWorkflowAuditViolation::new(
            "completed_session_active_queue",
            order_id,
            apparatus,
            "completed run session cannot leave an active queue state",
        ));
    }
}


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
        OrderProgressBatchWipStatus::Waiting => {
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
            if !parent.next_apparatus.trim().is_empty()
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
        if transfer.progress_batch.order_id.trim() != order_id
            || transfer.progress_batch.status != OrderProgressBatchStatus::Paused
            || !queue_state::apparatus_ids_match(
                &transfer.progress_batch.apparatus,
                &transfer.to_apparatus,
            )
            || transfer.progress_batch.batch_id.trim() != transfer.progress_batch_id.trim()
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "apparatus_transfer_progress_mismatch",
                order_id,
                transfer_id,
                "transfer receipt progress batch must be the paused batch on the target apparatus",
            ));
        }
        let source_state =
            queue_state_for_apparatus_order(queue_states, &transfer.from_apparatus, order_id);
        let target_state =
            queue_state_for_apparatus_order(queue_states, &transfer.to_apparatus, order_id);
        if source_state.is_some_and(ApparatusQueueOrderState::is_active)
            || target_state != Some(ApparatusQueueOrderState::Paused)
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "apparatus_transfer_queue_mismatch",
                order_id,
                transfer_id,
                "transfer receipt must leave the source free and the target paused",
            ));
        }
    }
}


fn audit_capacity(
    known_orders: &BTreeSet<String>,
    profiles: &[ApparatusCapacityProfile],
    downtimes: &[ApparatusDowntime],
    reservations: &[ApparatusScheduleReservation],
    violations: &mut Vec<ProductionWorkflowAuditViolation>,
) {
    let mut profile_keys = BTreeSet::new();
    for profile in profiles {
        let key = profile.apparatus_id.as_str().to_string();
        if !profile_keys.insert(key) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_capacity_profile",
                "",
                &profile.apparatus,
                "each apparatus must have at most one capacity profile",
            ));
        }
        if profile.capacity_slots == 0
            || profile.efficiency_percent == 0
            || profile.efficiency_percent > 200
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_capacity_profile",
                "",
                &profile.apparatus,
                "capacity slots and efficiency must be within valid bounds",
            ));
        }
    }
    for downtime in downtimes {
        if downtime.id.trim().is_empty()
            || downtime.apparatus_id.as_str().trim().is_empty()
            || downtime.starts_at_unix <= 0
            || downtime.ends_at_unix <= downtime.starts_at_unix
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_apparatus_downtime",
                "",
                &downtime.id,
                "downtime must identify an apparatus and have a positive interval",
            ));
        }
    }

    let mut reservation_keys = BTreeSet::new();
    for reservation in reservations {
        let reservation_id = reservation.reservation_id.trim();
        let order_id = reservation.order_id.trim();
        if !known_orders.contains(order_id) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "unknown_order_schedule_reservation",
                order_id,
                reservation_id,
                "schedule reservation references an order that is not present in production maps",
            ));
        }
        if reservation_id.is_empty()
            || reservation.idempotency_key.trim().is_empty()
            || reservation.apparatus_id.as_str().trim().is_empty()
            || reservation.starts_at_unix <= 0
            || reservation.ends_at_unix <= reservation.starts_at_unix
            || reservation.reserved_duration_minutes == 0
        {
            violations.push(ProductionWorkflowAuditViolation::new(
                "invalid_schedule_reservation",
                order_id,
                reservation_id,
                "schedule reservation identity, apparatus, and interval are required",
            ));
        }
        if !reservation_keys.insert(reservation.idempotency_key.trim().to_ascii_lowercase()) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "duplicate_schedule_idempotency_key",
                order_id,
                reservation_id,
                "schedule idempotency keys must be unique",
            ));
        }
    }

    for reservation in reservations
        .iter()
        .filter(|reservation| reservation.status.reserves_capacity())
    {
        let same_apparatus = reservations.iter().filter(|other| {
            other.status.reserves_capacity()
                && other.apparatus_id == reservation.apparatus_id
                && other.starts_at_unix < reservation.ends_at_unix
                && reservation.starts_at_unix < other.ends_at_unix
        });
        let overlap_count = same_apparatus.count();
        let capacity_slots = profiles
            .iter()
            .find(|profile| profile.apparatus_id == reservation.apparatus_id)
            .map(|profile| profile.capacity_slots)
            .unwrap_or(1);
        if overlap_count > usize::from(capacity_slots) {
            violations.push(ProductionWorkflowAuditViolation::new(
                "capacity_overbooked",
                reservation.order_id.trim(),
                reservation.apparatus.trim(),
                "overlapping planned or active reservations exceed apparatus capacity",
            ));
        }
    }
}

fn queue_state_for_apparatus_order(
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    apparatus: &str,
    order_id: &str,
) -> Option<ApparatusQueueOrderState> {
    queue_states
        .iter()
        .find(|(stored_apparatus, _)| queue_state::apparatus_ids_match(stored_apparatus, apparatus))
        .and_then(|(_, states)| states.get(order_id.trim()))
        .and_then(|state| ApparatusQueueOrderState::parse(state))
}
