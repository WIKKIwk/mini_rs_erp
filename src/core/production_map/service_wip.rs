use std::collections::BTreeMap;

use super::*;

use super::progress::unix_seconds;
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
        let order = self.raw_map(&order_id).await?;
        let mut progress_batches = self.store.progress_batches_for_order(&order_id).await?;
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
        let queue_states =
            queue_states_for_order(self.store.apparatus_queue_states().await?, &order_id);
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(std::slice::from_ref(&order_id))
            .await?;
        let logs = logs_by_order.get(&order_id).cloned().unwrap_or_default();
        let opened_by = logs.first().map(|entry| ProductionQrOpenedBy {
            actor_role: entry.actor_role.clone(),
            actor_ref: entry.actor_ref.clone(),
            actor_display_name: entry.actor_display_name.clone(),
            opened_at_unix: entry.created_at_unix,
        });
        let run_sessions = self.store.order_run_sessions_for_order(&order_id).await?;
        let active_sessions = run_sessions
            .iter()
            .filter(|session| {
                matches!(
                    session.status,
                    OrderRunStatus::Active | OrderRunStatus::Paused
                )
            })
            .cloned()
            .collect();
        let order_status = ProductionOrderStatusDetail::from_order_flow(
            &progress_batches,
            &run_sessions,
            &queue_states,
            &logs,
        );
        Ok(ProductionQrReport {
            scanned_batch,
            current_batch,
            is_stale,
            stale_reason,
            order,
            order_status,
            queue_states,
            logs,
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
        if batch.action != queue_state::ApparatusQueueAction::Complete
            || batch.status != OrderProgressBatchStatus::Completed
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
            || !batch.next_apparatus.trim().is_empty()
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let order_map = self
            .raw_map(&batch.order_id)
            .await?
            .ok_or(ProductionMapError::MapNotFound)?;
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
        let mut batches = self.store.wip_progress_batches(query).await?;
        if batches.iter().any(progress_batch_needs_location_repair) {
            let maps_by_id = self
                .store
                .maps()
                .await?
                .into_iter()
                .map(|map| (map.id.trim().to_string(), map))
                .collect::<BTreeMap<_, _>>();
            repair_wip_progress_batch_locations(&mut batches, &maps_by_id);
        }
        for batch in &mut batches {
            batch.refresh_status_detail();
        }
        Ok(batches)
    }
}
