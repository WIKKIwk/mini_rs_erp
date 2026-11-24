impl ProductionMapService {
    pub async fn next_order_number(&self) -> Result<String, ProductionMapError> {
        self.store.next_order_number().await
    }

    pub async fn maps(&self) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        Ok(compile_saved_maps(self.store.maps().await?))
    }

    pub async fn production_order_lifecycle(
        &self,
        order_id: &str,
    ) -> Result<ProductionOrderLifecycleRecord, ProductionMapError> {
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        self.store
            .production_order_lifecycles(&[order_id.to_string()])
            .await?
            .remove(order_id)
            .ok_or(ProductionMapError::MapNotFound)
    }

    pub async fn fully_completed_orders(
        &self,
        limit: usize,
    ) -> Result<Vec<FullyCompletedProductionOrder>, ProductionMapError> {
        let mut orders = self.closed_orders_read(None, true).await?;
        orders.truncate(limit.clamp(1, 500));
        Ok(orders)
    }

    /// The archive index omits progress batches. Details are read for one ID
    /// when expanded, instead of loading every closed order's WIP on entry.
    pub async fn closed_order_summaries(
        &self,
    ) -> Result<Vec<FullyCompletedProductionOrder>, ProductionMapError> {
        self.closed_orders_read(None, false).await
    }

    pub async fn closed_order_detail(
        &self,
        order_id: &str,
    ) -> Result<Option<FullyCompletedProductionOrder>, ProductionMapError> {
        let order_id = order_id.trim();
        if !order_id.starts_with("zakaz-") {
            return Err(ProductionMapError::MissingId);
        }
        Ok(self.closed_orders_read(Some(order_id), true).await?.into_iter().next())
    }

    async fn closed_orders_read(
        &self,
        only_order: Option<&str>,
        include_progress: bool,
    ) -> Result<Vec<FullyCompletedProductionOrder>, ProductionMapError> {
        let terminal = [
            ProductionOrderLifecycleStatus::ProductionCompleted,
            ProductionOrderLifecycleStatus::Closed,
            ProductionOrderLifecycleStatus::Cancelled,
        ];
        let maps = if let Some(id) = only_order {
            let lifecycle = self.store.production_order_lifecycles(&[id.to_string()]).await?;
            if !lifecycle.get(id).is_some_and(|record| terminal.contains(&record.status)) {
                return Ok(Vec::new());
            }
            self.store.map_by_id(id).await?.into_iter().collect()
        } else {
            self.store.maps_by_lifecycle_statuses(&terminal).await?
        };
        let mut candidates = Vec::new();
        for map in maps {
            let order_id = map.id.trim();
            if order_id.is_empty() || !order_id.starts_with("zakaz-") {
                continue;
            }
            if let Err(error) = compile_map(&map) {
                tracing::warn!(
                    map_id = %map.id,
                    error = ?error,
                    "skipping invalid production map in closed-order evaluation"
                );
                continue;
            }
            let Some(required_apparatus) = required_apparatus_for_closed_order(&map) else {
                tracing::warn!(
                    map_id = %map.id,
                    "skipping production map with invalid closed-order apparatus identity"
                );
                continue;
            };
            if required_apparatus.is_empty() {
                continue;
            }
            candidates.push((map, required_apparatus));
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let order_ids = candidates
            .iter()
            .map(|(map, _)| map.id.trim().to_string())
            .collect::<Vec<_>>();
        let (
            logs_by_order,
            progress_batches_by_order,
            sessions_by_order,
            transfers,
            freeze_requests,
        ) = tokio::join!(
            self.store.queue_action_logs_for_orders(&order_ids),
            async {
                if include_progress {
                    self.store.progress_batches_for_orders(&order_ids).await
                } else {
                    Ok(BTreeMap::new())
                }
            },
            self.store.order_run_sessions_for_orders(&order_ids),
            self.store.apparatus_transfers_for_audit(),
            self.store.order_freeze_requests_for_audit(),
        );
        let mut logs_by_order = logs_by_order?;
        let mut progress_batches_by_order = progress_batches_by_order?;
        let mut sessions_by_order = sessions_by_order?;
        let transfers = match transfers {
            Ok(transfers) => transfers,
            Err(error) => {
                tracing::warn!(
                    error = ?error,
                    "closed order apparatus transfer audit is unavailable"
                );
                Vec::new()
            }
        };
        let freeze_requests = match freeze_requests {
            Ok(freeze_requests) => freeze_requests,
            Err(error) => {
                tracing::warn!(
                    error = ?error,
                    "closed order freeze audit is unavailable"
                );
                Vec::new()
            }
        };
        let mut closed = Vec::new();
        let controls = self.store.order_control_states().await?;
        for (map, required_apparatus) in candidates {
            let order_id = map.id.trim().to_string();
            let early_close = controls.get(&order_id)
                .and_then(|control| control.early_close.clone())
                .filter(|close| close.closed_at_unix.is_some());
            let mut logs = logs_by_order.remove(&order_id).unwrap_or_default();
            let closed_event = latest_required_complete_event(&map, &logs, &required_apparatus);
            let sessions = sessions_by_order.remove(&order_id).unwrap_or_default();
            // A late upstream close may make the order terminal, but must not
            // replace the final operation's last reported execution in history.
            let last_report = sessions.iter()
                .filter(|s| s.status == OrderRunStatus::Completed && !s.stage_node_id.is_empty())
                .filter(|s| chain::next_work_stages_for_node(&map, &s.stage_node_id).is_empty())
                .filter_map(|s| crate::core::production_map::stage_execution::work_report(s).map(|r| (s, r)))
                .max_by_key(|(_, r)| r.sequence);
            let (completed_at_unix, closed_by_role, closed_by_ref, closed_by_display_name) =
                if let Some(close) = &early_close {
                    (close.closed_at_unix.unwrap(), close.actor.role.clone(),
                        close.actor.ref_.clone(), close.actor.display_name.clone())
                } else if let Some((session, report)) = last_report {
                    (report.submitted_at_unix.max(closed_event.map(|e| e.created_at_unix).unwrap_or(0)),
                        if report.worker_role.is_empty() { session.worker_role.clone() } else { report.worker_role }, report.worker_ref, report.worker_display_name)
                } else if let Some(event) = closed_event {
                    (event.created_at_unix, event.actor_role.clone(), event.actor_ref.clone(), event.actor_display_name.clone())
                } else { continue; };
            for transfer in transfers
                .iter()
                .filter(|transfer| transfer.order_id.trim().eq_ignore_ascii_case(&order_id))
            {
                logs.push(ProductionOrderLogEntry {
                    event_id: transfer.transfer_id.clone(),
                    apparatus: transfer.to_apparatus.clone(),
                    order_id: transfer.order_id.clone(),
                    stage_node_id: String::new(),
                    action: queue_state::ApparatusQueueAction::Pause,
                    from_state: transfer.queue_state,
                    to_state: transfer.queue_state,
                    actor_role: transfer.actor.role.clone(),
                    actor_ref: transfer.actor.ref_.clone(),
                    actor_display_name: transfer.actor.display_name.clone(),
                    created_at_unix: transfer.created_at_unix,
                    completed_with_issue: false,
                    issue_note: String::new(),
                    transfer: Some(ProductionOrderTransferDetails {
                        transfer_id: transfer.transfer_id.clone(),
                        from_apparatus: transfer.from_apparatus.clone(),
                        to_apparatus: transfer.to_apparatus.clone(),
                        reason: transfer.reason.clone(),
                        session_id: transfer.session_id.clone(),
                        progress_batch_id: transfer.progress_batch_id.clone(),
                        material_barcodes: transfer.material_barcodes.clone(),
                    }),
                    freeze: None,
                    zero_output_waste: None,
                });
            }
            for freeze in freeze_requests
                .iter()
                .filter(|freeze| freeze.order_id.trim().eq_ignore_ascii_case(&order_id))
            {
                logs.push(closed_order_freeze_log_entry(freeze));
            }
            logs.sort_by(|left, right| {
                left.created_at_unix
                    .cmp(&right.created_at_unix)
                    .then_with(|| closed_order_log_rank(left).cmp(&closed_order_log_rank(right)))
                    .then_with(|| left.event_id.cmp(&right.event_id))
            });
            let progress_batches = progress_batches_by_order
                .remove(&order_id)
                .unwrap_or_default();
            closed.push(FullyCompletedProductionOrder {
                early_close,
                order_id,
                order_number: map.order_number.trim().to_string(),
                title: map.title.trim().to_string(),
                product_code: map.product_code.trim().to_string(),
                completed_at_unix,
                closed_by_role,
                closed_by_ref,
                closed_by_display_name,
                logs,
                progress_batches,
            });
        }
        closed.sort_by(|left, right| {
            right
                .completed_at_unix
                .cmp(&left.completed_at_unix)
                .then_with(|| left.order_id.cmp(&right.order_id))
        });
        Ok(closed)
    }

    pub async fn map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapSaved>, ProductionMapError> {
        let map_id = map_id.trim();
        if map_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let Some(mut map) = self.raw_map(map_id).await? else {
            return Ok(None);
        };
        if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
            map.code = map.order_number.trim().to_string();
        }
        let program = compile_map(&map)?;
        Ok(Some(ProductionMapSaved { map, program }))
    }

    async fn reject_started_stage_changes(
        &self,
        next: &ProductionMapDefinition,
        allow_preflight_edit: bool,
    ) -> Result<(), ProductionMapError> {
        let Some(previous) = self.raw_map(&next.id).await? else {
            return Ok(());
        };
        let order_id = previous.id.trim();
        for control in self.store.stage_order_control_states().await? {
            if control.order_id != order_id || control.state == OrderControlState::Active { continue; }
            let apparatus = control.freeze_request.as_ref().map(|r| r.target_apparatus.as_str()).unwrap_or_default();
            if !chain::linear_work_stages(next).iter().any(|stage|
                stage.node_id == control.stage_node_id && stage.apparatus_id.as_deref() == Some(apparatus)) {
                return Err(ProductionMapError::StartedProductionMapStageLocked);
            }
        }
        let mut started_apparatus = BTreeSet::<ApparatusId>::new();

        for (apparatus, states) in self.store.apparatus_queue_states().await? {
            let Some(state) = states.get(order_id) else {
                continue;
            };
            let state = queue_state::ApparatusQueueOrderState::parse(state);
            if state != Some(queue_state::ApparatusQueueOrderState::Pending)
                && !(allow_preflight_edit
                    && state == Some(queue_state::ApparatusQueueOrderState::PrintPreflight))
            {
                insert_apparatus_id(&mut started_apparatus, &apparatus);
            }
        }
        for session in self.store.order_run_sessions_for_order(order_id).await? {
            insert_apparatus_id(&mut started_apparatus, &session.apparatus);
        }
        let batches = self.store.progress_batches_for_order(order_id).await?;
        super::wip_route::validate_map_wip_routes(&previous, next, &batches)?;
        for batch in batches {
            insert_apparatus_id(&mut started_apparatus, &batch.apparatus);
            insert_apparatus_id(&mut started_apparatus, &batch.current_apparatus);
            insert_apparatus_id(&mut started_apparatus, &batch.used_by_apparatus);
            insert_apparatus_id(&mut started_apparatus, &batch.processed_by_apparatus);
        }
        if started_apparatus.is_empty() {
            return Ok(());
        }
        if previous.products != next.products {
            return Err(ProductionMapError::StartedProductionMapStageLocked);
        }

        let mut locked_node_ids = previous
            .nodes
            .iter()
            .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .filter(|node| {
                node.canonical_apparatus_id()
                    .is_some_and(|apparatus_id| started_apparatus.contains(&apparatus_id))
            })
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let locked_group_ids = previous
            .nodes
            .iter()
            .filter(|node| locked_node_ids.contains(&node.id))
            .map(|node| node.alternative_group_id.trim())
            .filter(|group_id| !group_id.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        if !locked_group_ids.is_empty() {
            locked_node_ids.extend(
                previous
                    .nodes
                    .iter()
                    .filter(|node| locked_group_ids.contains(node.alternative_group_id.trim()))
                    .map(|node| node.id.clone()),
            );
        }
        if locked_node_ids.is_empty() {
            return Ok(());
        }

        let locked_node_changed = locked_node_ids.iter().any(|node_id| {
            let previous_node = previous.nodes.iter().find(|node| node.id == *node_id);
            let next_node = next.nodes.iter().find(|node| node.id == *node_id);
            previous_node != next_node
        });
        if locked_node_changed {
            return Err(ProductionMapError::StartedProductionMapStageLocked);
        }

        // An incoming edge is part of the already executed route. Outgoing
        // edges may still be rewired so admins can replace future stages.
        let previous_incoming = previous
            .edges
            .iter()
            .filter(|edge| locked_node_ids.contains(&edge.to))
            .collect::<Vec<_>>();
        let next_incoming = next
            .edges
            .iter()
            .filter(|edge| locked_node_ids.contains(&edge.to))
            .collect::<Vec<_>>();
        let incoming_edges_changed = previous_incoming.len() != next_incoming.len()
            || previous_incoming
                .iter()
                .any(|edge| !next_incoming.contains(edge));
        if incoming_edges_changed {
            return Err(ProductionMapError::StartedProductionMapStageLocked);
        }
        Ok(())
    }

    pub(crate) async fn set_order_print_side(
        &self, order_id: &str, assigned: &[String], standard: bool, previous: bool,
    ) -> Result<(), ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        self.store.set_order_print_side(order_id, assigned, standard, previous).await?;
        self.notify_live();
        Ok(())
    }

    pub async fn upsert_map(
        &self,
        map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        self.upsert_map_under_queue_guard(map).await
    }

    pub(crate) async fn upsert_map_under_queue_guard(
        &self,
        map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        if !map.id.starts_with("template-")
            && let Some(previous) = self.store.map_by_id(map.id.trim()).await?
            && !previous.order_number.trim().is_empty()
            && crate::core::order_edit::calculation_map_changed(&previous, &map)
        {
            return Err(ProductionMapError::OpenedOrderCalculationLocked);
        }
        let saved = self.prepare_map_for_save(map).await?;
        self.store.put_map(saved.map.clone()).await?;
        self.notify_live();
        Ok(saved)
    }

    /// Validation shared by ordinary saves and atomic Telegram order completion.
    pub(crate) async fn prepare_map_for_save(
        &self,
        map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        self.prepare_map(map, false).await
    }

    /// The dedicated order editor rechecks work/material eligibility under
    /// database locks and invalidates colour approval when print inputs change.
    pub(crate) async fn prepare_map_for_order_edit(
        &self,
        map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        self.prepare_map(map, true).await
    }

    async fn prepare_map(
        &self,
        mut map: ProductionMapDefinition,
        allow_preflight_edit: bool,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        if let Some(previous) = self.store.map_by_id(map.id.trim()).await? {
            map.duplicate_rezka_labels = map.duplicate_rezka_labels.or(previous.duplicate_rezka_labels);
            map.include_bobina_in_net = map.include_bobina_in_net.or(previous.include_bobina_in_net);
            map.standard_print_side = previous.standard_print_side;
        }
        normalize_map(&mut map);
        compile_map(&map)?;
        self.validate_and_normalize_map_apparatus(&mut map).await?;
        let program = compile_map(&map)?;
        self.reject_started_stage_changes(&map, allow_preflight_edit).await?;
        Ok(ProductionMapSaved { map, program })
    }

    #[allow(dead_code)]
    pub async fn upsert_maps_batch(
        &self,
        maps: Vec<ProductionMapDefinition>,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let mut normalized = Vec::with_capacity(maps.len());
        let mut saved = Vec::with_capacity(maps.len());
        for mut map in maps {
            if let Some(previous) = self.store.map_by_id(map.id.trim()).await? {
                map.duplicate_rezka_labels = map.duplicate_rezka_labels.or(previous.duplicate_rezka_labels);
                map.include_bobina_in_net = map.include_bobina_in_net.or(previous.include_bobina_in_net);
                map.standard_print_side = previous.standard_print_side;
            }
            normalize_map(&mut map);
            compile_map(&map)?;
            self.validate_and_normalize_map_apparatus(&mut map).await?;
            let program = compile_map(&map)?;
            self.reject_started_stage_changes(&map, false).await?;
            saved.push(ProductionMapSaved {
                map: map.clone(),
                program,
            });
            normalized.push(map);
        }
        self.store.put_maps_batch(&normalized).await?;
        self.notify_live();
        Ok(saved)
    }
}
