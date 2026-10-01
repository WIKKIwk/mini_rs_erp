use super::*;

impl MemoryProductionMapStore {
    pub(super) async fn warehouse_wip_snapshot(
        &self,
        qr: &str,
    ) -> Result<Option<WarehouseWipSnapshot>, ProductionMapError> {
        let batches = self.order_progress_batches.read().await;
        let mut matched = batches
            .values()
            .filter(|b| b.qr_payload.trim().eq_ignore_ascii_case(qr.trim()));
        let Some(batch) = matched.next().cloned() else {
            return Ok(None);
        };
        if matched.next().is_some() {
            return Err(ProductionMapError::WarehouseQrAmbiguous);
        }
        let order = self.maps.read().await.get(&batch.order_id).cloned();
        let order_receiving_allowed = self
            .order_controls
            .read()
            .await
            .get(&batch.order_id)
            .is_none_or(|control| {
                control.state == OrderControlState::Active && control.early_close.is_none()
            });
        // This memory store has no pallet-membership persistence. Production uses
        // the PostgreSQL implementation; its tests cover membership transactions.
        let qr_is_paddon = self
            .paddons
            .read()
            .await
            .values()
            .any(|p| p.code == qr.trim());
        Ok(Some(WarehouseWipSnapshot {
            batch,
            order,
            order_receiving_allowed,
            apparatus_active: true,
            paddon_code: None,
            qr_is_paddon,
        }))
    }

    pub(super) async fn receive_warehouse_wip(
        &self,
        write: WarehouseWipReceiveWrite,
    ) -> Result<(), ProductionMapError> {
        let current = self
            .warehouse_wip_snapshot(&write.expected.batch.qr_payload)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if current != write.expected || current.receive_blocked_reason().is_some() {
            return Err(ProductionMapError::WarehouseWipConflict);
        }
        let order_id = write.batch.order_id.clone();
        let mut batches = self.order_progress_batches.write().await;
        if batches.get(&write.expected.batch.batch_id) != Some(&write.expected.batch) {
            return Err(ProductionMapError::WarehouseWipConflict);
        }
        let mut stocks = self.finished_goods_stock.write().await;
        batches.insert(write.batch.batch_id.clone(), write.batch);
        stocks.insert(write.stock.id.clone(), write.stock);
        drop(stocks);
        drop(batches);
        queue::refresh_production_order_lifecycles(self, &[order_id]).await
    }
}
