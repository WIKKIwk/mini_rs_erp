macro_rules! postgres_map_execution {
    ($($following:tt)*) => {
        postgres_map_catalog! {
            async fn apparatus_queue_states(
                &self,
            ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
                PostgresProductionMapStore::apparatus_queue_states(self).await
            }

            async fn put_apparatus_queue_states(
                &self,
                apparatus: &str,
                states: BTreeMap<String, String>,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_apparatus_queue_states(self, apparatus, states).await
            }

            async fn put_apparatus_queue_states_with_event(
                &self,
                apparatus: &str,
                _states: BTreeMap<String, String>,
                event: ApparatusQueueActionEvent,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_apparatus_queue_states_with_event(self, apparatus, _states, event).await
            }

            async fn append_apparatus_queue_action_event(
                &self,
                event: ApparatusQueueActionEvent,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::append_apparatus_queue_action_event(self, event).await
            }

            async fn completed_queue_orders_for_actor(
                &self,
                actor_ref: &str,
                limit: usize,
            ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
                PostgresProductionMapStore::completed_queue_orders_for_actor(self, actor_ref, limit).await
            }

            async fn completed_queue_orders_for_apparatuses(
                &self,
                apparatuses: &[String],
                limit: usize,
            ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
                PostgresProductionMapStore::completed_queue_orders_for_apparatuses(self, apparatuses, limit).await
            }

            async fn completion_requests(
                &self,
                limit: usize,
            ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
                PostgresProductionMapStore::completion_requests(self, limit).await
            }

            async fn completion_request_by_event_id(
                &self,
                event_id: &str,
            ) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
                PostgresProductionMapStore::completion_request_by_event_id(self, event_id).await
            }

            async fn completion_request_decisions_for_actor(
                &self,
                actor_ref: &str,
                limit: usize,
            ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
                PostgresProductionMapStore::completion_request_decisions_for_actor(self, actor_ref, limit).await
            }

            async fn resolve_completion_request_decision(
                &self,
                request_event_id: &str,
                decision: CompletionRequestDecision,
                actor: &QueueActionActor,
                notification: &CompletionRequestDecisionNotification,
                state_resolution: Option<CompletionRequestStateResolution>,
            ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
                PostgresProductionMapStore::resolve_completion_request_decision(self, request_event_id, decision, actor, notification, state_resolution).await
            }

            async fn queue_action_logs_for_orders(
                &self,
                order_ids: &[String],
            ) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
                PostgresProductionMapStore::queue_action_logs_for_orders(self, order_ids).await
            }

            async fn queue_snapshot_logs_for_orders(
                &self,
                order_ids: &[String],
            ) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
                PostgresProductionMapStore::queue_snapshot_logs_for_orders(self, order_ids).await
            }

            async fn queue_action_logs_for_worker(
                &self,
                worker_refs: &[String],
                worker_display_name: &str,
                limit: usize,
            ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
                PostgresProductionMapStore::queue_action_logs_for_worker(self, worker_refs, worker_display_name, limit).await
            }

            async fn active_order_run_session(
                &self,
                apparatus: &str,
                order_id: &str,
            ) -> Result<Option<OrderRunSession>, ProductionMapError> {
                PostgresProductionMapStore::active_order_run_session(self, apparatus, order_id).await
            }

            async fn active_order_run_session_for_qolip(
                &self,
                qolip_code: &str,
            ) -> Result<Option<OrderRunSession>, ProductionMapError> {
                PostgresProductionMapStore::active_order_run_session_for_qolip(self, qolip_code).await
            }

            async fn active_order_run_sessions_for_worker(
                &self,
                worker_refs: &[String],
                running_on_apparatus: Option<&[String]>,
                limit: usize,
            ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
                PostgresProductionMapStore::active_order_run_sessions_for_worker(self, worker_refs, running_on_apparatus, limit).await
            }

            async fn active_order_run_sessions_for_orders(
                &self,
                order_ids: &[String],
            ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
                PostgresProductionMapStore::active_order_run_sessions_for_orders(self, order_ids).await
            }

            async fn order_run_session(
                &self,
                session_id: &str,
            ) -> Result<Option<OrderRunSession>, ProductionMapError> {
                PostgresProductionMapStore::order_run_session(self, session_id).await
            }

            async fn order_run_sessions_for_order(
                &self,
                order_id: &str,
            ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
                PostgresProductionMapStore::order_run_sessions_for_order(self, order_id).await
            }

            async fn order_run_sessions_for_orders(
                &self,
                order_ids: &[String],
            ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
                PostgresProductionMapStore::order_run_sessions_for_orders(self, order_ids).await
            }

            async fn bosma_astatka_reports_for_order(
                &self,
                order_id: &str,
            ) -> Result<Vec<BosmaAstatkaReport>, ProductionMapError> {
                let rows = sqlx::query_scalar::<_, serde_json::Value>(
                    "SELECT report_json FROM mini_bosma_astatka_reports WHERE order_id = $1 ORDER BY to_at_unix, report_id",
                ).bind(order_id.trim()).fetch_all(&self.pool).await
                    .map_err(|_| ProductionMapError::StoreFailed)?;
                rows.into_iter().map(|row| serde_json::from_value(row)
                    .map_err(|_| ProductionMapError::StoreFailed)).collect()
            }

            async fn put_bosma_astatka_report(&self, report: BosmaAstatkaReport) -> Result<(), ProductionMapError> {
                let payload = serde_json::to_value(&report).map_err(|_| ProductionMapError::StoreFailed)?;
                sqlx::query("INSERT INTO mini_bosma_astatka_reports
            (report_id, order_id, apparatus, from_at_unix, to_at_unix, report_json)
            VALUES ($1, $2, $3, $4, $5, $6)")
                    .bind(&report.report_id).bind(&report.order_id).bind(&report.apparatus)
                    .bind(report.from_at_unix).bind(report.to_at_unix).bind(payload)
                    .execute(&self.pool).await.map_err(|_| ProductionMapError::StoreFailed)?;
                Ok(())
            }

            async fn laminatsiya_astatka_reports_for_order(
                &self,
                order_id: &str,
            ) -> Result<Vec<LaminatsiyaAstatkaReport>, ProductionMapError> {
                PostgresProductionMapStore::laminatsiya_astatka_reports_for_order(self, order_id).await
            }

            async fn put_laminatsiya_astatka_report(
                &self,
                report: LaminatsiyaAstatkaReport,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_laminatsiya_astatka_report(self, report).await
            }

            async fn rezka_astatka_reports_for_order(
                &self,
                order_id: &str,
            ) -> Result<Vec<RezkaAstatkaReport>, ProductionMapError> {
                PostgresProductionMapStore::rezka_astatka_reports_for_order(self, order_id).await
            }

            async fn put_rezka_astatka_report(
                &self,
                report: RezkaAstatkaReport,
            ) -> Result<(), ProductionMapError> {
                PostgresProductionMapStore::put_rezka_astatka_report(self, report).await
            }

            async fn order_run_sessions_for_audit(
                &self,
            ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
                PostgresProductionMapStore::order_run_sessions_for_audit(self).await
            }

            async fn progress_batch(
                &self,
                batch_id: &str,
            ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
                PostgresProductionMapStore::progress_batch(self, batch_id).await
            }

            async fn progress_batch_by_qr(
                &self,
                qr_payload: &str,
            ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
                PostgresProductionMapStore::progress_batch_by_qr(self, qr_payload).await
            }

            async fn progress_batches_for_worker(
                &self,
                worker_refs: &[String],
                worker_display_name: &str,
                limit: usize,
            ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
                PostgresProductionMapStore::progress_batches_for_worker(self, worker_refs, worker_display_name, limit).await
            }

            async fn progress_batches_for_order(
                &self,
                order_id: &str,
            ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
                PostgresProductionMapStore::progress_batches_for_order(self, order_id).await
            }

            async fn progress_batches_for_orders(
                &self,
                order_ids: &[String],
            ) -> Result<BTreeMap<String, Vec<OrderProgressBatch>>, ProductionMapError> {
                PostgresProductionMapStore::progress_batches_for_orders(self, order_ids).await
            }

            async fn progress_batch_corrections_for_order(
                &self,
                order_id: &str,
            ) -> Result<Vec<ProgressBatchCorrectionRecord>, ProductionMapError> {
                PostgresProductionMapStore::progress_batch_corrections_for_order(self, order_id).await
            }

            async fn correct_progress_batch(
                &self,
                current: OrderProgressBatch,
                input: ProgressBatchCorrectionInput,
                actor: QueueActionActor,
            ) -> Result<OrderProgressBatch, ProductionMapError> {
                PostgresProductionMapStore::correct_progress_batch(self, current, input, actor).await
            }

            async fn progress_batches_for_audit(
                &self,
            ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
                PostgresProductionMapStore::progress_batches_for_audit(self).await
            }

            async fn apparatus_transfers_for_audit(
                &self,
            ) -> Result<Vec<ProductionMapApparatusTransferRecord>, ProductionMapError> {
                PostgresProductionMapStore::apparatus_transfers_for_audit(self).await
            }

            async fn wip_progress_batches(
                &self,
                query: WipProgressBatchQuery,
            ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
                PostgresProductionMapStore::wip_progress_batches(self, query).await
            }

            async fn opening_wip_by_idempotency_key(
                &self,
                idempotency_key: &str,
            ) -> Result<Option<OpeningWipRecord>, ProductionMapError> {
                PostgresProductionMapStore::opening_wip_by_idempotency_key(self, idempotency_key).await
            }

            async fn opening_wip_records(
                &self,
                query: OpeningWipQuery,
            ) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
                PostgresProductionMapStore::opening_wip_records(self, query).await
            }

            async fn opening_wip_batch(
                &self,
                batch_id: &str,
                qr_payload: &str,
            ) -> Result<Option<OpeningWipBatchRecord>, ProductionMapError> {
                PostgresProductionMapStore::opening_wip_batch(self, batch_id, qr_payload).await
            }

            async fn create_opening_wip(
                &self,
                write: OpeningWipCreateWrite,
            ) -> Result<OpeningWipRecord, ProductionMapError> {
                PostgresProductionMapStore::create_opening_wip(self, write).await
            }

            async fn delete_opening_wip_batch(
                &self,
                write: OpeningWipDeleteWrite,
            ) -> Result<OpeningWipBatchRecord, ProductionMapError> {
                PostgresProductionMapStore::delete_opening_wip_batch(self, write).await
            }
            $($following)*
        }
    };
}
