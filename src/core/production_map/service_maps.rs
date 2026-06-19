use super::*;

use super::apparatus::{
    move_allowed, reassign_alternative_apparatus_assignment, reassign_apparatus_nodes,
};
use super::compiler::{compile_map, normalize_map, run_map_with_variables};
use super::progress::{
    latest_required_complete_event, order_completed_on_apparatus,
    required_apparatus_for_closed_order,
};

impl ProductionMapService {
    pub async fn maps(&self) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let maps = self.store.maps().await?;
        let mut saved = Vec::with_capacity(maps.len());
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
        Ok(saved)
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
            let required_apparatus = required_apparatus_for_closed_order(&map);
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
        let mut closed = Vec::new();
        for (map, required_apparatus) in candidates {
            let order_id = map.id.trim().to_string();
            let logs = logs_by_order.get(&order_id).cloned().unwrap_or_default();
            let Some(closed_event) = latest_required_complete_event(&logs, &required_apparatus)
            else {
                continue;
            };
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

    pub async fn upsert_map(
        &self,
        mut map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        normalize_map(&mut map);
        let program = compile_map(&map)?;
        self.store.put_map(map.clone()).await?;
        self.notify_live();
        Ok(ProductionMapSaved { map, program })
    }

    #[allow(dead_code)]
    pub async fn upsert_maps_batch(
        &self,
        maps: Vec<ProductionMapDefinition>,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let mut normalized = Vec::with_capacity(maps.len());
        let mut saved = Vec::with_capacity(maps.len());
        for mut map in maps {
            normalize_map(&mut map);
            let program = compile_map(&map)?;
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
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if from.is_empty() || to.is_empty() || from == to {
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

        let maps = self.store.maps().await?;
        let mut updated = Vec::with_capacity(map_ids.len());
        for map_id in &map_ids {
            let Some(map) = maps.iter().find(|item| item.id.trim() == map_id).cloned() else {
                return Err(ProductionMapError::MapNotFound);
            };
            if !move_allowed(&map, from, to) {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            let mut next = map;
            if !reassign_alternative_apparatus_assignment(&mut next, from, to)
                && !reassign_apparatus_nodes(&mut next, from, to)
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
        let map_id = input.map_id.trim().to_ascii_lowercase();
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if map_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if to.is_empty() || from == to {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let maps = self.store.maps().await?;
        let Some(map) = maps.into_iter().find(|map| map.id.trim() == map_id) else {
            return Err(ProductionMapError::MapNotFound);
        };
        if !move_allowed(&map, from, to) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let mut next = map;
        if !reassign_alternative_apparatus_assignment(&mut next, from, to)
            && !reassign_apparatus_nodes(&mut next, from, to)
        {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        self.upsert_map(next).await
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
