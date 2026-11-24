impl ProductionMapService {
    async fn validate_and_normalize_map_apparatus(
        &self,
        map: &mut ProductionMapDefinition,
    ) -> Result<(), ProductionMapError> {
        let apparatus_ids = map
            .nodes
            .iter()
            .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .filter_map(ProductionMapNode::canonical_apparatus_id)
            .collect::<BTreeSet<_>>();
        let mut configurations = std::collections::BTreeMap::new();
        let mut cut_ids = BTreeSet::new();
        for apparatus_id in apparatus_ids {
            let canonical = self.resolve_canonical_apparatus(&apparatus_id).await?;
            if canonical.runtime.execution_profile.operation
                == crate::core::apparatus_standard::ExecutionOperation::Cut
            {
                cut_ids.insert(apparatus_id.clone());
            }
            configurations.insert(apparatus_id, canonical);
        }
        for node in map.nodes.iter().filter(|n| n.kind == ProductionMapNodeKind::Apparatus) {
            let Some(canonical) = node
                .canonical_apparatus_id()
                .and_then(|id| configurations.get(&id))
            else {
                continue;
            };
            let profile = &canonical.runtime.execution_profile;
            if profile.operation
                == crate::core::apparatus_standard::ExecutionOperation::Print
            {
                continue;
            }
            let widths = super::automatic::stage_input_widths(map, &node.id, &cut_ids);
            if widths.iter().any(|width_mm| {
                profile
                    .min_web_width_mm
                    .is_some_and(|minimum| *width_mm < f64::from(minimum))
            }) {
                return Err(ProductionMapError::ApparatusWidthBelowCapability);
            }
            if widths.iter().any(|width_mm| {
                profile
                    .max_web_width_mm
                    .is_some_and(|maximum| *width_mm > f64::from(maximum))
            }) {
                return Err(ProductionMapError::ApparatusWidthExceedsCapability);
            }
        }
        alternatives::normalize_topology_alternatives(map, &configurations);
        Ok(())
    }

    pub async fn raw_map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapDefinition>, ProductionMapError> {
        let map_id = map_id.trim().to_ascii_lowercase();
        self.store.map_by_id(&map_id).await
    }

    pub async fn restore_map(
        &self,
        previous: Option<&ProductionMapDefinition>,
        map_id: &str,
    ) -> Result<(), ProductionMapError> {
        let result = match previous {
            Some(map) => self.store.put_map(map.clone()).await,
            None => self.store.delete_map(map_id).await,
        };
        if result.is_ok() {
            self.notify_live();
        }
        result
    }

    /// Moves multiple orders atomically: either every move succeeds or none
    /// are persisted.
    pub async fn move_apparatus_batch(
        &self,
        input: ProductionMapBatchMoveRequest,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if from.is_empty() || to.is_empty() || from.eq_ignore_ascii_case(to) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let map_ids: Vec<String> = input
            .map_ids
            .iter()
            .map(|id| id.trim().to_ascii_lowercase())
            .filter(|id| !id.is_empty())
            .collect();
        let mut seen = BTreeSet::new();
        let map_ids = map_ids
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .collect::<Vec<_>>();
        if map_ids.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let from_id =
            ApparatusId::new(from.to_string()).map_err(|_| ProductionMapError::MoveNotAllowed)?;
        let to_id =
            ApparatusId::new(to.to_string()).map_err(|_| ProductionMapError::MoveNotAllowed)?;
        let source = self.resolve_canonical_apparatus(&from_id).await?;
        let target = self.resolve_canonical_apparatus(&to_id).await?;
        if !crate::core::production_map::pechat::reroute_compatible(&source, &target) {
            return Err(ProductionMapError::MoveNotAllowed);
        }

        let maps = self.store.maps().await?;
        let mut updated = Vec::with_capacity(map_ids.len());
        for map_id in &map_ids {
            let Some(map) = maps.iter().find(|item| item.id.trim() == map_id).cloned() else {
                return Err(ProductionMapError::MapNotFound);
            };
            if !move_allowed_by_id(&map, &from_id, &to_id)
                || !crate::core::production_map::pechat::reroute_order_compatible(
                    &source,
                    &target,
                    map.roll_count,
                    map.print_compatibility_width_mm(),
                )
            {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            self.ensure_normal_map_move_is_pending(map_id, from_id.as_str())
                .await?;
            let mut next = map;
            if !reassign_alternative_apparatus_assignment_by_id(&mut next, &from_id, &to_id)
                && !reassign_apparatus_nodes_by_id(&mut next, &from_id, &to_id)
            {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            updated.push(next);
        }

        let saved = updated
            .into_iter()
            .map(|map| {
                let program = compile_map(&map)?;
                Ok(ProductionMapSaved { map, program })
            })
            .collect::<Result<Vec<_>, ProductionMapError>>()?;
        let updated = saved
            .iter()
            .map(|saved| saved.map.clone())
            .collect::<Vec<_>>();
        self.persist_pending_move_maps(&maps, &updated, from_id.as_str(), to_id.as_str())
            .await?;
        self.notify_live();
        Ok(saved)
    }

    /// Moves an order between apparatus, validating pechat rules server-side.
    pub async fn move_apparatus(
        &self,
        input: ProductionMapMoveRequest,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let map_id = input.map_id.trim().to_ascii_lowercase();
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if map_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if to.is_empty() || from.eq_ignore_ascii_case(to) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let from_id =
            ApparatusId::new(from.to_string()).map_err(|_| ProductionMapError::MoveNotAllowed)?;
        let to_id =
            ApparatusId::new(to.to_string()).map_err(|_| ProductionMapError::MoveNotAllowed)?;
        let source = self.resolve_canonical_apparatus(&from_id).await?;
        let target = self.resolve_canonical_apparatus(&to_id).await?;
        if !crate::core::production_map::pechat::reroute_compatible(&source, &target) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let maps = self.store.maps().await?;
        let Some(map) = maps.iter().find(|map| map.id.trim() == map_id).cloned() else {
            return Err(ProductionMapError::MapNotFound);
        };
        if !move_allowed_by_id(&map, &from_id, &to_id)
            || !crate::core::production_map::pechat::reroute_order_compatible(
                &source,
                &target,
                map.roll_count,
                map.print_compatibility_width_mm(),
            )
        {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        self.ensure_normal_map_move_is_pending(&map_id, from_id.as_str())
            .await?;
        let mut next = map;
        if !reassign_alternative_apparatus_assignment_by_id(&mut next, &from_id, &to_id)
            && !reassign_apparatus_nodes_by_id(&mut next, &from_id, &to_id)
        {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        if !next.id.starts_with("template-")
            && let Some(previous) = maps.iter().find(|map| map.id == next.id)
            && !previous.order_number.trim().is_empty()
            && crate::core::order_edit::calculation_map_changed(previous, &next)
        {
            return Err(ProductionMapError::OpenedOrderCalculationLocked);
        }
        let saved = self.prepare_map_for_save(next).await?;
        self.persist_pending_move_maps(
            &maps,
            std::slice::from_ref(&saved.map),
            from_id.as_str(),
            to_id.as_str(),
        )
        .await?;
        self.notify_live();
        Ok(saved)
    }

    async fn persist_pending_move_maps(
        &self,
        previous_maps: &[ProductionMapDefinition],
        updated_maps: &[ProductionMapDefinition],
        from: &str,
        to: &str,
    ) -> Result<(), ProductionMapError> {
        let (stored, controls) = tokio::join!(
            self.store.apparatus_sequences(),
            self.store.order_control_states()
        );
        let stored = stored?;
        let frozen = controls?
            .into_iter()
            .filter_map(|(id, control)| (control.state == OrderControlState::Frozen).then_some(id))
            .collect();
        let mut effective =
            Self::effective_apparatus_sequences_for_maps(previous_maps, &stored, &frozen);
        super::apparatus::filter_unselected_print_orders(
            previous_maps,
            &self.active_canonical_apparatuses().await?,
            &mut effective,
        );
        let moved = updated_maps
            .iter()
            .map(|map| map.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut sequences = std::collections::BTreeMap::new();
        let mut expected = std::collections::BTreeMap::new();
        for apparatus in [from, to] {
            expected.insert(
                apparatus.to_string(),
                stored.get(apparatus).cloned().unwrap_or_default(),
            );
            let mut sequence = effective.remove(apparatus).unwrap_or_default();
            sequence.retain(|id| !moved.contains(id.as_str()));
            if apparatus == to {
                sequence.extend(updated_maps.iter().map(|map| map.id.clone()));
            }
            sequences.insert(apparatus.to_string(), sequence);
        }
        self.store
            .put_maps_with_sequences(updated_maps, &expected, &sequences)
            .await
    }

    pub async fn run_map(
        &self,
        input: ProductionMapRunRequest,
    ) -> Result<ProductionMapRunResult, ProductionMapError> {
        if input.order_qty <= 0.0 {
            return Err(ProductionMapError::InvalidOrderQty);
        }
        let map_id = input.map_id.trim().to_ascii_lowercase();
        let product_code = input.product_code.trim();
        let maps = self.store.maps().await?;
        let Some(map) = maps.into_iter().find(|map| {
            (!map_id.is_empty() && map.id == map_id)
                || (!product_code.is_empty() && map.product_code == product_code)
        }) else {
            return Err(ProductionMapError::MapNotFound);
        };
        run_map_with_variables(&map, input.order_qty, input.variables)
    }
}


fn closed_order_log_rank(log: &ProductionOrderLogEntry) -> u8 {
    if let Some(freeze) = &log.freeze {
        return match freeze.status.trim() {
            "pending" => 1,
            "unfrozen" => 4,
            _ => 2,
        };
    }
    if log.transfer.is_some() {
        return 3;
    }
    match log.action {
        queue_state::ApparatusQueueAction::Start => 0,
        queue_state::ApparatusQueueAction::Pause => 1,
        queue_state::ApparatusQueueAction::Freeze => 2,
        queue_state::ApparatusQueueAction::DetachRoll => 1,
        queue_state::ApparatusQueueAction::Resume => 4,
        queue_state::ApparatusQueueAction::Merge => 3,
        queue_state::ApparatusQueueAction::RollComplete => 3,
        queue_state::ApparatusQueueAction::Complete => 5,
    }
}

fn closed_order_freeze_log_entry(freeze: &OrderFreezeAuditRecord) -> ProductionOrderLogEntry {
    let request = &freeze.request;
    let status = request.status.as_str();
    ProductionOrderLogEntry {
        event_id: format!("order-freeze:{}:{}", request.request_id, status),
        apparatus: request.target_apparatus.clone(),
        order_id: freeze.order_id.clone(),
        stage_node_id: String::new(),
        action: queue_state::ApparatusQueueAction::Freeze,
        from_state: queue_state::ApparatusQueueOrderState::InProgress,
        to_state: if status == "frozen" {
            queue_state::ApparatusQueueOrderState::Frozen
        } else {
            queue_state::ApparatusQueueOrderState::InProgress
        },
        actor_role: freeze.actor.role.clone(),
        actor_ref: freeze.actor.ref_.clone(),
        actor_display_name: freeze.actor.display_name.clone(),
        created_at_unix: freeze.occurred_at_unix,
        completed_with_issue: false,
        issue_note: String::new(),
        transfer: None,
        zero_output_waste: None,
        freeze: Some(ProductionOrderFreezeDetails {
            request_id: request.request_id.clone(),
            status: status.to_string(),
            target_session_id: request.target_session_id.clone(),
            target_apparatus: request.target_apparatus.clone(),
            target_worker_role: request.target_worker_role.clone(),
            target_worker_ref: request.target_worker_ref.clone(),
            target_worker_display_name: request.target_worker_display_name.clone(),
            requested_at_unix: request.requested_at_unix,
            transitioned_at_unix: request.transitioned_at_unix,
        }),
    }
}

fn insert_apparatus_id(target: &mut BTreeSet<ApparatusId>, value: &str) {
    if let Ok(apparatus_id) = ApparatusId::new(value.trim().to_string()) {
        target.insert(apparatus_id);
    }
}

fn effective_apparatus_id(node: &ProductionMapNode) -> Option<ApparatusId> {
    node.canonical_apparatus_id()
}

fn move_allowed_by_id(
    map: &ProductionMapDefinition,
    from_id: &ApparatusId,
    to_id: &ApparatusId,
) -> bool {
    if from_id == to_id {
        return false;
    }
    let source_nodes = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && effective_apparatus_id(node).is_some_and(|id| id == *from_id)
        })
        .collect::<Vec<_>>();
    if source_nodes.is_empty() {
        return false;
    }
    let source_groups = source_nodes
        .iter()
        .map(|node| node.alternative_group_id.trim())
        .filter(|group_id| !group_id.is_empty())
        .collect::<BTreeSet<_>>();
    if source_groups.is_empty() {
        return true;
    }
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && source_groups.contains(node.alternative_group_id.trim())
            && node.base_apparatus_id().is_some_and(|id| id == *to_id)
    })
}

fn reassign_alternative_apparatus_assignment_by_id(
    map: &mut ProductionMapDefinition,
    from: &ApparatusId,
    to: &ApparatusId,
) -> bool {
    let groups = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && effective_apparatus_id(node).is_some_and(|id| id == *from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect::<BTreeSet<_>>();
    if groups.is_empty() {
        return false;
    }
    let target_title = map
        .nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && groups.contains(node.alternative_group_id.trim())
                && node.base_apparatus_id().is_some_and(|id| id == *to)
        })
        .map(|node| node.title.trim().to_string());
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && groups.contains(node.alternative_group_id.trim())
        {
            node.set_alternative_assigned_apparatus_id(to);
            if let Some(title) = &target_title {
                node.alternative_assigned_title = title.clone();
            }
            changed = true;
        }
    }
    changed
}

fn reassign_apparatus_nodes_by_id(
    map: &mut ProductionMapDefinition,
    from: &ApparatusId,
    to: &ApparatusId,
) -> bool {
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && node.alternative_group_id.trim().is_empty()
            && effective_apparatus_id(node).is_some_and(|id| id == *from)
        {
            node.set_apparatus_id(to);
            changed = true;
        }
    }
    changed
}
