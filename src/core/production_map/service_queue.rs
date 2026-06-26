use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::apparatus::visible_order_ids_for_apparatus;
use super::progress::{
    effective_apparatus_queue_policy, effective_apparatus_queue_policy_record,
    queue_action_event_id, unix_seconds,
};

impl ProductionMapService {
    pub async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        self.store.apparatus_sequences().await
    }

    pub async fn effective_apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let maps = self.store.maps().await?;
        let sequences = self.store.apparatus_sequences().await?;
        Ok(sequences
            .into_iter()
            .map(|(apparatus, stored_sequence)| {
                let visible_order_ids = visible_order_ids_for_apparatus(&maps, &apparatus);
                let sequence = if visible_order_ids.is_empty() {
                    stored_sequence
                } else {
                    queue_state::effective_apparatus_sequence(&stored_sequence, &visible_order_ids)
                };
                (apparatus, sequence)
            })
            .collect())
    }

    pub async fn set_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let order_ids = order_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .map(|key| key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let current_sequence = sequences
            .get(&storage_key)
            .or_else(|| sequences.get(apparatus))
            .cloned()
            .unwrap_or_default();
        let states = all_states
            .get(&storage_key)
            .or_else(|| all_states.get(apparatus))
            .cloned()
            .unwrap_or_default();
        validate_active_sequence_barrier(&current_sequence, &order_ids, &states)?;
        self.store
            .put_apparatus_sequence(apparatus, order_ids)
            .await?;
        self.notify_live();
        Ok(())
    }

    pub async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        self.store.apparatus_queue_states().await
    }

    pub async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        self.store
            .completed_queue_orders_for_actor(actor_ref, limit)
            .await
    }

    pub async fn queue_action_logs_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        let order_id = order_id.trim().to_string();
        if order_id.is_empty() {
            return Ok(Vec::new());
        }
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(std::slice::from_ref(&order_id))
            .await?;
        Ok(logs_by_order.get(&order_id).cloned().unwrap_or_default())
    }

    pub async fn active_order_run_sessions_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        self.store
            .active_order_run_sessions_for_worker(worker_refs, worker_display_name, limit)
            .await
    }

    pub async fn progress_batches_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        self.store
            .progress_batches_for_worker(worker_refs, worker_display_name, limit)
            .await
    }

    pub async fn progress_qr_report(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<ProductionQrReport, ProductionMapError> {
        let scanned_batch = self
            .progress_batch_for_qr(progress_batch_id, qr_payload)
            .await?;
        let order_id = scanned_batch.order_id.trim().to_string();
        let order = self.raw_map(&order_id).await?;
        let mut progress_batches = self.store.progress_batches_for_order(&order_id).await?;
        if progress_batches.is_empty() {
            progress_batches.push(scanned_batch.clone());
        }
        let current_batch = current_progress_batch_for_report(&scanned_batch, &progress_batches);
        let is_stale = scanned_batch.wip_status == OrderProgressBatchWipStatus::Processed
            || current_batch
                .as_ref()
                .is_some_and(|batch| batch.batch_id.trim() != scanned_batch.batch_id.trim());
        let stale_reason = if !is_stale {
            String::new()
        } else if scanned_batch.wip_status == OrderProgressBatchWipStatus::Processed {
            "processed_by_next_stage".to_string()
        } else {
            "superseded_by_new_qr".to_string()
        };
        let queue_states =
            queue_states_for_order(self.store.apparatus_queue_states().await?, &order_id);
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(std::slice::from_ref(&order_id))
            .await?;
        let logs = logs_by_order.get(&order_id).cloned().unwrap_or_default();
        let opened_by = logs.first().map(|entry| ProductionQrOpenedBy {
            actor_role: entry.actor_role.clone(),
            actor_ref: entry.actor_ref.clone(),
            actor_display_name: entry.actor_display_name.clone(),
            opened_at_unix: entry.created_at_unix,
        });
        let run_sessions = self.store.order_run_sessions_for_order(&order_id).await?;
        let active_sessions = run_sessions
            .iter()
            .filter(|session| {
                matches!(
                    session.status,
                    OrderRunStatus::Active | OrderRunStatus::Paused
                )
            })
            .cloned()
            .collect();
        Ok(ProductionQrReport {
            scanned_batch,
            current_batch,
            is_stale,
            stale_reason,
            order,
            queue_states,
            logs,
            progress_batches,
            run_sessions,
            active_sessions,
            opened_by,
        })
    }

    pub async fn receive_finished_goods(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
        warehouse: &str,
        actor: QueueActionActor,
    ) -> Result<FinishedGoodsReceipt, ProductionMapError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let _guard = self.queue_action_guard().await;
        let mut batch = self
            .progress_batch_for_qr(progress_batch_id, qr_payload)
            .await?;
        if batch.action != queue_state::ApparatusQueueAction::Complete
            || batch.status != OrderProgressBatchStatus::Completed
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
            || !batch.next_apparatus.trim().is_empty()
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let now = unix_seconds();
        let (qty, uom) = finished_goods_qty_uom(&batch)?;
        let stock = FinishedGoodsStockEntry {
            id: format!("finished:{}", batch.batch_id.trim()),
            warehouse: warehouse.to_string(),
            order_id: batch.order_id.trim().to_string(),
            item_code: batch.label_item_code.trim().to_string(),
            item_name: batch.label_item_name.trim().to_string(),
            qty,
            uom,
            status: "available".to_string(),
            barcode: batch.qr_payload.trim().to_string(),
            source_progress_batch_id: batch.batch_id.trim().to_string(),
            accepted_by_role: actor.role.trim().to_string(),
            accepted_by_ref: actor.ref_.trim().to_string(),
            accepted_by_display_name: actor.display_name.trim().to_string(),
            accepted_at_unix: now,
            payload_json: serde_json::json!({
                "source": "production_finished_goods_receipt",
                "progress_batch_id": batch.batch_id.trim(),
                "qr_payload": batch.qr_payload.trim(),
                "warehouse": warehouse,
                "order_id": batch.order_id.trim(),
                "accepted_by_role": actor.role.trim(),
                "accepted_by_ref": actor.ref_.trim(),
                "accepted_by_display_name": actor.display_name.trim(),
                "accepted_at_unix": now,
            }),
        };
        batch.wip_status = OrderProgressBatchWipStatus::Processed;
        batch.current_location = warehouse.to_string();
        batch.processed_by_session_id = stock.id.clone();
        batch.processed_by_apparatus = format!("warehouse:{warehouse}");
        batch.payload_json["received_warehouse"] = serde_json::json!(warehouse);
        batch.payload_json["received_by_role"] = serde_json::json!(actor.role.trim());
        batch.payload_json["received_by_ref"] = serde_json::json!(actor.ref_.trim());
        batch.payload_json["received_by_display_name"] =
            serde_json::json!(actor.display_name.trim());
        batch.payload_json["received_at_unix"] = serde_json::json!(now);
        batch.payload_json["finished_goods_stock_id"] = serde_json::json!(stock.id);
        batch.payload_json["wip_status"] = serde_json::json!(batch.wip_status.as_str());
        batch.payload_json["current_location"] = serde_json::json!(batch.current_location);
        batch.payload_json["processed_by_session_id"] =
            serde_json::json!(batch.processed_by_session_id);
        batch.payload_json["processed_by_apparatus"] =
            serde_json::json!(batch.processed_by_apparatus);
        self.store
            .receive_finished_goods_batch(batch.clone(), stock.clone())
            .await?;
        self.notify_live();
        Ok(FinishedGoodsReceipt { batch, stock })
    }

    pub async fn wip_progress_batches(
        &self,
        apparatus: &str,
        current_location: &str,
        status: Option<OrderProgressBatchWipStatus>,
        order_id: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        let mut batches = self
            .store
            .wip_progress_batches(apparatus, current_location, status, order_id, limit)
            .await?;
        if batches.iter().any(|batch| {
            batch.current_apparatus.trim().is_empty() || batch.next_apparatus.trim().is_empty()
        }) {
            let maps_by_id = self
                .store
                .maps()
                .await?
                .into_iter()
                .map(|map| (map.id.trim().to_string(), map))
                .collect::<BTreeMap<_, _>>();
            for batch in &mut batches {
                if batch.current_apparatus.trim().is_empty() {
                    batch.current_apparatus = batch.apparatus.trim().to_string();
                    batch.current_apparatus_key =
                        queue_state::apparatus_search_key(&batch.current_apparatus);
                    if batch.current_location.trim().is_empty() {
                        batch.current_location = batch.current_apparatus.clone();
                    }
                    batch.payload_json["current_apparatus"] =
                        serde_json::json!(batch.current_apparatus);
                    batch.payload_json["current_apparatus_key"] =
                        serde_json::json!(batch.current_apparatus_key);
                    batch.payload_json["current_location"] =
                        serde_json::json!(batch.current_location);
                }
                if batch.next_apparatus.trim().is_empty()
                    && let Some(map) = maps_by_id.get(batch.order_id.trim())
                {
                    if let Some(next) =
                        chain::next_work_stage_station(map, &batch.current_apparatus)
                    {
                        batch.next_apparatus = next;
                        batch.payload_json["next_apparatus"] =
                            serde_json::json!(batch.next_apparatus);
                    }
                }
            }
        }
        Ok(batches)
    }

    pub async fn queue_action_logs_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        self.store
            .queue_action_logs_for_worker(worker_refs, worker_display_name, limit)
            .await
    }

    pub async fn completion_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        self.store.completion_requests(limit).await
    }

    pub async fn completion_request_decisions_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        self.store
            .completion_request_decisions_for_actor(actor_ref, limit)
            .await
    }

    pub async fn apparatus_queue_policy_records(
        &self,
    ) -> Result<Vec<ApparatusQueuePolicyRecord>, ProductionMapError> {
        Ok(self
            .store
            .apparatus_queue_policies()
            .await?
            .into_iter()
            .map(|(apparatus, policy)| effective_apparatus_queue_policy_record(&apparatus, policy))
            .collect())
    }

    pub async fn set_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<ApparatusQueuePolicyRecord, ProductionMapError> {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let record = effective_apparatus_queue_policy_record(apparatus, policy);
        if record.locked && record.policy != policy {
            return Err(ProductionMapError::ApparatusQueuePolicyLocked);
        }
        self.store
            .put_apparatus_queue_policy(apparatus, record.policy, actor)
            .await?;
        self.notify_live();
        Ok(record)
    }

    pub async fn apply_apparatus_queue_action(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
    ) -> Result<BTreeMap<String, String>, ProductionMapError> {
        Ok(self
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                order_id,
                action,
                assigned_apparatus,
                actor,
                QueueProgressInput::default(),
            )
            .await?
            .states)
    }

    pub async fn apply_apparatus_queue_action_with_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let prepared = self
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order_id,
                action,
                assigned_apparatus,
                actor,
                progress,
            )
            .await?;
        self.commit_prepared_queue_action(prepared).await
    }

    pub(crate) async fn prepare_apparatus_queue_action_with_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<PreparedApparatusQueueAction, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if !queue_state::apparatus_matches_assigned(apparatus, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .map(|key| key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let policy = effective_apparatus_queue_policy(
            apparatus,
            policies
                .get(&storage_key)
                .copied()
                .or_else(|| policies.get(apparatus).copied())
                .or_else(|| {
                    policies.iter().find_map(|(key, policy)| {
                        queue_state::apparatus_titles_match(key, apparatus).then_some(*policy)
                    })
                }),
        );
        let stored_sequence = sequences.get(&storage_key).cloned().unwrap_or_default();
        let all_maps = self.store.maps().await?;
        let visible_order_ids = visible_order_ids_for_apparatus(&all_maps, apparatus);
        let sequence =
            queue_state::effective_apparatus_sequence(&stored_sequence, &visible_order_ids);
        if !sequence.iter().any(|id| id.trim() == order_id) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let order_map = all_maps
            .iter()
            .find(|map| map.id.trim() == order_id)
            .ok_or(ProductionMapError::MapNotFound)?;
        let previous_progress_ready = if action == queue_state::ApparatusQueueAction::Start {
            self.previous_stage_start_progress_batch(order_id, order_map, apparatus, &progress)
                .await?
                .is_some()
        } else {
            false
        };
        let states = all_states.get(&storage_key).cloned().unwrap_or_default();
        let mut parsed = BTreeMap::new();
        for (id, value) in states {
            if let Some(state) = queue_state::ApparatusQueueOrderState::parse(&value) {
                parsed.insert(id, state);
            }
        }
        let from_state = parsed
            .get(order_id)
            .copied()
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        match policy {
            ApparatusQueuePolicy::StrictSequence if !previous_progress_ready => {
                queue_state::apply_queue_action(&sequence, &mut parsed, order_id, action)?;
            }
            ApparatusQueuePolicy::StrictSequence => {
                queue_state::apply_unordered_queue_action(&mut parsed, order_id, action)?;
            }
            ApparatusQueuePolicy::FreePick => {
                queue_state::apply_unordered_queue_action(&mut parsed, order_id, action)?;
            }
        }
        let to_state = parsed
            .get(order_id)
            .copied()
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let saved = parsed
            .into_iter()
            .map(|(id, state)| (id, state.as_str().to_string()))
            .collect::<BTreeMap<_, _>>();
        let mut event = ApparatusQueueActionEvent {
            event_id: queue_action_event_id(&storage_key, order_id, action),
            apparatus: storage_key.clone(),
            order_id: order_id.to_string(),
            action,
            from_state,
            to_state,
            policy,
            actor: actor.clone(),
            assigned_apparatus: assigned_apparatus
                .iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            payload_json: serde_json::json!({
                "requested_apparatus": apparatus,
                "storage_key": storage_key,
                "sequence": sequence,
                "visible_order_ids": visible_order_ids,
                "from_state": from_state.as_str(),
                "to_state": to_state.as_str(),
                "policy": policy.as_str(),
            }),
        };
        let progress = self
            .build_progress_records(&storage_key, order_id, order_map, action, &actor, progress)
            .await?;
        if let Some(batch) = progress.progress_batch.as_ref() {
            if action == queue_state::ApparatusQueueAction::Complete
                && batch.lamination_print_leftover_rolls.is_some()
                && batch.lamination_film_leftover_rolls.is_some()
            {
                let print_leftover = batch.lamination_print_leftover_rolls.unwrap_or_default();
                let film_leftover = batch.lamination_film_leftover_rolls.unwrap_or_default();
                let total_waste = batch.total_waste.unwrap_or_default();
                let finished_kg = batch.finished_goods_kg.unwrap_or_default();
                let finished_meter = batch.finished_goods_meter.unwrap_or_default();
                event.payload_json["notice_kind"] =
                    serde_json::Value::String("laminatsiya_double_leftover".to_string());
                event.payload_json["decision_required"] = serde_json::Value::Bool(false);
                event.payload_json["order_number"] =
                    serde_json::Value::String(order_map.order_number.trim().to_string());
                event.payload_json["order_title"] =
                    serde_json::Value::String(order_map.title.trim().to_string());
                event.payload_json["product_code"] =
                    serde_json::Value::String(order_map.product_code.trim().to_string());
                event.payload_json["description"] = serde_json::Value::String(format!(
                    "Laminatsiya tugatishda ikkala qavat qoldig'i yozildi. Bosmadan ortgan rulon: {print_leftover}. Plyonkadan ortgan rulon: {film_leftover}. Jami chiqindi: {total_waste} kg. Tayyor mahsulot: {finished_kg} kg, {finished_meter} m."
                ));
            }
        }
        Ok(PreparedApparatusQueueAction {
            apparatus: storage_key,
            states: saved,
            event,
            session: progress.session,
            progress_event: progress.progress_event,
            progress_batch: progress.progress_batch,
            progress_batch_updates: progress.progress_batch_updates,
        })
    }

    pub(crate) async fn commit_prepared_queue_action(
        &self,
        prepared: PreparedApparatusQueueAction,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.store
            .put_apparatus_queue_states_with_event_and_progress(
                &prepared.apparatus,
                prepared.states.clone(),
                prepared.event,
                prepared.session.clone(),
                prepared.progress_event.clone(),
                prepared.progress_batch.clone(),
                prepared.progress_batch_updates.clone(),
            )
            .await?;
        self.notify_live();
        Ok(ApparatusQueueActionResult {
            states: prepared.states,
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
        })
    }
}

fn current_progress_batch_for_report(
    scanned_batch: &OrderProgressBatch,
    progress_batches: &[OrderProgressBatch],
) -> Option<OrderProgressBatch> {
    let mut current = scanned_batch.clone();
    let mut seen = BTreeSet::from([current.batch_id.trim().to_string()]);
    loop {
        let next = progress_batches
            .iter()
            .filter(|batch| batch.parent_batch_id.trim() == current.batch_id.trim())
            .max_by(|left, right| {
                progress_batch_order_key(left).cmp(&progress_batch_order_key(right))
            })
            .cloned();
        let Some(next) = next else {
            break;
        };
        if !seen.insert(next.batch_id.trim().to_string()) {
            break;
        }
        current = next;
    }
    Some(current)
}

fn finished_goods_qty_uom(batch: &OrderProgressBatch) -> Result<(f64, String), ProductionMapError> {
    if let Some(qty) = batch.finished_goods_kg
        && qty > 0.0
    {
        return Ok((qty, "kg".to_string()));
    }
    if let Some(qty) = batch.finished_goods_meter
        && qty > 0.0
    {
        return Ok((qty, "m".to_string()));
    }
    if batch.produced_qty > 0.0 && !batch.uom.trim().is_empty() {
        return Ok((batch.produced_qty, batch.uom.trim().to_string()));
    }
    Err(ProductionMapError::ProgressInputInvalid)
}

fn progress_batch_order_key(batch: &OrderProgressBatch) -> (u128, String) {
    let stamp = batch
        .batch_id
        .split(':')
        .nth(1)
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or_default();
    (stamp, batch.batch_id.trim().to_string())
}

fn queue_states_for_order(
    queue_states: BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let order_id = order_id.trim();
    queue_states
        .into_iter()
        .filter_map(|(apparatus, states)| {
            states.get(order_id).map(|state| {
                (
                    apparatus,
                    BTreeMap::from([(order_id.to_string(), state.clone())]),
                )
            })
        })
        .collect()
}

fn validate_active_sequence_barrier(
    current_sequence: &[String],
    next_sequence: &[String],
    states: &BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    for (order_id, state) in states {
        let Some(parsed) = queue_state::ApparatusQueueOrderState::parse(state) else {
            continue;
        };
        if !parsed.is_active() {
            continue;
        }
        let order_id = order_id.trim();
        let Some(next_index) = next_sequence.iter().position(|id| id.trim() == order_id) else {
            return Err(ProductionMapError::QueueActionNotAllowed);
        };
        let current_index = current_sequence
            .iter()
            .position(|id| id.trim() == order_id)
            .unwrap_or(0);
        if next_index > current_index {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let allowed_before = current_sequence
            .iter()
            .take(current_index)
            .map(|id| id.trim())
            .collect::<BTreeSet<_>>();
        for id in next_sequence.iter().take(next_index) {
            if !allowed_before.contains(id.trim()) {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
        }
    }
    Ok(())
}
