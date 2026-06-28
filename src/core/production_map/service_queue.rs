use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::apparatus::visible_order_ids_for_apparatus;
use super::progress::{effective_apparatus_queue_policy_record, unix_seconds};
use super::service_queue_support::*;

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

    pub async fn order_status_detail(
        &self,
        order_id: &str,
    ) -> Result<ProductionOrderStatusDetail, ProductionMapError> {
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let queue_states =
            queue_states_for_order(self.store.apparatus_queue_states().await?, order_id);
        let progress_batches = self.store.progress_batches_for_order(order_id).await?;
        let run_sessions = self.store.order_run_sessions_for_order(order_id).await?;
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(&[order_id.to_string()])
            .await?;
        let logs = logs_by_order.get(order_id).cloned().unwrap_or_default();
        Ok(ProductionOrderStatusDetail::from_order_flow(
            &progress_batches,
            &run_sessions,
            &queue_states,
            &logs,
        ))
    }

    pub async fn order_status_details(
        &self,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let mut statuses = BTreeMap::new();
        for saved in self.maps().await? {
            let order_id = saved.map.id.trim();
            if order_id.is_empty() {
                continue;
            }
            statuses.insert(
                order_id.to_string(),
                self.order_status_detail(order_id).await?,
            );
        }
        Ok(statuses)
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
        for batch in &mut progress_batches {
            batch.refresh_status_detail();
        }
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
        let order_status = ProductionOrderStatusDetail::from_order_flow(
            &progress_batches,
            &run_sessions,
            &queue_states,
            &logs,
        );
        Ok(ProductionQrReport {
            scanned_batch,
            current_batch,
            is_stale,
            stale_reason,
            order,
            order_status,
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
        let stock = finished_goods_stock_entry(&batch, warehouse, &actor, qty, uom, now);
        mark_finished_goods_batch_received(&mut batch, &stock, warehouse, &actor, now);
        self.store
            .receive_finished_goods_batch(batch.clone(), stock.clone())
            .await?;
        let order_status = self.order_status_detail(&stock.order_id).await?;
        self.notify_live();
        Ok(FinishedGoodsReceipt {
            batch,
            stock,
            order_status,
        })
    }

    pub async fn wip_progress_batches(
        &self,
        apparatus: &str,
        next_apparatus: &str,
        current_location: &str,
        status: Option<OrderProgressBatchWipStatus>,
        include_processed: bool,
        order_id: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        let mut batches = self
            .store
            .wip_progress_batches(
                apparatus,
                next_apparatus,
                current_location,
                status,
                include_processed,
                order_id,
                limit,
            )
            .await?;
        if batches.iter().any(progress_batch_needs_location_repair) {
            let maps_by_id = self
                .store
                .maps()
                .await?
                .into_iter()
                .map(|map| (map.id.trim().to_string(), map))
                .collect::<BTreeMap<_, _>>();
            repair_wip_progress_batch_locations(&mut batches, &maps_by_id);
        }
        for batch in &mut batches {
            batch.refresh_status_detail();
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
        validate_queue_action_request(apparatus, order_id, assigned_apparatus)?;
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
        let known_keys = known_apparatus_storage_keys(&sequences, &all_states);
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let policy = queue_policy_for_apparatus(apparatus, &storage_key, &policies);
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
        let previous_progress_ready = self
            .previous_progress_ready_for_action(action, order_id, order_map, apparatus, &progress)
            .await?;
        let states = all_states.get(&storage_key).cloned().unwrap_or_default();
        let mut parsed = parsed_queue_states(states);
        let from_state = parsed
            .get(order_id)
            .copied()
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        apply_queue_policy(
            policy,
            previous_progress_ready,
            &sequence,
            &mut parsed,
            order_id,
            action,
        )?;
        let to_state = parsed
            .get(order_id)
            .copied()
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let mut saved = serialized_queue_states(parsed);
        let mut event = queue_action_event(
            apparatus,
            &storage_key,
            order_id,
            action,
            from_state,
            to_state,
            policy,
            &actor,
            assigned_apparatus,
            &sequence,
            &visible_order_ids,
        );
        let progress = self
            .build_progress_records(&storage_key, order_id, order_map, action, &actor, progress)
            .await?;
        if action == queue_state::ApparatusQueueAction::Complete
            && to_state == queue_state::ApparatusQueueOrderState::Completed
            && self
                .has_unprocessed_previous_wips(
                    order_id,
                    order_map,
                    &storage_key,
                    &progress.progress_batch_updates,
                )
                .await?
        {
            downgrade_completed_state_to_pending(order_id, &mut saved, &mut event);
        }
        append_laminatsiya_double_leftover_notice(
            action,
            progress.progress_batch.as_ref(),
            order_map,
            &mut event,
        );
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

    async fn previous_progress_ready_for_action(
        &self,
        action: queue_state::ApparatusQueueAction,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<bool, ProductionMapError> {
        if action != queue_state::ApparatusQueueAction::Start {
            return Ok(false);
        }
        Ok(self
            .previous_stage_start_progress_batch(order_id, order_map, apparatus, progress)
            .await?
            .is_some())
    }

    async fn has_unprocessed_previous_wips(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress_batch_updates: &[OrderProgressBatch],
    ) -> Result<bool, ProductionMapError> {
        let Some(previous_apparatus) = chain::previous_work_stage_station(order_map, apparatus)
        else {
            return Ok(false);
        };
        let mut batches = self
            .store
            .progress_batches_for_order(order_id)
            .await?
            .into_iter()
            .map(|batch| (batch.batch_id.trim().to_string(), batch))
            .collect::<BTreeMap<_, _>>();
        for batch in progress_batch_updates {
            batches.insert(batch.batch_id.trim().to_string(), batch.clone());
        }
        Ok(batches
            .values()
            .filter(|batch| {
                batch.order_id.trim() == order_id.trim()
                    && queue_state::apparatus_titles_match(&batch.apparatus, &previous_apparatus)
                    && queue_state::apparatus_titles_match(&batch.next_apparatus, apparatus)
            })
            .any(|batch| {
                batch.wip_status == OrderProgressBatchWipStatus::Waiting
                    || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && queue_state::apparatus_titles_match(&batch.used_by_apparatus, apparatus))
            }))
    }

    pub(crate) async fn commit_prepared_queue_action(
        &self,
        prepared: PreparedApparatusQueueAction,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let order_id = prepared.event.order_id.clone();
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
        let order_status = self.order_status_detail(&order_id).await?;
        self.notify_live();
        Ok(ApparatusQueueActionResult {
            states: prepared.states,
            order_status,
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
        })
    }
}
