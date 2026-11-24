impl MemoryProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        maps::maps(self).await
    }

    async fn production_order_lifecycles(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
        maps::production_order_lifecycles(self, order_ids).await
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        maps::put_map(self, map).await
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        maps::put_maps_batch(self, maps).await
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        maps::delete_map(self, map_id).await
    }

    async fn order_control_states(
        &self,
    ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
        Ok(self.order_controls.read().await.clone())
    }

    async fn order_freeze_requests_for_audit(
        &self,
    ) -> Result<Vec<OrderFreezeAuditRecord>, ProductionMapError> {
        Ok(self
            .order_freeze_requests
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn put_order_control_state(
        &self,
        record: OrderControlRecord,
    ) -> Result<(), ProductionMapError> {
        let control_order_id = record.order_id.clone();
        if !record.stage_node_id.trim().is_empty() {
            let apparatus = record.freeze_request.as_ref().map(|r| r.target_apparatus.trim())
                .filter(|id| !id.is_empty()).ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
            if record.early_close.is_some() { return Err(ProductionMapError::OrderControlActionNotAllowed); }
            self.stage_order_controls.write().await.insert(
                (record.order_id.clone(), apparatus.to_string(), record.stage_node_id.clone()), record.clone());
            if let Some(request) = &record.freeze_request {
                let occurred_at_unix = if request.status == OrderFreezeRequestStatus::Pending {
                    request.requested_at_unix
                } else { request.transitioned_at_unix };
                self.order_freeze_requests.write().await.insert(
                    format!("{}:{}", request.request_id, request.status.as_str()),
                    OrderFreezeAuditRecord { order_id: record.order_id.clone(), request: request.clone(),
                        actor: record.actor.clone(), occurred_at_unix });
            }
            queue::refresh_production_order_lifecycles(self, &[control_order_id]).await?;
            return Ok(());
        }
        if let Some(previous) = self.order_controls.read().await.get(record.order_id.trim()) {
            if let Some(close) = &previous.early_close {
                let Some(next) = &record.early_close else {
                    return Err(ProductionMapError::OrderControlActionNotAllowed);
                };
                if close.comment != next.comment || close.actor != next.actor
                    || close.requested_at_unix != next.requested_at_unix
                    || (close.closed_at_unix.is_some() && close != next)
                    || record.state == OrderControlState::Active {
                    return Err(ProductionMapError::OrderControlActionNotAllowed);
                }
            }
        }
        if let Some(close) = &record.early_close {
            if let Some(closed_at) = close.closed_at_unix {
                if let Some(lifecycle) = self.production_order_lifecycles.write().await.get_mut(record.order_id.trim()) {
                    if lifecycle.status != ProductionOrderLifecycleStatus::Cancelled {
                        lifecycle.status = ProductionOrderLifecycleStatus::Cancelled;
                        lifecycle.lifecycle_version += 1;
                        lifecycle.lifecycle_changed_at_unix = closed_at;
                        lifecycle.closed_at_unix = Some(closed_at);
                        lifecycle.completion_outcome = "with_issue".to_string();
                    }
                }
                for sequence in self.sequences.write().await.values_mut() {
                    sequence.retain(|id| id != &record.order_id);
                }
            }
        }
        self.order_controls
            .write()
            .await
            .insert(record.order_id.trim().to_string(), record.clone());
        if let Some(request) = record.freeze_request {
            let request_id = request.request_id.trim().to_string();
            if !request_id.is_empty() {
                let mut requests = self.order_freeze_requests.write().await;
                let status = request.status.as_str();
                let occurred_at_unix = if request.status == OrderFreezeRequestStatus::Pending {
                    request.requested_at_unix
                } else {
                    request.transitioned_at_unix
                };
                requests.insert(
                    format!("{request_id}:{status}"),
                    OrderFreezeAuditRecord {
                        order_id: record.order_id,
                        request,
                        actor: record.actor,
                        occurred_at_unix,
                    },
                );
            }
        }
        queue::refresh_production_order_lifecycles(self, &[control_order_id]).await?;
        Ok(())
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        maps::apparatus_sequences(self).await
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        maps::put_apparatus_sequence(self, apparatus, order_ids).await
    }

    async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
        capacity::apparatus_downtimes(self).await
    }

    async fn put_apparatus_downtime(
        &self,
        downtime: ApparatusDowntime,
    ) -> Result<(), ProductionMapError> {
        capacity::put_apparatus_downtime(self, downtime).await
    }

    async fn apparatus_schedule_reservations(
        &self,
    ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
        capacity::apparatus_schedule_reservations(self).await
    }

    async fn apparatus_schedule_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
        capacity::apparatus_schedule_reservation_by_idempotency_key(self, idempotency_key).await
    }

    async fn put_apparatus_schedule_reservation(
        &self,
        reservation: ApparatusScheduleReservation,
        capacity_slots: u16,
        finite_capacity: bool,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        capacity::put_apparatus_schedule_reservation(
            self,
            reservation,
            capacity_slots,
            finite_capacity,
        )
        .await
    }

    async fn cancel_apparatus_schedule_reservation(
        &self,
        input: ApparatusScheduleCancelRequest,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        capacity::cancel_apparatus_schedule_reservation(self, input).await
    }

    async fn update_apparatus_schedule_reservation_status(
        &self,
        order_id: &str,
        apparatus_id: &ApparatusId,
        status: ApparatusScheduleStatus,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        capacity::update_apparatus_schedule_reservation_status(
            self,
            order_id,
            apparatus_id,
            status,
            actor,
        )
        .await
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        queue::apparatus_queue_states(self).await
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        queue::put_apparatus_queue_states(self, apparatus, states).await
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        queue::append_apparatus_queue_action_event(self, event).await
    }

    async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        queue::completed_queue_orders_for_actor(self, actor_ref, limit).await
    }

    async fn completed_queue_orders_for_apparatuses(
        &self,
        apparatuses: &[String],
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        queue::completed_queue_orders_for_apparatuses(self, apparatuses, limit).await
    }

    async fn completion_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        queue::completion_requests(self, limit).await
    }

    async fn completion_request_by_event_id(
        &self,
        event_id: &str,
    ) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
        queue::completion_request_by_event_id(self, event_id).await
    }

    async fn completion_request_decisions_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        queue::completion_request_decisions_for_actor(self, actor_ref, limit).await
    }

    async fn resolve_completion_request_decision(
        &self,
        request_event_id: &str,
        decision: CompletionRequestDecision,
        actor: &QueueActionActor,
        notification: &CompletionRequestDecisionNotification,
        state_resolution: Option<CompletionRequestStateResolution>,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        queue::resolve_completion_request_decision(
            self,
            request_event_id,
            decision,
            actor,
            notification,
            state_resolution,
        )
        .await
    }

    async fn queue_action_logs_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
        queue::queue_action_logs_for_orders(self, order_ids).await
    }

    async fn queue_action_logs_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        queue::queue_action_logs_for_worker(self, worker_refs, worker_display_name, limit).await
    }

    async fn active_order_run_session(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        runs::active_order_run_session(self, apparatus, order_id).await
    }

    async fn active_order_run_sessions_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
        runs::active_order_run_sessions_for_orders(self, order_ids).await
    }

    async fn active_order_run_session_for_qolip(
        &self,
        qolip_code: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        runs::active_order_run_session_for_qolip(self, qolip_code).await
    }

    async fn active_order_run_sessions_for_worker(
        &self,
        worker_refs: &[String],
        running_on_apparatus: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        runs::active_order_run_sessions_for_worker(self, worker_refs, running_on_apparatus, limit)
            .await
    }

    async fn order_run_session(
        &self,
        session_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        runs::order_run_session(self, session_id).await
    }

    async fn order_run_sessions_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        runs::order_run_sessions_for_order(self, order_id).await
    }

    async fn laminatsiya_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<LaminatsiyaAstatkaReport>, ProductionMapError> {
        let order_id = order_id.trim();
        let mut reports = self
            .laminatsiya_astatka_reports
            .read()
            .await
            .iter()
            .filter(|report| report.order_id.trim().eq_ignore_ascii_case(order_id))
            .cloned()
            .collect::<Vec<_>>();
        reports.sort_by(|left, right| {
            left.to_at_unix
                .cmp(&right.to_at_unix)
                .then_with(|| left.created_at_unix.cmp(&right.created_at_unix))
                .then_with(|| left.report_id.cmp(&right.report_id))
        });
        Ok(reports)
    }

    async fn put_laminatsiya_astatka_report(
        &self,
        report: LaminatsiyaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        let mut reports = self.laminatsiya_astatka_reports.write().await;
        reports.retain(|existing| existing.report_id != report.report_id);
        reports.push(report);
        Ok(())
    }

    async fn rezka_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<RezkaAstatkaReport>, ProductionMapError> {
        let order_id = order_id.trim();
        let mut reports = self
            .rezka_astatka_reports
            .read()
            .await
            .iter()
            .filter(|report| report.order_id.trim().eq_ignore_ascii_case(order_id))
            .cloned()
            .collect::<Vec<_>>();
        reports.sort_by(|left, right| {
            left.to_at_unix
                .cmp(&right.to_at_unix)
                .then_with(|| left.created_at_unix.cmp(&right.created_at_unix))
                .then_with(|| left.report_id.cmp(&right.report_id))
        });
        Ok(reports)
    }

    async fn put_rezka_astatka_report(
        &self,
        report: RezkaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        let mut reports = self.rezka_astatka_reports.write().await;
        reports.retain(|existing| existing.report_id != report.report_id);
        reports.push(report);
        Ok(())
    }

    async fn order_run_sessions_for_audit(
        &self,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        Ok(self
            .order_run_sessions
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn progress_batch(
        &self,
        batch_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        runs::progress_batch(self, batch_id).await
    }

    async fn progress_batch_by_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        runs::progress_batch_by_qr(self, qr_payload).await
    }

    async fn progress_batches_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        runs::progress_batches_for_worker(self, worker_refs, worker_display_name, limit).await
    }

    async fn progress_batches_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        runs::progress_batches_for_order(self, order_id).await
    }

    async fn progress_batch_corrections_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ProgressBatchCorrectionRecord>, ProductionMapError> {
        runs::progress_batch_corrections_for_order(self, order_id).await
    }

    async fn correct_progress_batch(
        &self,
        current: OrderProgressBatch,
        input: ProgressBatchCorrectionInput,
        actor: QueueActionActor,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        runs::correct_progress_batch(self, current, input, actor).await
    }
}
