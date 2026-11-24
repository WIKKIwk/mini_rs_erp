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
        let mut effective =
            Self::effective_apparatus_sequences_for_maps(&maps, &sequences, &frozen_order_ids);
        apparatus::filter_unselected_print_orders(
            &maps,
            &self.active_canonical_apparatuses().await?,
            &mut effective,
        );
        Ok(effective)
    }

    pub(crate) fn effective_apparatus_sequences_for_maps(
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        frozen_order_ids: &BTreeSet<String>,
    ) -> BTreeMap<String, Vec<String>> {
        let visible_by_apparatus = visible_order_ids_by_apparatus(maps);
        Self::effective_apparatus_sequences_for_visible(
            &visible_by_apparatus, sequences, frozen_order_ids,
        )
    }

    pub(crate) fn effective_apparatus_sequences_for_visible(
        visible_by_apparatus: &BTreeMap<String, Vec<String>>,
        sequences: &BTreeMap<String, Vec<String>>,
        frozen_order_ids: &BTreeSet<String>,
    ) -> BTreeMap<String, Vec<String>> {
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
        let mut visible = visible_order_ids_by_apparatus(&maps);
        apparatus::filter_unselected_print_orders(
            &maps,
            &self.active_canonical_apparatuses().await?,
            &mut visible,
        );
        Ok(visible)
    }

    pub async fn set_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        self.save_apparatus_sequence(apparatus, order_ids, None)
            .await?;
        Ok(())
    }

    /// Move one order to the closest legal slot, using the current server
    /// queue and the same barriers as a strict sequence update.
    pub async fn reorder_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
        moved_order_id: &str,
    ) -> Result<Vec<String>, ProductionMapError> {
        self.save_apparatus_sequence(apparatus, order_ids, Some(moved_order_id.trim()))
            .await
    }

    async fn save_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
        moved_order_id: Option<&str>,
    ) -> Result<Vec<String>, ProductionMapError> {
        let _guard = self.apparatus_action_guard(apparatus).await;
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
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        let canonical_id = canonical.runtime.apparatus_id.to_string();
        let now = super::super::progress::unix_seconds();
        let preflight_order_ids = self
            .store
            .active_print_preflight_holds()
            .await?
            .into_iter()
            .filter(|hold| {
                hold.is_live_at(now)
                    && queue_state::apparatus_ids_match(&hold.apparatus, &canonical_id)
            })
            .map(|hold| hold.order_id)
            .collect::<Vec<_>>();
        if moved_order_id.is_none() && !preflight_order_ids.is_empty() {
            return Err(ProductionMapError::PrintPreflightActive);
        }
        let visible_sequence = apparatus::selected_order_ids_for_apparatus(&maps, &canonical);
        let visible_order_ids = visible_sequence.iter().cloned().collect::<BTreeSet<_>>();
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
        let current_sequence = sequences
            .get(apparatus)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let empty_states = BTreeMap::new();
        let states = all_states.get(apparatus).unwrap_or(&empty_states);
        let frozen_order_ids = self
            .store
            .order_control_states()
            .await?
            .into_iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id)
            })
            .collect::<BTreeSet<_>>();
        if let Some(moved) = moved_order_id {
            // Legacy mobiles still send their entire visible list. Translate
            // only the drag intent; frozen display rows are not queue members.
            if order_ids.iter().collect::<BTreeSet<_>>().len() != order_ids.len() {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            let context = SequenceMoveState::from_data(
                &canonical,
                &maps,
                current_sequence,
                states,
                &frozen_order_ids,
                &preflight_order_ids.iter().cloned().collect(),
            );
            if context.frozen.contains(moved) {
                return Err(ProductionMapError::QueueReorderFrozen);
            }
            let requested = order_ids
                .iter()
                .filter(|id| !context.frozen.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            let index = requested
                .iter()
                .position(|id| id == moved)
                .ok_or(ProductionMapError::QueueActionNotAllowed)?;
            let before = requested.get(index + 1).cloned();
            let after = if before.is_none() && index > 0 {
                requested.get(index - 1).cloned()
            } else {
                None
            };
            let command = SequenceMove {
                apparatus: apparatus.into(),
                order_id: moved.into(),
                before_order_id: before,
                after_order_id: after,
                expected_version: context.version(),
                idempotency_key: format!("legacy-{:032x}", rand::random::<u128>()),
            };
            let mut result = self
                .store
                .move_apparatus_sequence(
                    &canonical,
                    &command,
                    &QueueActionActor {
                        role: "system".into(),
                        ref_: "legacy-queue-reorder".into(),
                        display_name: String::new(),
                    },
                )
                .await?;
            self.notify_live();
            // Old clients require every submitted ID in the response. This is
            // a display-only compatibility projection, NEVER persisted.
            result.order_ids.extend(
                order_ids
                    .into_iter()
                    .filter(|id| context.frozen.contains(id)),
            );
            return Ok(result.order_ids);
        }
        let mut barrier_states = states.clone();
        // Running and passed color trials reserve their order, not the entire
        // waiting queue. Never let a moved order cross that reservation.
        for order_id in preflight_order_ids {
            barrier_states.insert(order_id, "print_preflight".to_string());
        }
        validate_active_sequence_barrier(
            current_sequence,
            &order_ids,
            &barrier_states,
            &frozen_order_ids,
        )?;
        self.store
            .put_apparatus_sequence(apparatus, order_ids.clone())
            .await?;
        self.notify_live();
        Ok(order_ids)
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
        // Personal stage audit remains actor-scoped. Worker queue views use
        // assigned-apparatus history so the next shift can continue the work.
        // Global order closure belongs to `fully_completed_orders`, not either
        // of these per-apparatus histories.
        self.store
            .completed_queue_orders_for_actor(actor_ref, limit)
            .await
    }

    /// Stage history shared by workers assigned to these exact apparatuses.
    /// Assignment authorization belongs to the caller; an empty scope stays empty.
    pub async fn completed_queue_orders_for_apparatuses(
        &self,
        apparatuses: &[String],
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        let scope = apparatuses.iter().map(|id| {
            crate::core::apparatus_standard::ApparatusId::new(id.trim().to_string())
                .map(|id| id.to_string())
                .map_err(|_| ProductionMapError::QueueActionNotAllowed)
        }).collect::<Result<BTreeSet<_>, _>>()?;
        if scope.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        self.store.completed_queue_orders_for_apparatuses(
            &scope.into_iter().collect::<Vec<_>>(), limit,
        ).await
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

    /// Limit the lookup to running sessions when an apparatus scope is supplied.
    pub async fn active_order_run_sessions_for_worker(
        &self,
        worker_refs: &[String],
        running_on_apparatus: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        self.store
            .active_order_run_sessions_for_worker(worker_refs, running_on_apparatus, limit)
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
        let order_ids = [order_id.to_string()];
        let (lifecycles, order_control) = tokio::try_join!(
            self.store.production_order_lifecycles(&order_ids),
            self.store.order_control_by_id(order_id),
        )?;
        let record = lifecycles
            .get(order_id)
            .ok_or(ProductionMapError::MapNotFound)?;
        let mut status = ProductionOrderStatusDetail::from_persisted_projection(record);
        if order_control
            .as_ref()
            .is_some_and(|control| control.state == OrderControlState::Frozen)
        {
            status.force_frozen();
        }
        Ok(status)
    }

    pub async fn order_status_details(
        &self,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let (maps, order_controls) = tokio::join!(self.maps(), self.store.order_control_states());
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
        Self::order_status_details_from_lifecycles(&order_ids, &lifecycles, order_controls)
    }

    pub(in crate::core::production_map) fn order_status_details_from_lifecycles(
        order_ids: &[String],
        lifecycles: &BTreeMap<String, ProductionOrderLifecycleRecord>,
        order_controls: &OrderControlMap,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let mut statuses = BTreeMap::new();
        for order_id in order_ids {
            // A map row committed without its lifecycle projection yet (or removed
            // concurrently) must not fail statuses for every other order.
            let Some(record) = lifecycles.get(order_id) else {
                tracing::warn!(
                    order_id = %order_id,
                    "skipping order status without lifecycle projection"
                );
                continue;
            };
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
        self.queue_action_controls_in_scope(None).await
    }
}
