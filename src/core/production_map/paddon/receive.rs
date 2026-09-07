use super::progress::unix_seconds;
use super::service_queue_support::{
    finished_goods_qty_uom, finished_goods_stock_entry, mark_finished_goods_batch_received,
};
use super::*;

impl ProductionMapService {
    pub async fn validate_paddon_receiving_items(
        &self,
        items: &[OrderProgressBatch],
    ) -> Result<(), ProductionMapError> {
        if items.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        for batch in items {
            let map = self
                .raw_map(&batch.order_id)
                .await?
                .ok_or(ProductionMapError::MapNotFound)?;
            let stage = batch
                .payload_json
                .get("stage_node_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let final_stage = if stage.is_empty() {
                chain::is_final_work_stage_station(&map, &batch.apparatus)
            } else {
                chain::is_final_work_stage_node(&map, stage)
            };
            if !final_stage
                || !batch.is_finished_goods_output()
                || batch.wip_status != OrderProgressBatchWipStatus::Waiting
            {
                return Err(ProductionMapError::ProgressBatchNotAccepted);
            }
            if map.product_code.trim().is_empty() {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            finished_goods_qty_uom(batch)?;
        }
        Ok(())
    }
    pub async fn paddon_receipt(
        &self,
        code: &str,
    ) -> Result<Option<PaddonReceipt>, ProductionMapError> {
        self.store.paddon_receipt(code.trim()).await
    }

    pub async fn receive_paddon(
        &self,
        code: &str,
        warehouse: &str,
        expected_batch_ids: &[String],
        snapshot_token: &str,
        actor: QueueActionActor,
    ) -> Result<PaddonReceipt, ProductionMapError> {
        if actor.role != "werka" {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if warehouse.trim().is_empty() || expected_batch_ids.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        let _guard = self.queue_action_guard().await;
        if let Some(receipt) = self.paddon_receipt(code).await? {
            validate_receipt_retry(&receipt, warehouse, expected_batch_ids)?;
            return Ok(receipt);
        }
        let snapshot = self.paddon_scan_snapshot(code).await?;
        if paddon_snapshot_token(&snapshot) != snapshot_token {
            return Err(ProductionMapError::PaddonReceiptConflict);
        }
        self.validate_paddon_receiving_items(&snapshot.items)
            .await?;
        let mut actual: Vec<_> = snapshot.items.iter().map(|b| b.batch_id.clone()).collect();
        let mut expected = expected_batch_ids.to_vec();
        actual.sort();
        expected.sort();
        if actual != expected {
            return Err(ProductionMapError::PaddonReceiptConflict);
        }
        let mut receipt = PaddonReceipt {
            paddon: snapshot.paddon,
            items: Vec::new(),
            stocks: Vec::new(),
            warehouse: warehouse.trim().to_string(),
            accepted_by_ref: actor.ref_.clone(),
            accepted_by_display_name: actor.display_name.clone(),
            accepted_at_unix: unix_seconds(),
        };
        for original in &snapshot.items {
            let map = self
                .raw_map(&original.order_id)
                .await?
                .ok_or(ProductionMapError::MapNotFound)?;
            let stage = original
                .payload_json
                .get("stage_node_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let final_stage = if stage.is_empty() {
                chain::is_final_work_stage_station(&map, &original.apparatus)
            } else {
                chain::is_final_work_stage_node(&map, stage)
            };
            if !final_stage
                || !original.is_finished_goods_output()
                || original.wip_status != OrderProgressBatchWipStatus::Waiting
            {
                return Err(ProductionMapError::ProgressBatchNotAccepted);
            }
            if map.product_code.trim().is_empty() {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            let (qty, uom) = finished_goods_qty_uom(original)?;
            let mut stock = finished_goods_stock_entry(
                original,
                &receipt.warehouse,
                &map.product_code,
                &map.title,
                &actor,
                qty,
                uom,
                receipt.accepted_at_unix,
            );
            stock.payload_json["paddon_code"] = serde_json::json!(code.trim());
            let mut batch = original.clone();
            mark_finished_goods_batch_received(
                &mut batch,
                &stock,
                &receipt.warehouse,
                &actor,
                receipt.accepted_at_unix,
            );
            receipt.items.push(batch);
            receipt.stocks.push(stock);
        }
        receipt.paddon.location = receipt.warehouse.clone();
        receipt.paddon.updated_at_unix = receipt.accepted_at_unix;
        let receipt = self
            .store
            .receive_paddon(PaddonReceiveWrite {
                originals: snapshot.items,
                receipt,
            })
            .await?;
        self.notify_live();
        Ok(receipt)
    }
}

pub(crate) fn paddon_snapshot_token(snapshot: &PaddonSnapshot) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(snapshot).expect("serializable paddon snapshot"))
    )
}

pub(crate) fn validate_receipt_retry(
    receipt: &PaddonReceipt,
    warehouse: &str,
    ids: &[String],
) -> Result<(), ProductionMapError> {
    let mut actual: Vec<_> = receipt.items.iter().map(|b| b.batch_id.clone()).collect();
    let mut expected = ids.to_vec();
    actual.sort();
    expected.sort();
    if receipt.warehouse != warehouse.trim() || actual != expected {
        return Err(ProductionMapError::PaddonAlreadyReceived);
    }
    Ok(())
}
