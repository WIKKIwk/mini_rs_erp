use std::collections::BTreeSet;

use crate::core::apparatus_groups::{apparatus_id_for_name, apparatus_master_data_for_name};

use super::apparatus::{
    move_allowed, reassign_alternative_apparatus_assignment, reassign_apparatus_nodes,
};
use super::compiler::compile_map;
use super::pechat;
use super::queue_state;
use super::service::ProductionMapService;
use super::store_port::ProductionMapApparatusTransferWrite;
use super::types::*;

impl ProductionMapService {
    pub async fn transfer_apparatus_order(
        &self,
        input: ProductionMapApparatusTransferRequest,
        actor: QueueActionActor,
    ) -> Result<ProductionMapApparatusTransferResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = input.order_id.trim().to_ascii_lowercase();
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        let reason = input.reason.trim();
        let idempotency_key = input.idempotency_key.trim();
        if order_id.is_empty() || from.is_empty() || to.is_empty() || from == to {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        if reason.is_empty() {
            return Err(ProductionMapError::ApparatusTransferReasonRequired);
        }
        if idempotency_key.is_empty() || idempotency_key.len() > 200 {
            return Err(ProductionMapError::ApparatusTransferIdempotencyRequired);
        }

        if let Some(record) = self
            .store
            .apparatus_transfer_by_idempotency_key(idempotency_key)
            .await?
        {
            if record.order_id.trim() != order_id
                || !queue_state::apparatus_titles_match(&record.from_apparatus, from)
                || !queue_state::apparatus_titles_match(&record.to_apparatus, to)
            {
                return Err(ProductionMapError::ApparatusTransferIdempotencyConflict);
            }
            return self.transfer_result(record).await;
        }

        let maps = self.store.maps().await?;
        let map = maps
            .iter()
            .find(|map| map.id.trim() == order_id)
            .cloned()
            .ok_or(ProductionMapError::MapNotFound)?;
        if !move_allowed(&map, from, to) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        self.ensure_transfer_target_capabilities(to).await?;

        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .map(|key| key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let from_key = queue_state::resolve_apparatus_storage_key(from, &known_keys);
        let to_key = queue_state::resolve_apparatus_storage_key(to, &known_keys);
        let target_apparatus_id = apparatus_id_for_name(&to_key);
        if from_key == to_key {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let mut from_states = all_states.get(&from_key).cloned().unwrap_or_default();
        let mut to_states = all_states.get(&to_key).cloned().unwrap_or_default();
        let source_state = from_states
            .get(&order_id)
            .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state));
        if source_state != Some(queue_state::ApparatusQueueOrderState::Paused) {
            return Err(ProductionMapError::ApparatusTransferOrderNotPaused);
        }
        if to_states.contains_key(&order_id) {
            return Err(ProductionMapError::ApparatusTransferTargetConflict);
        }

        let source_session = self
            .store
            .active_order_run_session(&from_key, &order_id)
            .await?
            .ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
        if source_session.status != OrderRunStatus::Paused
            || source_session.order_id.trim() != order_id
            || !queue_state::apparatus_titles_match(&source_session.apparatus, &from_key)
        {
            return Err(ProductionMapError::ApparatusTransferSessionMismatch);
        }

        let progress_batches = self.store.progress_batches_for_order(&order_id).await?;
        let paused_batches = progress_batches
            .iter()
            .filter(|batch| {
                batch.session_id.trim() == source_session.session_id.trim()
                    && batch.action == queue_state::ApparatusQueueAction::Pause
                    && batch.status == OrderProgressBatchStatus::Paused
                    && queue_state::apparatus_titles_match(&batch.apparatus, &from_key)
            })
            .collect::<Vec<_>>();
        if paused_batches.len() != 1 {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
        let mut progress_batch = paused_batches
            .into_iter()
            .next()
            .cloned()
            .ok_or(ProductionMapError::ApparatusTransferProgressNotFound)?;

        let transfer_id = format!("apparatus-transfer:{idempotency_key}");
        let now = unix_seconds();
        let transfer_payload = serde_json::json!({
            "transfer_id": transfer_id,
            "from_apparatus": from_key,
            "to_apparatus": to_key,
            "reason": reason,
            "actor": actor,
            "created_at_unix": now,
        });

        let mut session = source_session;
        session.apparatus = to_key.clone();
        session.updated_at_unix = now;
        if !session.payload_json.is_object() {
            session.payload_json = serde_json::json!({});
        }
        session.payload_json["last_apparatus_transfer"] = transfer_payload.clone();

        progress_batch.apparatus = to_key.clone();
        progress_batch.current_apparatus = to_key.clone();
        progress_batch.current_apparatus_key = queue_state::apparatus_search_key(&to_key);
        progress_batch.current_location = format!("{to_key} chiqim");
        if queue_state::apparatus_titles_match(&progress_batch.used_by_apparatus, from) {
            progress_batch.used_by_apparatus = to_key.clone();
        }
        if queue_state::apparatus_titles_match(&progress_batch.processed_by_apparatus, from) {
            progress_batch.processed_by_apparatus = to_key.clone();
        }
        if !progress_batch.payload_json.is_object() {
            progress_batch.payload_json = serde_json::json!({});
        }
        progress_batch.payload_json["last_apparatus_transfer"] = transfer_payload;
        progress_batch.payload_json["current_apparatus"] =
            serde_json::json!(progress_batch.current_apparatus);
        progress_batch.payload_json["current_apparatus_key"] =
            serde_json::json!(progress_batch.current_apparatus_key);
        progress_batch.payload_json["current_location"] =
            serde_json::json!(progress_batch.current_location);
        progress_batch.refresh_status_detail();
        progress_batch.payload_json["status_detail"] =
            serde_json::json!(progress_batch.status_detail);

        let mut progress_batch_updates = Vec::new();
        if !progress_batch.parent_batch_id.trim().is_empty() {
            let Some(mut parent_batch) = progress_batches
                .iter()
                .find(|candidate| {
                    candidate.batch_id.trim() == progress_batch.parent_batch_id.trim()
                })
                .cloned()
            else {
                return Err(ProductionMapError::ApparatusTransferProgressMismatch);
            };
            if parent_batch.order_id.trim() != order_id {
                return Err(ProductionMapError::ApparatusTransferProgressMismatch);
            }
            parent_batch.next_apparatus = to_key.clone();
            if !parent_batch.payload_json.is_object() {
                parent_batch.payload_json = serde_json::json!({});
            }
            parent_batch.payload_json["next_apparatus"] =
                serde_json::json!(parent_batch.next_apparatus);
            parent_batch.refresh_status_detail();
            parent_batch.payload_json["status_detail"] =
                serde_json::json!(parent_batch.status_detail);
            progress_batch_updates.push(parent_batch);
        }

        let mut updated_map = map;
        if !reassign_alternative_apparatus_assignment(&mut updated_map, from, to)
            && !reassign_apparatus_nodes(&mut updated_map, from, to)
        {
            return Err(ProductionMapError::MoveNotAllowed);
        }

        from_states.remove(&order_id);
        to_states.insert(
            order_id.clone(),
            queue_state::ApparatusQueueOrderState::Paused
                .as_str()
                .to_string(),
        );
        let mut from_sequence = sequences.get(&from_key).cloned().unwrap_or_default();
        from_sequence.retain(|id| id.trim() != order_id);
        let mut to_sequence = sequences.get(&to_key).cloned().unwrap_or_default();
        to_sequence.retain(|id| id.trim() != order_id);
        to_sequence.push(order_id.clone());

        let raw_material_assignments = self
            .store
            .raw_material_assignments()
            .await?
            .into_iter()
            .filter(|assignment| {
                assignment.order_id.trim() == order_id
                    && queue_state::apparatus_titles_match(&assignment.apparatus, from)
            })
            .map(|mut assignment| {
                assignment.apparatus = to_key.clone();
                assignment
            })
            .collect::<Vec<_>>();
        let material_barcodes = raw_material_assignments
            .iter()
            .map(|assignment| assignment.barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        let record = ProductionMapApparatusTransferRecord {
            transfer_id,
            idempotency_key: idempotency_key.to_string(),
            order_id: order_id.clone(),
            from_apparatus: from_key,
            to_apparatus: to_key,
            reason: reason.to_string(),
            actor,
            session_id: session.session_id.clone(),
            progress_batch_id: progress_batch.batch_id.clone(),
            material_barcodes,
            map: updated_map.clone(),
            session: session.clone(),
            progress_batch: progress_batch.clone(),
            progress_batch_updates: progress_batch_updates.clone(),
            created_at_unix: now,
        };
        let record = self
            .store
            .commit_apparatus_transfer(ProductionMapApparatusTransferWrite {
                record,
                updated_map,
                from_sequence,
                to_sequence,
                from_states,
                to_states,
                target_apparatus_id,
                session,
                progress_batch,
                progress_batch_updates,
                raw_material_assignments,
            })
            .await?;
        self.notify_live();
        self.transfer_result(record).await
    }

    async fn ensure_transfer_target_capabilities(
        &self,
        target_apparatus: &str,
    ) -> Result<(), ProductionMapError> {
        let requirements = if pechat::is_flexo_apparatus(target_apparatus) {
            ["print", "pechat", "flexo"].as_slice()
        } else if pechat::is_pechat_apparatus(target_apparatus) {
            ["print", "pechat"].as_slice()
        } else {
            return Ok(());
        };
        let target_id = apparatus_id_for_name(target_apparatus);
        let profiles = self.store.apparatus_capacity_profiles().await?;
        let levels = profiles
            .iter()
            .find(|profile| {
                profile.apparatus_id.eq_ignore_ascii_case(&target_id)
                    || profile.apparatus.eq_ignore_ascii_case(target_apparatus)
            })
            .map(|profile| {
                requirements
                    .iter()
                    .map(|code| profile.capability_level(code))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                let master = apparatus_master_data_for_name(target_apparatus);
                requirements
                    .iter()
                    .map(|code| {
                        master
                            .capability_profiles
                            .iter()
                            .find(|profile| {
                                profile.code.eq_ignore_ascii_case(code)
                                    && profile.is_valid_at(unix_seconds())
                            })
                            .map(|profile| profile.level)
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            });
        if levels.iter().any(|level| *level == 0) {
            return Err(ProductionMapError::CapabilityNotSupported);
        }
        Ok(())
    }

    async fn transfer_result(
        &self,
        record: ProductionMapApparatusTransferRecord,
    ) -> Result<ProductionMapApparatusTransferResult, ProductionMapError> {
        let program = compile_map(&record.map)?;
        Ok(ProductionMapApparatusTransferResult {
            saved: ProductionMapSaved {
                map: record.map.clone(),
                program,
            },
            order_status: self.order_status_detail(&record.order_id).await?,
            transfer: record,
        })
    }

    pub(super) async fn ensure_normal_map_move_is_pending(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<(), ProductionMapError> {
        let order_id = order_id.trim();
        let states = self.store.apparatus_queue_states().await?;
        if states.iter().any(|(stored_apparatus, values)| {
            queue_state::apparatus_titles_match(stored_apparatus, apparatus)
                && values
                    .get(order_id)
                    .and_then(|value| queue_state::ApparatusQueueOrderState::parse(value))
                    .is_some_and(|state| state != queue_state::ApparatusQueueOrderState::Pending)
        }) {
            return Err(ProductionMapError::StartedOrderMoveRequiresTransfer);
        }
        if self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .is_some()
        {
            return Err(ProductionMapError::StartedOrderMoveRequiresTransfer);
        }
        Ok(())
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
