use std::collections::BTreeMap;

use super::*;

use super::progress::unix_seconds;
use super::service_progress_support::normalize_self_consumed_wip_history;
use super::service_queue_support::*;

impl ProductionMapService {
    pub async fn progress_qr_report(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<ProductionQrReport, ProductionMapError> {
        let scanned_batch = self
            .progress_batch_for_qr(progress_batch_id, qr_payload)
            .await?;
        let order_id = scanned_batch.order_id.trim().to_string();
        let (
            order,
            mut progress_batches,
            all_queue_states,
            mut logs_by_order,
            corrections,
            run_sessions,
            order_status,
        ) = tokio::try_join!(
            self.raw_map(&order_id),
            self.store.progress_batches_for_order(&order_id),
            self.store.apparatus_queue_states(),
            self.store
                .queue_action_logs_for_orders(std::slice::from_ref(&order_id)),
            self.store
                .progress_batch_corrections_for_order(&order_id),
            self.store.order_run_sessions_for_order(&order_id),
            self.order_status_detail(&order_id),
        )?;
        normalize_self_consumed_wip_history(&mut progress_batches);
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
        let queue_states = queue_states_for_order(&all_queue_states, &order_id);
        let logs = logs_by_order.remove(&order_id).unwrap_or_default();
        let opened_by = logs.first().map(|entry| ProductionQrOpenedBy {
            actor_role: entry.actor_role.clone(),
            actor_ref: entry.actor_ref.clone(),
            actor_display_name: entry.actor_display_name.clone(),
            opened_at_unix: entry.created_at_unix,
        });
        let active_sessions = run_sessions
            .iter()
            .filter(|session| {
                matches!(
                    session.status,
                    OrderRunStatus::Active | OrderRunStatus::Paused | OrderRunStatus::RollDetached
                )
            })
            .cloned()
            .collect();
        Ok(ProductionQrReport {
            scanned_batch,
            current_batch,
            is_stale,
            stale_reason,
            order,
            order_status,
            queue_states,
            logs,
            corrections,
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
        if !actor.role.trim().eq_ignore_ascii_case("werka") {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let _guard = self.queue_action_guard().await;
        let mut batch = self
            .progress_batch_for_qr(progress_batch_id, qr_payload)
            .await?;
        let order_map = self
            .raw_map(&batch.order_id)
            .await?
            .ok_or(ProductionMapError::MapNotFound)?;
        let stage_node_id = batch
            .payload_json
            .get("stage_node_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        let is_final_stage = if stage_node_id.is_empty() {
            chain::is_final_work_stage_station(&order_map, &batch.apparatus)
        } else {
            chain::is_final_work_stage_node(&order_map, stage_node_id)
        };
        if !batch.is_finished_goods_output()
            || !is_final_stage
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let item_code = order_map.product_code.trim();
        if item_code.is_empty() {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let item_name = if order_map.title.trim().is_empty() {
            batch.label_item_name.trim()
        } else {
            order_map.title.trim()
        };
        let now = unix_seconds();
        let (qty, uom) = finished_goods_qty_uom(&batch)?;
        let stock = finished_goods_stock_entry(
            &batch, warehouse, item_code, item_name, &actor, qty, uom, now,
        );
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
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        let requested_status = query.status;
        let include_processed = query.include_processed;
        let requested_limit = query.limit;
        let requested_next_apparatus = query.next_apparatus.trim().to_string();
        let mut store_query = query;
        if !include_processed
            && requested_status.is_none_or(|status| status == OrderProgressBatchWipStatus::Waiting)
        {
            store_query.status = None;
            store_query.include_processed = true;
            store_query.limit = 500;
        }
        if !requested_next_apparatus.is_empty() {
            // Alternative topology is resolved with the order map below. Do
            // not make the store guess that a producer's first candidate is
            // the only valid canonical consumer.
            store_query.next_apparatus.clear();
            store_query.limit = 500;
        }
        let load_maps = !requested_next_apparatus.is_empty();
        let load_order_controls = !include_processed;
        let (mut batches, loaded_maps, order_controls) = tokio::try_join!(
            self.store.wip_progress_batches(store_query),
            async {
                if load_maps {
                    self.store.maps().await
                } else {
                    Ok(Vec::new())
                }
            },
            async {
                if load_order_controls {
                    self.store.order_control_states().await
                } else {
                    Ok(BTreeMap::new())
                }
            },
        )?;
        normalize_self_consumed_wip_history(&mut batches);
        let mut maps_by_id = maps_by_order_id(loaded_maps);
        if !requested_next_apparatus.is_empty() {
            batches.retain(|batch| {
                maps_by_id
                    .get(batch.order_id.trim())
                    .is_some_and(|map| {
                        chain::stage_ids_match_for_map(
                            map,
                            &batch.next_apparatus,
                            &requested_next_apparatus,
                        )
                    })
            });
        }
        if !include_processed {
            batches.retain(|batch| {
                requested_status.map_or(
                    batch.wip_status != OrderProgressBatchWipStatus::Processed,
                    |status| batch.wip_status == status,
                )
            });
            batches.retain(|batch| {
                order_controls
                    .get(batch.order_id.trim())
                    .is_none_or(|control| control.state != OrderControlState::Frozen)
            });
        }
        if batches.iter().any(progress_batch_needs_location_repair) {
            if !load_maps {
                maps_by_id = maps_by_order_id(self.store.maps().await?);
            }
            repair_wip_progress_batch_locations(&mut batches, &maps_by_id);
        }
        for batch in &mut batches {
            batch.refresh_status_detail();
        }
        batches.truncate(requested_limit.min(500));
        Ok(batches)
    }
}

fn maps_by_order_id(
    maps: Vec<ProductionMapDefinition>,
) -> BTreeMap<String, ProductionMapDefinition> {
    maps.into_iter()
        .map(|map| (map.id.trim().to_string(), map))
        .collect()
}
