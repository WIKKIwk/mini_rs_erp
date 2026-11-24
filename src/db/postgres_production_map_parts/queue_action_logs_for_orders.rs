impl PostgresProductionMapStore {
    async fn queue_action_logs_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
        load_queue_action_logs_for_orders(&self.pool, order_ids).await
    }

    async fn queue_snapshot_logs_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
        load_queue_snapshot_logs_for_orders(&self.pool, order_ids).await
    }

    async fn queue_action_logs_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        load_queue_action_logs_for_worker(&self.pool, worker_refs, worker_display_name, limit).await
    }

    async fn active_order_run_session(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        load_active_order_run_session(&self.pool, apparatus, order_id).await
    }

    async fn active_order_run_sessions_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
        load_active_order_run_sessions_for_orders(&self.pool, order_ids).await
    }

    async fn active_order_run_session_for_qolip(
        &self,
        qolip_code: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        load_active_order_run_session_for_qolip(&self.pool, qolip_code).await
    }

    async fn active_order_run_sessions_for_worker(
        &self,
        worker_refs: &[String],
        running_on_apparatus: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        load_active_order_run_sessions_for_worker(
            &self.pool,
            worker_refs,
            running_on_apparatus,
            limit,
        )
        .await
    }

    async fn order_run_session(
        &self,
        session_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        load_order_run_session(&self.pool, session_id).await
    }

    async fn order_run_sessions_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        load_order_run_sessions_for_order(&self.pool, order_id).await
    }

    async fn order_run_sessions_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
        load_order_run_sessions_for_orders(&self.pool, order_ids).await
    }

    async fn laminatsiya_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<LaminatsiyaAstatkaReport>, ProductionMapError> {
        load_laminatsiya_astatka_reports_for_order(&self.pool, order_id).await
    }

    async fn put_laminatsiya_astatka_report(
        &self,
        report: LaminatsiyaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        put_laminatsiya_astatka_report(&self.pool, &report).await
    }

    async fn rezka_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<RezkaAstatkaReport>, ProductionMapError> {
        load_rezka_astatka_reports_for_order(&self.pool, order_id).await
    }

    async fn put_rezka_astatka_report(
        &self,
        report: RezkaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        put_rezka_astatka_report(&self.pool, &report).await
    }

    async fn order_run_sessions_for_audit(
        &self,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        load_order_run_sessions_for_audit(&self.pool).await
    }

    async fn progress_batch(
        &self,
        batch_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        load_progress_batch(&self.pool, batch_id).await
    }

    async fn progress_batch_by_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        load_progress_batch_by_qr(&self.pool, qr_payload).await
    }

    async fn progress_batches_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_progress_batches_for_worker(&self.pool, worker_refs, worker_display_name, limit).await
    }

    async fn progress_batches_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_progress_batches_for_order(&self.pool, order_id).await
    }

    async fn progress_batches_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderProgressBatch>>, ProductionMapError> {
        load_progress_batches_for_orders(&self.pool, order_ids).await
    }

    async fn progress_batch_corrections_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ProgressBatchCorrectionRecord>, ProductionMapError> {
        load_progress_batch_corrections_for_order(&self.pool, order_id).await
    }

    async fn correct_progress_batch(
        &self,
        current: OrderProgressBatch,
        input: ProgressBatchCorrectionInput,
        actor: QueueActionActor,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        correct_progress_batch(&self.pool, &current, &input, &actor).await
    }

    async fn progress_batches_for_audit(
        &self,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_progress_batches_for_audit(&self.pool).await
    }

    async fn apparatus_transfers_for_audit(
        &self,
    ) -> Result<Vec<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        load_apparatus_transfers_for_audit(&self.pool).await
    }

    async fn wip_progress_batches(
        &self,
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_wip_progress_batches(&self.pool, query).await
    }

    async fn opening_wip_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<OpeningWipRecord>, ProductionMapError> {
        load_opening_wip_by_idempotency_key(&self.pool, idempotency_key).await
    }

    async fn opening_wip_records(
        &self,
        query: OpeningWipQuery,
    ) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
        load_opening_wip_records(&self.pool, query).await
    }

    async fn opening_wip_batch(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Result<Option<OpeningWipBatchRecord>, ProductionMapError> {
        load_opening_wip_batch(&self.pool, batch_id, qr_payload).await
    }
}
