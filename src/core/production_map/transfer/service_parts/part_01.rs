
impl ProductionMapService {
    pub async fn transfer_apparatus_order(
        &self,
        input: ProductionMapApparatusTransferRequest,
        actor: QueueActionActor,
    ) -> Result<ProductionMapApparatusTransferResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = input.order_id.trim().to_ascii_lowercase();
        let reason = input.reason.trim();
        let idempotency_key = input.idempotency_key.trim();
        let Some(from_id) = canonical_transfer_id(&input.from_apparatus) else {
            return Err(ProductionMapError::MoveNotAllowed);
        };
        let Some(to_id) = canonical_transfer_id(&input.to_apparatus) else {
            return Err(ProductionMapError::MoveNotAllowed);
        };
        if order_id.is_empty() || from_id == to_id {
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
            if !transfer_record_matches_request(&record, &order_id, &from_id, &to_id) {
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
        let order_controls = self.store.order_control_states().await?;
        if let Some(control) = order_controls.values().find(|control| {
            control.order_id.trim().eq_ignore_ascii_case(&order_id)
        }) {
            match control.state {
                OrderControlState::Active => {}
                OrderControlState::FreezeRequested => {
                    return Err(ProductionMapError::OrderFreezeRequested);
                }
                OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
            }
        }
        let from_apparatus_id =
            ApparatusId::new(from_id.clone()).map_err(|_| ProductionMapError::MoveNotAllowed)?;
        let to_apparatus_id =
            ApparatusId::new(to_id.clone()).map_err(|_| ProductionMapError::MoveNotAllowed)?;
        let source = self.resolve_canonical_apparatus(&from_apparatus_id).await?;
        let target = self.resolve_canonical_apparatus(&to_apparatus_id).await?;
        if !crate::core::production_map::pechat::reroute_order_compatible(
            &source,
            &target,
            map.roll_count,
            map.width_mm,
        ) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let target_display = target.runtime.display.display_name.clone();
        if !transfer_move_allowed_by_id(&map, &from_id, &to_id) {
            return Err(ProductionMapError::MoveNotAllowed);
        }

        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let mut from_states = all_states.get(&from_id).cloned().unwrap_or_default();
        let mut to_states = all_states.get(&to_id).cloned().unwrap_or_default();
        let source_state = from_states
            .get(&order_id)
            .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state));
        match source_state {
            Some(queue_state::ApparatusQueueOrderState::Paused) => {}
            Some(queue_state::ApparatusQueueOrderState::Frozen) => {
                return Err(ProductionMapError::OrderFrozen);
            }
            Some(queue_state::ApparatusQueueOrderState::Completed) => {
                return Err(ProductionMapError::OrderAlreadyCompleted);
            }
            _ => return Err(ProductionMapError::ApparatusTransferOrderNotPaused),
        }
        if to_states.contains_key(&order_id) {
            return Err(ProductionMapError::ApparatusTransferTargetConflict);
        }
        if self
            .store
            .active_order_run_session(&to_id, &order_id)
            .await?
            .is_some()
        {
            return Err(ProductionMapError::ApparatusTransferTargetConflict);
        }

        let source_session = self
            .store
            .active_order_run_session(&from_id, &order_id)
            .await?
            .ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
        if source_session.status != OrderRunStatus::Paused
            || source_session.order_id.trim() != order_id
            || source_session.apparatus.trim() != from_id
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
                    && batch.apparatus.trim() == from_id
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
            "from_apparatus": from_id,
            "to_apparatus": to_id,
            "to_apparatus_display": target_display,
            "reason": reason,
            "actor": actor,
            "created_at_unix": now,
        });

        let mut session = source_session;
        session.apparatus = to_id.clone();
        session.updated_at_unix = now;
        if !session.payload_json.is_object() {
            session.payload_json = serde_json::json!({});
        }
        session.payload_json["last_apparatus_transfer"] = transfer_payload.clone();

        progress_batch.apparatus = to_id.clone();
        progress_batch.current_apparatus = to_id.clone();
        progress_batch.current_apparatus_key = to_id.clone();
        progress_batch.current_location = if target_display.is_empty() {
            String::new()
        } else {
            format!("{target_display} chiqim")
        };
        if apparatus_ids_match(&progress_batch.used_by_apparatus, &from_id) {
            progress_batch.used_by_apparatus = to_id.clone();
        }
        if apparatus_ids_match(&progress_batch.processed_by_apparatus, &from_id) {
            progress_batch.processed_by_apparatus = to_id.clone();
        }
        if !progress_batch.payload_json.is_object() {
            progress_batch.payload_json = serde_json::json!({});
        }
        progress_batch.payload_json["last_apparatus_transfer"] = transfer_payload;
        progress_batch.refresh_status_detail();

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
            parent_batch.next_apparatus = to_id.clone();
            parent_batch.refresh_status_detail();
            progress_batch_updates.push(parent_batch);
        }

        let mut updated_map = map;
        if !reassign_alternative_apparatus_assignment_by_id(
            &mut updated_map,
            &from_id,
            &to_id,
            &target_display,
        ) && !reassign_apparatus_nodes_by_id(&mut updated_map, &from_id, &to_id, &target_display)
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
        let mut from_sequence = sequences.get(&from_id).cloned().unwrap_or_default();
        from_sequence.retain(|id| id.trim() != order_id);
        let mut to_sequence = sequences.get(&to_id).cloned().unwrap_or_default();
        to_sequence.retain(|id| id.trim() != order_id);
        to_sequence.push(order_id.clone());

        let raw_material_assignments = self
            .store
            .raw_material_assignments()
            .await?
            .into_iter()
            .filter(|assignment| {
                assignment.order_id.trim() == order_id
                    && assignment.apparatus_id.as_str() == from_id
            })
            .map(|mut assignment| {
                assignment.apparatus_id = ApparatusId::new(to_id.clone())
                    .expect("validated transfer target apparatus id");
                assignment.apparatus = target_display.clone();
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
            from_apparatus: from_id.clone(),
            to_apparatus: to_id.clone(),
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
                target_apparatus_id: to_id.clone(),
                session,
                progress_batch,
                progress_batch_updates,
                raw_material_assignments,
            })
            .await?;
        if !transfer_record_matches_request(&record, &order_id, &from_id, &to_id) {
            return Err(ProductionMapError::ApparatusTransferIdempotencyConflict);
        }
        self.notify_live();
        self.transfer_result(record).await
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
            apparatus_ids_match(stored_apparatus, apparatus)
                && values
                    .get(order_id)
                    .and_then(|value| queue_state::ApparatusQueueOrderState::parse(value))
                    .is_some_and(|state| state != queue_state::ApparatusQueueOrderState::Pending)
        }) {
            return Err(ProductionMapError::StartedOrderMoveRequiresTransfer);
        }
        if self
            .store
            .active_order_run_session(apparatus.trim(), order_id)
            .await?
            .is_some()
        {
            return Err(ProductionMapError::StartedOrderMoveRequiresTransfer);
        }
        Ok(())
    }
}

fn transfer_record_matches_request(
    record: &ProductionMapApparatusTransferRecord,
    order_id: &str,
    from_id: &str,
    to_id: &str,
) -> bool {
    record.order_id.trim() == order_id
        && record.from_apparatus.trim() == from_id
        && record.to_apparatus.trim() == to_id
}

fn canonical_transfer_id(value: &str) -> Option<String> {
    ApparatusId::new(value.trim().to_string())
        .ok()
        .map(|id| id.to_string())
}

fn apparatus_ids_match(left: &str, right: &str) -> bool {
    canonical_transfer_id(left)
        .is_some_and(|left| canonical_transfer_id(right).is_some_and(|right| left == right))
}

fn effective_transfer_apparatus_id(node: &ProductionMapNode) -> &str {
    let assigned = node.alternative_assigned_apparatus_id.trim();
    if assigned.is_empty() {
        node.apparatus_id.trim()
    } else {
        assigned
    }
}

fn transfer_move_allowed_by_id(map: &ProductionMapDefinition, from_id: &str, to_id: &str) -> bool {
    let (Some(from_id), Some(to_id)) =
        (canonical_transfer_id(from_id), canonical_transfer_id(to_id))
    else {
        return false;
    };
    if from_id == to_id {
        return false;
    }
    let source_nodes = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && effective_transfer_apparatus_id(node) == from_id
        })
        .collect::<Vec<_>>();
    if source_nodes.is_empty() {
        return false;
    }
    let source_groups = source_nodes
        .iter()
        .map(|node| node.alternative_group_id.trim())
        .filter(|group_id| !group_id.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if source_groups.is_empty() {
        return true;
    }
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && source_groups.contains(node.alternative_group_id.trim())
            && effective_transfer_apparatus_id(node) == to_id
    })
}

fn reassign_alternative_apparatus_assignment_by_id(
    map: &mut ProductionMapDefinition,
    from_id: &str,
    to_id: &str,
    target_display: &str,
) -> bool {
    let groups = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && effective_transfer_apparatus_id(node) == from_id
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    if groups.is_empty()
        || !map.nodes.iter().any(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && groups.contains(node.alternative_group_id.trim())
                && node.apparatus_id.trim() == to_id
        })
    {
        return false;
    }
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_apparatus_id = to_id.to_string();
            node.alternative_assigned_title = target_display.to_string();
        }
    }
    true
}
