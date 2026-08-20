use super::*;

use std::collections::BTreeSet;

use super::compiler::{compile_map, normalize_map, run_map_with_variables};
use super::progress::{
    latest_required_complete_event, order_completed_on_apparatus,
    required_apparatus_for_closed_order,
};
use crate::core::apparatus_standard::ApparatusId;

pub(super) fn compile_saved_maps(
    maps: impl IntoIterator<Item = ProductionMapDefinition>,
) -> Vec<ProductionMapSaved> {
    let mut saved = Vec::new();
    for mut map in maps {
        // Legacy maps saved before `code` existed: expose the order
        // number as the code so clients never need a fallback.
        if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
            map.code = map.order_number.trim().to_string();
        }
        match compile_map(&map) {
            Ok(program) => saved.push(ProductionMapSaved { map, program }),
            Err(error) => {
                tracing::warn!(
                    map_id = %map.id,
                    error = ?error,
                    "skipping invalid production map in list response"
                );
            }
        }
    }
    saved
}

impl ProductionMapService {
    pub async fn next_order_number(&self) -> Result<String, ProductionMapError> {
        self.store.next_order_number().await
    }

    pub async fn maps(&self) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        Ok(compile_saved_maps(self.store.maps().await?))
    }

    pub async fn fully_completed_orders(
        &self,
        limit: usize,
    ) -> Result<Vec<FullyCompletedProductionOrder>, ProductionMapError> {
        let maps = self.store.maps().await?;
        let queue_states = self.store.apparatus_queue_states().await?;
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
            if !required_apparatus
                .iter()
                .all(|apparatus| order_completed_on_apparatus(&queue_states, order_id, apparatus))
            {
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
        let logs_by_order = self.store.queue_action_logs_for_orders(&order_ids).await?;
        let transfers = match self.store.apparatus_transfers_for_audit().await {
            Ok(transfers) => transfers,
            Err(error) => {
                tracing::warn!(
                    error = ?error,
                    "closed order apparatus transfer audit is unavailable"
                );
                Vec::new()
            }
        };
        let freeze_requests = match self.store.order_freeze_requests_for_audit().await {
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
        for (map, required_apparatus) in candidates {
            let order_id = map.id.trim().to_string();
            let queue_logs = logs_by_order.get(&order_id).cloned().unwrap_or_default();
            let Some(closed_event) =
                latest_required_complete_event(&queue_logs, &required_apparatus).cloned()
            else {
                continue;
            };
            let mut logs = queue_logs;
            for transfer in transfers
                .iter()
                .filter(|transfer| transfer.order_id.trim().eq_ignore_ascii_case(&order_id))
            {
                logs.push(ProductionOrderLogEntry {
                    event_id: transfer.transfer_id.clone(),
                    apparatus: transfer.to_apparatus.clone(),
                    order_id: transfer.order_id.clone(),
                    action: queue_state::ApparatusQueueAction::Pause,
                    from_state: queue_state::ApparatusQueueOrderState::Paused,
                    to_state: queue_state::ApparatusQueueOrderState::Paused,
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
            let progress_batches = self.store.progress_batches_for_order(&order_id).await?;
            closed.push(FullyCompletedProductionOrder {
                order_id,
                order_number: map.order_number.trim().to_string(),
                title: map.title.trim().to_string(),
                product_code: map.product_code.trim().to_string(),
                completed_at_unix: closed_event.created_at_unix,
                closed_by_role: closed_event.actor_role.clone(),
                closed_by_ref: closed_event.actor_ref.clone(),
                closed_by_display_name: closed_event.actor_display_name.clone(),
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
        closed.truncate(limit.clamp(1, 500));
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
    ) -> Result<(), ProductionMapError> {
        let Some(previous) = self.raw_map(&next.id).await? else {
            return Ok(());
        };
        let order_id = previous.id.trim();
        let mut started_apparatus = BTreeSet::<ApparatusId>::new();

        for (apparatus, states) in self.store.apparatus_queue_states().await? {
            let Some(state) = states.get(order_id) else {
                continue;
            };
            if queue_state::ApparatusQueueOrderState::parse(state)
                != Some(queue_state::ApparatusQueueOrderState::Pending)
            {
                insert_apparatus_id(&mut started_apparatus, &apparatus);
            }
        }
        for session in self.store.order_run_sessions_for_order(order_id).await? {
            insert_apparatus_id(&mut started_apparatus, &session.apparatus);
        }
        for batch in self.store.progress_batches_for_order(order_id).await? {
            insert_apparatus_id(&mut started_apparatus, &batch.apparatus);
            insert_apparatus_id(&mut started_apparatus, &batch.current_apparatus);
            insert_apparatus_id(&mut started_apparatus, &batch.used_by_apparatus);
            insert_apparatus_id(&mut started_apparatus, &batch.processed_by_apparatus);
        }
        if started_apparatus.is_empty() {
            return Ok(());
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

    pub async fn upsert_map(
        &self,
        map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        self.upsert_map_under_queue_guard(map).await
    }

    async fn upsert_map_under_queue_guard(
        &self,
        mut map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        normalize_map(&mut map);
        let program = compile_map(&map)?;
        self.reject_started_stage_changes(&map).await?;
        self.store.put_map(map.clone()).await?;
        self.notify_live();
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
            normalize_map(&mut map);
            let program = compile_map(&map)?;
            self.reject_started_stage_changes(&map).await?;
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

    pub async fn raw_map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapDefinition>, ProductionMapError> {
        let map_id = map_id.trim().to_ascii_lowercase();
        Ok(self
            .store
            .maps()
            .await?
            .into_iter()
            .find(|map| map.id.trim() == map_id))
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
                    map.width_mm,
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

        self.store.put_maps_batch(&updated).await?;
        self.notify_live();
        updated
            .into_iter()
            .map(|map| {
                let program = compile_map(&map)?;
                Ok(ProductionMapSaved { map, program })
            })
            .collect()
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
        let Some(map) = maps.into_iter().find(|map| map.id.trim() == map_id) else {
            return Err(ProductionMapError::MapNotFound);
        };
        if !move_allowed_by_id(&map, &from_id, &to_id)
            || !crate::core::production_map::pechat::reroute_order_compatible(
                &source,
                &target,
                map.roll_count,
                map.width_mm,
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
        self.upsert_map_under_queue_guard(next).await
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
