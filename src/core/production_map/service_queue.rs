use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::apparatus::visible_order_ids_for_apparatus;
use super::progress::{
    effective_apparatus_queue_policy, effective_apparatus_queue_policy_record,
    queue_action_event_id,
};

impl ProductionMapService {
    pub async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        self.store.apparatus_sequences().await
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
            .collect();
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
        if matches!(action, queue_state::ApparatusQueueAction::Start)
            && !chain::order_ready_for_station(
                order_map,
                order_id,
                apparatus,
                &all_states,
                &known_keys,
            )
        {
            return Err(ProductionMapError::PreviousStageNotCompleted);
        }
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
            ApparatusQueuePolicy::StrictSequence => {
                queue_state::apply_queue_action(&sequence, &mut parsed, order_id, action)?;
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
                    "Laminatsiya tugatishda ikkala qavat qoldig'i yozildi. Bosmadan ortgan rulon: {print_leftover}. Plyonkadan ortgan rulon: {film_leftover}. Jami atxot: {total_waste}. Tayyor mahsulot: {finished_kg} kg, {finished_meter} m."
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
