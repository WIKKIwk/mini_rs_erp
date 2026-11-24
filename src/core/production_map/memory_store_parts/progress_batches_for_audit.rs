impl MemoryProductionMapStore {
    async fn progress_batches_for_audit(
        &self,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        Ok(self
            .order_progress_batches
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn apparatus_transfers_for_audit(
        &self,
    ) -> Result<Vec<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        Ok(self
            .apparatus_transfers
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn wip_progress_batches(
        &self,
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        runs::wip_progress_batches(self, query).await
    }

    async fn put_order_run_session(
        &self,
        session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        runs::put_order_run_session(self, session).await
    }

    async fn put_order_progress_event(
        &self,
        event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        runs::put_order_progress_event(self, event).await
    }
}
