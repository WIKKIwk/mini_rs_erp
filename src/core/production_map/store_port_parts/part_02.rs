
#[async_trait]
pub trait ProductionMapStorePort: Send + Sync {
    // Maps and apparatus sequence persistence.
    async fn maps(&self) -> StoreResult<Vec<ProductionMapDefinition>>;
    async fn maps_by_lifecycle_statuses(
        &self,
        statuses: &[ProductionOrderLifecycleStatus],
    ) -> StoreResult<Vec<ProductionMapDefinition>> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let maps = self.maps().await?;
        let order_ids = maps
            .iter()
            .map(|map| map.id.trim().to_string())
            .filter(|order_id| !order_id.is_empty())
            .collect::<Vec<_>>();
        let lifecycles = self.production_order_lifecycles(&order_ids).await?;
        Ok(maps
            .into_iter()
            .filter(|map| {
                lifecycles
                    .get(map.id.trim())
                    .is_some_and(|record| statuses.contains(&record.status))
            })
            .collect())
    }
    async fn production_order_lifecycles(
        &self,
        order_ids: &[String],
    ) -> StoreResult<BTreeMap<String, ProductionOrderLifecycleRecord>> {
        let requested = order_ids
            .iter()
            .map(|order_id| order_id.trim())
            .filter(|order_id| !order_id.is_empty())
            .collect::<BTreeSet<_>>();
        let queue_states = self.apparatus_queue_states().await?;
        let mut records = BTreeMap::new();
        for map in self.maps().await? {
            let order_id = map.id.trim();
            if !requested.is_empty() && !requested.contains(order_id) {
                continue;
            }
            let Some(status) = super::progress::derive_production_order_lifecycle(
                &map,
                &queue_states,
            ) else {
                continue;
            };
            let mut record = ProductionOrderLifecycleRecord::released(order_id);
            record.transition_to(status, 0);
            records.insert(order_id.to_string(), record);
        }
        Ok(records)
    }
    async fn put_map(&self, map: ProductionMapDefinition) -> StoreResult<()>;
    async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> StoreResult<()>;
    async fn next_order_number(&self) -> StoreResult<String> {
        let max_order_number = self
            .maps()
            .await?
            .iter()
            .filter_map(|map| {
                let value = map.order_number.trim();
                (value.len() <= 4
                    && !value.is_empty()
                    && value.chars().all(|ch| ch.is_ascii_digit()))
                .then(|| value.parse::<u32>().ok())
                .flatten()
            })
            .max()
            .unwrap_or_default();
        let next_order_number = max_order_number
            .checked_add(1)
            .filter(|value| *value <= 9999)
            .ok_or(ProductionMapError::OrderNumberExhausted)?;
        Ok(format!("{next_order_number:04}"))
    }
    async fn delete_map(&self, map_id: &str) -> StoreResult<()>;
    async fn order_control_states(&self) -> StoreResult<OrderControlMap> {
        Ok(BTreeMap::new())
    }
    async fn order_freeze_requests_for_audit(&self) -> StoreResult<Vec<OrderFreezeAuditRecord>> {
        Ok(Vec::new())
    }
    async fn put_order_control_state(&self, _record: OrderControlRecord) -> StoreResult<()> {
        Ok(())
    }
    async fn apparatus_sequences(&self) -> StoreResult<ApparatusSequenceMap>;
    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> StoreResult<()>;

    // Finite capacity, working calendars, downtime, and reservations.
    async fn apparatus_downtimes(&self) -> StoreResult<Vec<ApparatusDowntime>> {
        Ok(Vec::new())
    }
    async fn put_apparatus_downtime(&self, _downtime: ApparatusDowntime) -> StoreResult<()> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn apparatus_schedule_reservations(
        &self,
    ) -> StoreResult<Vec<ApparatusScheduleReservation>> {
        Ok(Vec::new())
    }
    async fn apparatus_schedule_reservation_by_idempotency_key(
        &self,
        _idempotency_key: &str,
    ) -> StoreResult<Option<ApparatusScheduleReservation>> {
        Ok(None)
    }
    async fn put_apparatus_schedule_reservation(
        &self,
        _reservation: ApparatusScheduleReservation,
        _capacity_slots: u16,
        _finite_capacity: bool,
    ) -> StoreResult<ApparatusScheduleReservation> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn cancel_apparatus_schedule_reservation(
        &self,
        _input: ApparatusScheduleCancelRequest,
    ) -> StoreResult<ApparatusScheduleReservation> {
        Err(ProductionMapError::ScheduleReservationNotFound)
    }
    async fn update_apparatus_schedule_reservation_status(
        &self,
        _order_id: &str,
        _apparatus_id: &ApparatusId,
        _status: ApparatusScheduleStatus,
        _actor: &QueueActionActor,
    ) -> StoreResult<()> {
        Ok(())
    }

    // Queue state, policy, log, and completion-request persistence.
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
        _worker_display_name: &str,
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
    async fn paddons(&self, _limit: usize) -> StoreResult<Vec<PaddonSummary>> {
        Ok(Vec::new())
    }
    async fn paddon_summary(&self, _code: &str) -> StoreResult<Option<PaddonSummary>> {
        Ok(None)
    }
    async fn create_paddon(&self, _input: PaddonCreateInput) -> StoreResult<PaddonSummary> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn paddon_snapshot(&self, _code: &str) -> StoreResult<Option<PaddonSnapshot>> {
        Ok(None)
    }
    async fn paddon_scan_snapshot(&self, _code: &str) -> StoreResult<Option<PaddonSnapshot>> {
        Ok(None)
    }
    async fn add_paddon_item(
        &self,
        _code: &str,
        _progress_batch_id: &str,
        _actor: &QueueActionActor,
    ) -> StoreResult<PaddonSnapshot> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn add_paddon_items(
        &self,
        _code: &str,
        _progress_batch_ids: &[String],
        _actor: &QueueActionActor,
    ) -> StoreResult<PaddonSnapshot> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn remove_paddon_item(
        &self,
        _code: &str,
        _progress_batch_id: &str,
        _actor: &QueueActionActor,
    ) -> StoreResult<PaddonSnapshot> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn remove_paddon_items(
        &self,
        _code: &str,
        _progress_batch_ids: &[String],
        _actor: &QueueActionActor,
    ) -> StoreResult<PaddonSnapshot> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn put_order_run_session(&self, _session: OrderRunSession) -> StoreResult<()> {
        Ok(())
    }
    async fn put_order_progress_event(&self, _event: OrderProgressEvent) -> StoreResult<()> {
        Ok(())
    }
    async fn put_order_progress_batch(&self, _batch: OrderProgressBatch) -> StoreResult<()> {
        Ok(())
    }
    async fn apparatus_transfer_by_idempotency_key(
        &self,
        _idempotency_key: &str,
    ) -> StoreResult<Option<ProductionMapApparatusTransferRecord>> {
        Ok(None)
    }
    async fn commit_apparatus_transfer(
        &self,
        _write: ProductionMapApparatusTransferWrite,
    ) -> StoreResult<ProductionMapApparatusTransferRecord> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        _stock: FinishedGoodsStockEntry,
    ) -> StoreResult<()> {
        self.put_order_progress_batch(batch).await
    }
    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: QueueActionProgressWrite,
    ) -> StoreResult<QueueActionProgressWriteResult> {
        validate_queue_progress_write(&write)?;
        if let Some(map) = write.map_update.clone() {
            self.put_map(map).await?;
        }
        let schedule_reservation_status = write.schedule_reservation_status;
        let sequence_updates = write.sequence_updates;
        let event_order_id = write.event.order_id.clone();
        let event_actor = write.event.actor.clone();
        let event_apparatus = write.apparatus.clone();
        self.put_apparatus_queue_states_with_event(&write.apparatus, write.states, write.event)
            .await?;
        for (apparatus, order_ids) in sequence_updates {
            self.put_apparatus_sequence(&apparatus, order_ids).await?;
        }
        if let Some(session) = write.session {
            self.put_order_run_session(session).await?;
        }
        if let Some(event) = write.progress_event {
            self.put_order_progress_event(event).await?;
        }
        let progress_batches = write.progress_batches;
        if progress_batches.is_empty() {
            if let Some(batch) = write.progress_batch {
                self.put_order_progress_batch(batch).await?;
            }
        } else {
            for batch in progress_batches {
                self.put_order_progress_batch(batch).await?;
            }
        }
        for batch in write.progress_batch_updates {
            self.put_order_progress_batch(batch).await?;
        }
        if let Some(record) = write.order_control_update {
            self.put_order_control_state(record).await?;
        }
        if let Some(status) = schedule_reservation_status {
            let event_apparatus_id = ApparatusId::new(event_apparatus.trim().to_string())
                .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
            self.update_apparatus_schedule_reservation_status(
                &event_order_id,
                &event_apparatus_id,
                status,
                &event_actor,
            )
            .await?;
        }
        Ok(QueueActionProgressWriteResult::default())
    }

    // Raw material assignment persistence. Apparatus material rules are
    // canonical runtime projections and have no independent store API.
    async fn raw_material_assignments(&self) -> StoreResult<Vec<RawMaterialAssignment>>;
    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> StoreResult<()>;
    async fn receive_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
        _actor: &QueueActionActor,
    ) -> StoreResult<Vec<String>> {
        self.put_raw_material_assignment(assignment).await?;
        Ok(Vec::new())
    }
    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> StoreResult<Option<RawMaterialAssignment>>;
}
