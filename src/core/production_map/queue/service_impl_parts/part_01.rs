impl ProductionMapService {
    pub async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        self.store.apparatus_sequences().await
    }

    pub async fn effective_apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let (maps, sequences, order_controls) = tokio::join!(
            self.store.maps(),
            self.store.apparatus_sequences(),
            self.store.order_control_states(),
        );
        let maps = maps?;
        let sequences = sequences?;
        let frozen_order_ids = order_controls?
            .into_iter()
            .filter_map(|(id, control)| (control.state == OrderControlState::Frozen).then_some(id))
            .collect::<BTreeSet<_>>();
        Ok(Self::effective_apparatus_sequences_for_maps(
            &maps,
            &sequences,
            &frozen_order_ids,
        ))
    }

    pub(in crate::core::production_map) fn effective_apparatus_sequences_for_maps(
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        frozen_order_ids: &BTreeSet<String>,
    ) -> BTreeMap<String, Vec<String>> {
        let visible_by_apparatus = visible_order_ids_by_apparatus(maps);
        let apparatuses = sequences
            .keys()
            .chain(visible_by_apparatus.keys())
            .map(String::as_str)
            .filter(|key| queue_state::is_canonical_apparatus_id(key))
            .collect::<BTreeSet<_>>();
        apparatuses
            .into_iter()
            .map(|apparatus| {
                let stored_sequence = sequences
                    .get(apparatus)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let visible_order_ids = visible_by_apparatus
                    .get(apparatus)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let sequence = if visible_order_ids.is_empty() {
                    stored_sequence.to_vec()
                } else {
                    queue_state::effective_apparatus_sequence_excluding(
                        stored_sequence,
                        visible_order_ids,
                        frozen_order_ids,
                    )
                };
                (apparatus.to_string(), sequence)
            })
            .collect()
    }

    pub async fn visible_order_ids_by_apparatus(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let maps = self.store.maps().await?;
        Ok(visible_order_ids_by_apparatus(&maps))
    }

    pub async fn set_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let apparatus = apparatus.trim();
        if !queue_state::is_canonical_apparatus_id(apparatus) {
            return Err(ProductionMapError::MissingId);
        }
        let order_ids = order_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        let maps = self.store.maps().await?;
        let known_order_ids = maps
            .iter()
            .map(|map| map.id.trim())
            .filter(|id| !id.is_empty())
            .collect::<BTreeSet<_>>();
        let visible_order_ids = visible_order_ids_for_apparatus(&maps, apparatus)
            .into_iter()
            .collect::<BTreeSet<_>>();
        for order_id in &order_ids {
            if !known_order_ids.contains(order_id.as_str()) {
                return Err(ProductionMapError::QueueSequenceOrderNotFound(
                    order_id.clone(),
                ));
            }
            if !visible_order_ids.contains(order_id) {
                return Err(ProductionMapError::QueueSequenceApparatusMismatch(
                    order_id.clone(),
                ));
            }
        }
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .map(|key| key.as_str())
            .filter(|key| !queue_state::apparatus_search_key(key).is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let current_sequence = sequences
            .get(&storage_key)
            .or_else(|| sequences.get(apparatus))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let empty_states = BTreeMap::new();
        let states = all_states
            .get(&storage_key)
            .or_else(|| all_states.get(apparatus))
            .unwrap_or(&empty_states);
        let frozen_order_ids = self
            .store
            .order_control_states()
            .await?
            .into_iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id)
            })
            .collect::<BTreeSet<_>>();
        validate_active_sequence_barrier(
            current_sequence,
            &order_ids,
            states,
            &frozen_order_ids,
        )?;
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
        // This endpoint is the actor-scoped stage history consumed by the
        // worker "Tugatish" tab. Global order closure belongs to
        // `fully_completed_orders`, which is exposed separately through the
        // closed-orders endpoint. Do not project the global closure invariant
        // onto this worker history.
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
        let mut logs_by_order = self
            .store
            .queue_action_logs_for_orders(std::slice::from_ref(&order_id))
            .await?;
        Ok(logs_by_order.remove(&order_id).unwrap_or_default())
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
        let order_ids = vec![order_id.to_string()];
        let (
            all_queue_states,
            progress_batches,
            run_sessions,
            logs_by_order,
            lifecycles,
            order_controls,
        ) = tokio::join!(
            self.store.apparatus_queue_states(),
            self.store.progress_batches_for_order(order_id),
            self.store.order_run_sessions_for_order(order_id),
            self.store.queue_action_logs_for_orders(&order_ids),
            self.store.production_order_lifecycles(&order_ids),
            self.store.order_control_states(),
        );
        let queue_states = queue_states_for_order(&all_queue_states?, order_id);
        let progress_batches = progress_batches?;
        let run_sessions = run_sessions?;
        let mut logs_by_order = logs_by_order?;
        let logs = logs_by_order.remove(order_id).unwrap_or_default();
        let lifecycles = lifecycles?;
        let order_controls = order_controls?;
        let mut status = ProductionOrderStatusDetail::from_order_flow(
            &progress_batches,
            &run_sessions,
            &queue_states,
            &logs,
        );
        status.lifecycle_status = lifecycles
            .get(order_id)
            .ok_or(ProductionMapError::MapNotFound)?
            .status;
        if order_controls
            .get(order_id)
            .is_some_and(|control| control.state == OrderControlState::Frozen)
        {
            status.force_frozen();
        }
        Ok(status)
    }

    pub async fn order_status_details(
        &self,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let (maps, order_controls) =
            tokio::join!(self.maps(), self.store.order_control_states());
        self.order_status_details_for_snapshot(&maps?, &order_controls?)
            .await
    }

    pub(in crate::core::production_map) async fn order_status_details_for_snapshot(
        &self,
        maps: &[ProductionMapSaved],
        order_controls: &OrderControlMap,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let order_ids = maps
            .iter()
            .filter_map(|saved| {
                let order_id = saved.map.id.trim();
                (!order_id.is_empty()).then(|| order_id.to_string())
            })
            .collect::<Vec<_>>();
        let lifecycles = self.store.production_order_lifecycles(&order_ids).await?;
        Self::order_status_details_from_lifecycles(
            &order_ids,
            &lifecycles,
            order_controls,
        )
    }

    pub(in crate::core::production_map) fn order_status_details_from_lifecycles(
        order_ids: &[String],
        lifecycles: &BTreeMap<String, ProductionOrderLifecycleRecord>,
        order_controls: &OrderControlMap,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let mut statuses = BTreeMap::new();
        for order_id in order_ids {
            let record = lifecycles
                .get(order_id)
                .ok_or(ProductionMapError::StoreFailed)?;
            let mut status = ProductionOrderStatusDetail::from_persisted_projection(record);
            if order_controls
                .get(order_id)
                .is_some_and(|control| control.state == OrderControlState::Frozen)
            {
                status.force_frozen();
            }
            statuses.insert(order_id.clone(), status);
        }
        Ok(statuses)
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
        let mut records = Vec::new();
        for canonical in self.active_canonical_apparatuses().await? {
            records.push(effective_apparatus_queue_policy_record(&canonical));
        }
        Ok(records)
    }

    pub async fn queue_action_controls(
        &self,
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        let (maps, sequences, all_states, order_controls, canonical_apparatuses) = tokio::join!(
            self.store.maps(),
            self.store.apparatus_sequences(),
            self.store.apparatus_queue_states(),
            self.order_control_states(),
            self.active_canonical_apparatuses(),
        );
        let maps = maps?;
        let sequences = sequences?;
        let all_states = all_states?;
        let order_controls = order_controls?;
        let canonical_apparatuses = canonical_apparatuses?;
        self.queue_action_controls_for_snapshot(
            &maps,
            &sequences,
            &all_states,
            &order_controls,
            &canonical_apparatuses,
        )
        .await
    }
}
