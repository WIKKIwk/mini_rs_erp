//! Warehouse scans are read-only. Receipt writes reuse the existing finished-goods
//! stock representation, with an optimistic snapshot checked again at commit.
use serde::Serialize;

use super::progress::unix_seconds;
use super::service_queue_support::{
    finished_goods_qty_uom, finished_goods_stock_entry, mark_finished_goods_batch_received,
};
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WarehouseWipSnapshot {
    pub batch: OrderProgressBatch,
    pub order: Option<ProductionMapDefinition>,
    pub order_receiving_allowed: bool,
    pub apparatus_active: bool,
    pub paddon_code: Option<String>,
    pub qr_is_paddon: bool,
}

#[derive(Debug, Clone)]
pub struct WarehouseWipReceiveWrite {
    pub expected: WarehouseWipSnapshot,
    pub batch: OrderProgressBatch,
    pub stock: FinishedGoodsStockEntry,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WarehouseWipReceipt {
    pub warehouse: String,
    pub accepted_by_display_name: String,
    pub accepted_at_unix: i64,
}

impl WarehouseWipSnapshot {
    pub fn snapshot_token(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(self).expect("serializable warehouse snapshot"))
        )
    }

    pub fn receipt(&self) -> Option<WarehouseWipReceipt> {
        let batch = &self.batch;
        let warehouse = batch
            .payload_json
            .get("received_warehouse")?
            .as_str()?
            .trim();
        let stock_id = batch
            .payload_json
            .get("finished_goods_stock_id")?
            .as_str()?;
        if warehouse.is_empty()
            || batch.wip_status != OrderProgressBatchWipStatus::Processed
            || stock_id != format!("finished:{}", batch.batch_id.trim())
            || batch.processed_by_session_id != stock_id
        {
            return None;
        }
        Some(WarehouseWipReceipt {
            warehouse: warehouse.to_string(),
            accepted_by_display_name: batch
                .payload_json
                .get("received_by_display_name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            accepted_at_unix: batch.payload_json.get("received_at_unix")?.as_i64()?,
        })
    }

    pub fn receive_blocked_reason(&self) -> Option<&'static str> {
        let batch = &self.batch;
        if self.qr_is_paddon {
            return Some("qr_ambiguous");
        }
        if self.receipt().is_some() {
            return Some("already_received");
        }
        if self.paddon_code.is_some() {
            return Some("paddon_item_already_assigned");
        }
        if !self.order_receiving_allowed {
            return Some("order_not_active");
        }
        if !self.apparatus_active {
            return Some("apparatus_inactive");
        }
        if batch.wip_status != OrderProgressBatchWipStatus::Waiting
            || !batch.used_by_session_id.trim().is_empty()
            || !batch.used_by_apparatus.trim().is_empty()
            || !batch.processed_by_session_id.trim().is_empty()
            || !batch.processed_by_apparatus.trim().is_empty()
        {
            return Some("progress_batch_not_accepted");
        }
        if batch.current_apparatus.trim() != batch.apparatus.trim()
            || batch.current_location.trim().is_empty()
        {
            return Some("wip_location_mismatch");
        }
        let Some(map) = &self.order else {
            return Some("map_not_found");
        };
        let stage_id = batch
            .payload_json
            .get("stage_node_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let final_stage = chain::work_stage_for_station(map, &batch.apparatus, stage_id)
            .is_some_and(|stage| chain::is_final_work_stage_node(map, &stage.node_id));
        if !batch.is_finished_goods_output() || !final_stage {
            return Some("progress_batch_not_accepted");
        }
        if map.product_code.trim().is_empty() || finished_goods_qty_uom(batch).is_err() {
            return Some("progress_input_invalid");
        }
        None
    }
}

impl ProductionMapService {
    pub async fn warehouse_wip_snapshot(
        &self,
        qr_payload: &str,
    ) -> Result<Option<WarehouseWipSnapshot>, ProductionMapError> {
        if qr_payload.trim().is_empty() || qr_payload.len() > 2048 {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        self.store.warehouse_wip_snapshot(qr_payload.trim()).await
    }

    pub async fn receive_warehouse_wip(
        &self,
        batch_id: &str,
        qr_payload: &str,
        warehouse: &str,
        snapshot_token: &str,
        actor: QueueActionActor,
    ) -> Result<(OrderProgressBatch, WarehouseWipReceipt), ProductionMapError> {
        if !matches!(
            actor.role.trim().to_ascii_lowercase().as_str(),
            "werka" | "omborchi"
        ) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if batch_id.trim().is_empty()
            || warehouse.trim().is_empty()
            || snapshot_token.trim().is_empty()
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let _guard = self.queue_action_guard().await;
        let snapshot = self
            .warehouse_wip_snapshot(qr_payload)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if snapshot.batch.batch_id != batch_id.trim() || snapshot.qr_is_paddon {
            return Err(ProductionMapError::WarehouseWipConflict);
        }
        // Only a retry of this exact confirmed snapshot is idempotent. Legacy or
        // pallet receipts remain visible on preview but cannot be re-received.
        if let Some(receipt) = snapshot.receipt() {
            if receipt.warehouse == warehouse.trim()
                && snapshot
                    .batch
                    .payload_json
                    .get("warehouse_receipt_snapshot_token")
                    .and_then(|v| v.as_str())
                    == Some(snapshot_token)
            {
                return Ok((snapshot.batch, receipt));
            }
            return Err(ProductionMapError::WarehouseWipConflict);
        }
        if snapshot.snapshot_token() != snapshot_token
            || snapshot.receive_blocked_reason().is_some()
        {
            return Err(ProductionMapError::WarehouseWipConflict);
        }
        let map = snapshot
            .order
            .as_ref()
            .ok_or(ProductionMapError::MapNotFound)?;
        let (qty, uom) = finished_goods_qty_uom(&snapshot.batch)?;
        let now = unix_seconds();
        let item_name = if map.title.trim().is_empty() {
            &snapshot.batch.label_item_name
        } else {
            &map.title
        };
        let stock = finished_goods_stock_entry(
            &snapshot.batch,
            warehouse.trim(),
            &map.product_code,
            item_name,
            &actor,
            qty,
            uom,
            now,
        );
        let mut batch = snapshot.batch.clone();
        mark_finished_goods_batch_received(&mut batch, &stock, warehouse.trim(), &actor, now);
        batch.payload_json["warehouse_receipt_snapshot_token"] = serde_json::json!(snapshot_token);
        match self
            .store
            .receive_warehouse_wip(WarehouseWipReceiveWrite {
                expected: snapshot,
                batch: batch.clone(),
                stock,
            })
            .await
        {
            Ok(()) => {}
            Err(ProductionMapError::WarehouseWipConflict) => {
                // A second server may have committed this exact request after our
                // initial read. Recover its persisted receipt, never a local guess.
                let current = self
                    .warehouse_wip_snapshot(qr_payload)
                    .await?
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                if current.batch.batch_id == batch_id.trim() && !current.qr_is_paddon {
                    if let Some(receipt) = current.receipt() {
                        if receipt.warehouse == warehouse.trim()
                            && current
                                .batch
                                .payload_json
                                .get("warehouse_receipt_snapshot_token")
                                .and_then(|v| v.as_str())
                                == Some(snapshot_token)
                        {
                            return Ok((current.batch, receipt));
                        }
                    }
                }
                return Err(ProductionMapError::WarehouseWipConflict);
            }
            Err(error) => return Err(error),
        }
        self.notify_live();
        Ok((
            batch,
            WarehouseWipReceipt {
                warehouse: warehouse.trim().to_string(),
                accepted_by_display_name: actor.display_name.trim().to_string(),
                accepted_at_unix: now,
            },
        ))
    }
}
