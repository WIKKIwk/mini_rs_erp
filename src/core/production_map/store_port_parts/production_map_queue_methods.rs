macro_rules! production_map_queue_methods {
    ($($following:tt)*) => {
        production_map_catalog_methods! {
            async fn apparatus_queue_states(&self) -> StoreResult<ApparatusQueueStateMap>;
            async fn put_apparatus_queue_states(
                &self,
                apparatus: &str,
                states: QueueStateMap,
            ) -> StoreResult<()>;
            async fn put_apparatus_queue_states_with_event(
                &self,
                apparatus: &str,
                states: QueueStateMap,
                event: ApparatusQueueActionEvent,
            ) -> StoreResult<()> {
                self.put_apparatus_queue_states(apparatus, states).await?;
                self.append_apparatus_queue_action_event(event).await
            }
            async fn append_apparatus_queue_action_event(
                &self,
                _event: ApparatusQueueActionEvent,
            ) -> StoreResult<()> {
                Ok(())
            }
            async fn completed_queue_orders_for_actor(
                &self,
                _actor_ref: &str,
                _limit: usize,
            ) -> StoreResult<Vec<CompletedQueueOrder>> {
                Ok(Vec::new())
            }
            async fn completed_queue_orders_for_apparatuses(
                &self,
                _apparatuses: &[String],
                _limit: usize,
            ) -> StoreResult<Vec<CompletedQueueOrder>> {
                Ok(Vec::new())
            }
            async fn completion_requests(
                &self,
                _limit: usize,
            ) -> StoreResult<Vec<CompletionRequestNotification>> {
                Ok(Vec::new())
            }
            async fn completion_request_by_event_id(
                &self,
                _event_id: &str,
            ) -> StoreResult<Option<CompletionRequestNotification>> {
                Ok(None)
            }
            async fn completion_request_decisions_for_actor(
                &self,
                _actor_ref: &str,
                _limit: usize,
            ) -> StoreResult<Vec<CompletionRequestDecisionNotification>> {
                Ok(Vec::new())
            }
            async fn resolve_completion_request_decision(
                &self,
                _request_event_id: &str,
                _decision: CompletionRequestDecision,
                _actor: &QueueActionActor,
                _notification: &CompletionRequestDecisionNotification,
                _state_resolution: Option<CompletionRequestStateResolution>,
            ) -> StoreResult<QueueActionProgressWriteResult> {
                Ok(QueueActionProgressWriteResult::default())
            }
            async fn queue_action_logs_for_orders(
                &self,
                _order_ids: &[String],
            ) -> StoreResult<OrderLogMap> {
                Ok(BTreeMap::new())
            }
            /// Minimal facts for stage-state and frozen-order snapshot projections.
            /// Audit and lifecycle callers must keep using the complete history read.
            async fn queue_snapshot_logs_for_orders(
                &self,
                order_ids: &[String],
            ) -> StoreResult<OrderLogMap> {
                Ok(queue_snapshot_logs_from_history(
                    self.queue_action_logs_for_orders(order_ids).await?,
                ))
            }
            async fn queue_action_logs_for_worker(
                &self,
                _worker_refs: &[String],
                _worker_display_name: &str,
                _limit: usize,
            ) -> StoreResult<Vec<ProductionOrderLogEntry>> {
                Ok(Vec::new())
            }

            // Run session, progress event, WIP, and finished-goods persistence.
            async fn active_order_run_session(
                &self,
                _apparatus: &str,
                _order_id: &str,
            ) -> StoreResult<Option<OrderRunSession>> {
                Ok(None)
            }
            async fn active_order_run_session_for_qolip(
                &self,
                _qolip_code: &str,
            ) -> StoreResult<Option<OrderRunSession>> {
                Ok(None)
            }
            async fn active_order_run_sessions_for_worker(
                &self,
                _worker_refs: &[String],
                _running_on_apparatus: Option<&[String]>,
                _limit: usize,
            ) -> StoreResult<Vec<OrderRunSession>> {
                Ok(Vec::new())
            }
            async fn active_order_run_sessions_for_orders(
                &self,
                order_ids: &[String],
            ) -> StoreResult<BTreeMap<String, Vec<OrderRunSession>>> {
                let mut sessions = self.order_run_sessions_for_orders(order_ids).await?;
                for order_sessions in sessions.values_mut() {
                    order_sessions.retain(|session| session.status.is_open());
                }
                Ok(sessions)
            }
            async fn order_run_session(&self, _session_id: &str) -> StoreResult<Option<OrderRunSession>> {
                Ok(None)
            }
            async fn order_run_sessions_for_order(
                &self,
                _order_id: &str,
            ) -> StoreResult<Vec<OrderRunSession>> {
                Ok(Vec::new())
            }
            async fn order_run_sessions_for_orders(
                &self,
                order_ids: &[String],
            ) -> StoreResult<BTreeMap<String, Vec<OrderRunSession>>> {
                let mut sessions = BTreeMap::new();
                for order_id in order_ids {
                    let order_id = order_id.trim();
                    if order_id.is_empty() {
                        continue;
                    }
                    sessions.insert(
                        order_id.to_string(),
                        self.order_run_sessions_for_order(order_id).await?,
                    );
                }
                Ok(sessions)
            }
            async fn order_run_sessions_for_audit(&self) -> StoreResult<Vec<OrderRunSession>> {
                Ok(Vec::new())
            }
            async fn active_order_run_sessions_for_apparatus(&self, apparatus: &str) -> StoreResult<Vec<OrderRunSession>> {
                Ok(self.order_run_sessions_for_audit().await?.into_iter()
                    .filter(|session| session.status == OrderRunStatus::Active && session.apparatus.trim() == apparatus.trim())
                    .collect())
            }
            async fn bosma_astatka_reports_for_order(
                &self,
                _order_id: &str,
            ) -> StoreResult<Vec<BosmaAstatkaReport>> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn put_bosma_astatka_report(&self, _report: BosmaAstatkaReport) -> StoreResult<()> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn laminatsiya_astatka_reports_for_order(
                &self,
                _order_id: &str,
            ) -> StoreResult<Vec<LaminatsiyaAstatkaReport>> {
                Ok(Vec::new())
            }
            async fn put_laminatsiya_astatka_report(
                &self,
                _report: LaminatsiyaAstatkaReport,
            ) -> StoreResult<()> {
                Ok(())
            }
            async fn rezka_astatka_reports_for_order(
                &self,
                _order_id: &str,
            ) -> StoreResult<Vec<RezkaAstatkaReport>> {
                Ok(Vec::new())
            }
            async fn put_rezka_astatka_report(&self, _report: RezkaAstatkaReport) -> StoreResult<()> {
                Ok(())
            }
            async fn progress_batch(&self, _batch_id: &str) -> StoreResult<Option<OrderProgressBatch>> {
                Ok(None)
            }
            async fn progress_batch_by_qr(
                &self,
                _qr_payload: &str,
            ) -> StoreResult<Option<OrderProgressBatch>> {
                Ok(None)
            }
            async fn progress_batches_for_worker(
                &self,
                _worker_refs: &[String],
                _worker_display_name: &str,
                _limit: usize,
            ) -> StoreResult<Vec<OrderProgressBatch>> {
                Ok(Vec::new())
            }
            async fn progress_batches_for_order(
                &self,
                _order_id: &str,
            ) -> StoreResult<Vec<OrderProgressBatch>> {
                Ok(Vec::new())
            }
            async fn progress_batches_for_orders(
                &self,
                order_ids: &[String],
            ) -> StoreResult<BTreeMap<String, Vec<OrderProgressBatch>>> {
                let mut batches = BTreeMap::new();
                for order_id in order_ids {
                    let order_id = order_id.trim();
                    if order_id.is_empty() {
                        continue;
                    }
                    batches.insert(
                        order_id.to_string(),
                        self.progress_batches_for_order(order_id).await?,
                    );
                }
                Ok(batches)
            }
            async fn progress_batch_corrections_for_order(
                &self,
                _order_id: &str,
            ) -> StoreResult<Vec<ProgressBatchCorrectionRecord>> {
                Ok(Vec::new())
            }
            async fn correct_progress_batch(
                &self,
                _current: OrderProgressBatch,
                _input: ProgressBatchCorrectionInput,
                _actor: QueueActionActor,
            ) -> StoreResult<OrderProgressBatch> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn progress_batches_for_audit(&self) -> StoreResult<Vec<OrderProgressBatch>> {
                self.wip_progress_batches(WipProgressBatchQuery::new(
                    "", "", "", None, true, "", 10_000,
                ))
                .await
            }
            async fn apparatus_transfers_for_audit(
                &self,
            ) -> StoreResult<Vec<ProductionMapApparatusTransferRecord>> {
                Ok(Vec::new())
            }
            async fn wip_progress_batches(
                &self,
                _query: WipProgressBatchQuery,
            ) -> StoreResult<Vec<OrderProgressBatch>> {
                Ok(Vec::new())
            }
            async fn opening_wip_by_idempotency_key(
                &self,
                _idempotency_key: &str,
            ) -> StoreResult<Option<OpeningWipRecord>> {
                Ok(None)
            }
            async fn opening_wip_records(
                &self,
                _query: OpeningWipQuery,
            ) -> StoreResult<Vec<OpeningWipRecord>> {
                Ok(Vec::new())
            }
            async fn opening_wip_batch(
                &self,
                _batch_id: &str,
                _qr_payload: &str,
            ) -> StoreResult<Option<OpeningWipBatchRecord>> {
                Ok(None)
            }
            async fn create_opening_wip(
                &self,
                _write: OpeningWipCreateWrite,
            ) -> StoreResult<OpeningWipRecord> {
                Err(ProductionMapError::StoreFailed)
            }
            async fn delete_opening_wip_batch(
                &self,
                _write: OpeningWipDeleteWrite,
            ) -> StoreResult<OpeningWipBatchRecord> {
                Err(ProductionMapError::StoreFailed)
            }
            $($following)*
        }
    };
}
