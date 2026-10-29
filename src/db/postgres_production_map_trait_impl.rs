#[async_trait]
impl ProductionMapStorePort for PostgresProductionMapStore {
    async fn qolip_start_settings(&self) -> Result<crate::core::production_map::QolipStartSettings, ProductionMapError> {
        qolip_start_settings::load(&self.pool).await
    }
    async fn update_qolip_start_settings(&self, enabled: bool, actor: &QueueActionActor) -> Result<crate::core::production_map::QolipStartSettings, ProductionMapError> {
        qolip_start_settings::update(&self.pool, enabled, actor).await
    }
    async fn output_stats(
        &self,
        request: &crate::core::production_map::output_stats::OutputStatsRequest,
        scope: Option<&[String]>,
    ) -> Result<crate::core::production_map::output_stats::OutputStatsReport, crate::core::production_map::output_stats::OutputStatsError> {
        output_stats::load(&self.pool, request, scope).await
    }

    async fn lamination_settings(&self) -> Result<crate::core::production_map::LaminationSettings, ProductionMapError> {
        lamination_settings::load(&self.pool).await
    }
    async fn update_lamination_settings(&self, required: bool, actor: &QueueActionActor) -> Result<crate::core::production_map::LaminationSettings, ProductionMapError> {
        lamination_settings::update(&self.pool, required, actor).await
    }
    async fn reclose_reactivated_work(&self, target: &crate::core::production_map::ReactivationRecloseTarget, actor: &QueueActionActor, assigned: &[String]) -> Result<(), ProductionMapError> {
        reactivation::reclose::execute(self, target, actor, assigned).await
    }
    async fn reactivation_was_applied(&self, target: &crate::core::production_map::ReactivationTarget, actor: &QueueActionActor) -> Result<bool, ProductionMapError> {
        reactivation::was_applied(self, target, actor).await
    }
    async fn preview_reactivation(&self, selector: &crate::core::production_map::ReactivationSelector, actor: &QueueActionActor, is_print: bool, is_lamination: bool) -> Result<crate::core::production_map::ReactivationPreview, ProductionMapError> {
        reactivation::preview(self, selector, actor, is_print, is_lamination).await
    }
    async fn reactivate_completed_work(&self, target: &crate::core::production_map::ReactivationTarget, actor: &QueueActionActor, assigned: &[String], checkouts: &[crate::core::qolip::QolipCheckout], is_print: bool, is_lamination: bool) -> Result<(), ProductionMapError> {
        reactivation::execute(self, target, actor, assigned, checkouts, is_print, is_lamination).await
    }
    fn supports_resume_pause(&self) -> bool { true }
    fn supports_temporary_pause(&self) -> bool { true }
    fn supports_cancel_start(&self) -> bool { true }
    async fn cancel_started_work(&self, apparatus: &str, order_id: &str, session_id: &str, token: &str,
        actor: &QueueActionActor, assigned: &[String]) -> Result<(), ProductionMapError> {
        cancel_start::execute(self, apparatus, order_id, session_id, token, actor, assigned).await
    }
    async fn temporarily_pause_work(&self, apparatus: &str, order_id: &str, session_id: &str, token: &str,
        actor: &QueueActionActor, assigned: &[String], policy: crate::core::production_map::ApparatusQueuePolicy)
        -> Result<(), ProductionMapError> {
        temporary_pause::execute(self, apparatus, order_id, session_id, token, actor, assigned, policy).await
    }

    async fn re_pause_resumed_work(
        &self, apparatus: &str, order_id: &str, resume_id: &str,
        actor: &QueueActionActor, assigned_apparatus: &[String],
    ) -> Result<(), ProductionMapError> {
        re_pause::re_pause(self, apparatus, order_id, resume_id, actor, assigned_apparatus).await
    }


    async fn layer_remainder_sources(&self, apparatus: &str, order_id: &str) -> Result<Vec<crate::core::production_map::LayerRemainderSource>, ProductionMapError> {
        PostgresProductionMapStore::layer_remainder_sources(self, apparatus, order_id).await
    }
    async fn layer_remainder_history(&self, apparatus: &str, order_id: &str) -> Result<Vec<crate::core::production_map::LayerRemainderRecord>, ProductionMapError> {
        PostgresProductionMapStore::layer_remainder_history(self, apparatus, order_id).await
    }
    async fn layer_remainder_by_id(&self, id: &str) -> Result<Option<crate::core::production_map::LayerRemainderRecord>, ProductionMapError> {
        PostgresProductionMapStore::layer_remainder_by_id(self, id).await
    }
    async fn record_layer_remainder(&self, input: crate::core::production_map::LayerRemainderInput, actor: QueueActionActor) -> Result<crate::core::production_map::LayerRemainderRecord, ProductionMapError> {
        PostgresProductionMapStore::record_layer_remainder(self, input, actor).await
    }
    async fn laminatsiya_astatka_commit_by_request_id(&self, actor_ref: &str, request_id: &str) -> Result<Option<(serde_json::Value,crate::core::production_map::LaminatsiyaAstatkaCommitResult)>,ProductionMapError> {
        PostgresProductionMapStore::laminatsiya_astatka_commit_by_request_id(self,actor_ref,request_id).await
    }
    async fn commit_laminatsiya_astatka(&self, request_id:String, request_json:serde_json::Value,
        report:Option<LaminatsiyaAstatkaReport>, expected_session:OrderRunSession, actor:QueueActionActor,
        queue_write:Option<QueueActionProgressWrite>,remainders:Vec<crate::core::production_map::LayerRemainderInput>,
    ) -> Result<crate::core::production_map::LaminatsiyaAstatkaCommitResult,ProductionMapError> {
        PostgresProductionMapStore::commit_laminatsiya_astatka(self,request_id,request_json,report,expected_session,actor,queue_write,remainders).await
    }
    async fn paddon_management_settings(&self) -> Result<PaddonManagementSettings, ProductionMapError> {
        paddon_management::load(&self.pool).await
    }
    async fn update_paddon_management_settings(&self, free_movement_enabled: Option<bool>, worker_visibility_enabled: Option<bool>, actor: &QueueActionActor) -> Result<PaddonManagementSettings, ProductionMapError> {
        paddon_management::update(&self.pool, free_movement_enabled, worker_visibility_enabled, actor).await
    }
    async fn move_apparatus_sequence(
        &self,
        canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
        command: &crate::core::production_map::SequenceMove,
        actor: &QueueActionActor,
    ) -> Result<crate::core::production_map::SequenceMoveResult, ProductionMapError> {
        sequence_move::commit(&self.pool, canonical, command, actor).await
    }
    async fn rezka_astatka_commit_by_request_id(
        &self, actor: &QueueActionActor, request_id: &str,
    ) -> Result<Option<(serde_json::Value, RezkaAstatkaReport)>, ProductionMapError> {
        stage_execution::rezka_receipt(&self.pool, actor, request_id).await
    }
    async fn commit_rezka_astatka_report(
        &self, request_id: String, request_json: serde_json::Value,
        report: RezkaAstatkaReport, expected_session: OrderRunSession, actor: QueueActionActor,
    ) -> Result<RezkaAstatkaReport, ProductionMapError> {
        stage_execution::commit_rezka_report(&self.pool, request_id, request_json, report, expected_session, actor).await
    }
    async fn commit_stage_astatka_report(&self, report: crate::core::production_map::StageAstatkaReport,
        expected: Option<OrderRunSession>, actor: QueueActionActor) -> Result<(), ProductionMapError> {
        stage_execution::commit_report(&self.pool, report, expected, actor).await
    }
    async fn paddon_receipt(&self, code: &str) -> Result<Option<crate::core::production_map::PaddonReceipt>, ProductionMapError> {
        paddon_receipts::load(&self.pool, code).await
    }

    async fn warehouse_wip_snapshot(&self, qr: &str) -> Result<Option<crate::core::production_map::WarehouseWipSnapshot>, ProductionMapError> {
        warehouse_wip::load(&self.pool, qr).await
    }
    async fn receive_warehouse_wip(&self, write: crate::core::production_map::WarehouseWipReceiveWrite) -> Result<(), ProductionMapError> {
        warehouse_wip::commit(&self.pool, write).await
    }
    async fn receive_paddon(&self, write: crate::core::production_map::PaddonReceiveWrite) -> Result<crate::core::production_map::PaddonReceipt, ProductionMapError> {
        paddon_receipts::commit(&self.pool, write).await
    }
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        PostgresProductionMapStore::maps(self).await
    }

    async fn map_by_id(&self, map_id: &str) -> Result<Option<ProductionMapDefinition>, ProductionMapError> {
        #[cfg(test)]
        if self.legacy_queue_reads {
            return Ok(self.maps().await?.into_iter().find(|map| map.id.trim() == map_id.trim()));
        }
        catalog_helpers::load_map_by_id(&self.pool, map_id).await
    }

    async fn maps_for_apparatus(&self, apparatus: &str) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        #[cfg(test)]
        if self.legacy_queue_reads { return self.maps().await; }
        catalog_helpers::load_maps_for_apparatus(&self.pool, apparatus).await
    }

    async fn maps_for_snapshot_scope(&self, apparatus: &[String], extra_order_ids: &[String]) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        #[cfg(test)]
        if self.legacy_queue_reads { return self.maps().await; }
        catalog_helpers::load_maps_for_snapshot_scope(&self.pool, apparatus, extra_order_ids).await
    }

    async fn raw_material_assignments_for_orders(&self, order_ids: &[String]) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        material_helpers::load_raw_material_assignments_for_orders(&self.pool, order_ids).await
    }

    async fn opening_wip_records_for_orders(&self, order_ids: &[String]) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
        opening_wip_helpers::load_opening_wip_records_for_orders(&self.pool, order_ids).await
    }

    async fn stage_order_control_states(&self) -> Result<Vec<OrderControlRecord>, ProductionMapError> {
        order_control_helpers::load_stage_controls(&self.pool).await
    }

    async fn order_control_by_id(&self, order_id: &str) -> Result<Option<OrderControlRecord>, ProductionMapError> {
        #[cfg(test)]
        if self.legacy_queue_reads { return Ok(self.order_control_states().await?.remove(order_id.trim())); }
        order_control_helpers::load_order_control_by_id(&self.pool, order_id).await
    }

    async fn raw_material_assignments_for_order(&self, order_id: &str) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        #[cfg(test)]
        if self.legacy_queue_reads {
            return Ok(self.raw_material_assignments().await?.into_iter()
                .filter(|assignment| assignment.order_id.trim() == order_id.trim()).collect());
        }
        material_helpers::load_raw_material_assignments_for_order(&self.pool, order_id).await
    }

    async fn active_order_run_sessions_for_apparatus(&self, apparatus: &str) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        #[cfg(test)]
        if self.legacy_queue_reads {
            return Ok(self.order_run_sessions_for_audit().await?.into_iter()
                .filter(|session| session.status == crate::core::production_map::OrderRunStatus::Active && session.apparatus.trim() == apparatus.trim()).collect());
        }
        order_query_helpers::load_active_order_run_sessions_for_apparatus(&self.pool, apparatus).await
    }

    async fn maps_by_lifecycle_statuses(
        &self,
        statuses: &[ProductionOrderLifecycleStatus],
    ) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        PostgresProductionMapStore::maps_by_lifecycle_statuses(self, statuses).await
    }

    async fn raw_material_assignment_maps(
        &self,
    ) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        PostgresProductionMapStore::raw_material_assignment_maps(self).await
    }

    async fn production_order_lifecycles(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
        PostgresProductionMapStore::production_order_lifecycles(self, order_ids).await
    }

    async fn next_order_number(&self) -> Result<String, ProductionMapError> {
        PostgresProductionMapStore::next_order_number(self).await
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_map(self, map).await
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_maps_batch(self, maps).await
    }

    async fn put_maps_with_sequences(
        &self,
        maps: &[ProductionMapDefinition],
        expected_sequences: &BTreeMap<String, Vec<String>>,
        sequences: &BTreeMap<String, Vec<String>>,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_maps_with_sequences(
            self,
            maps,
            expected_sequences,
            sequences,
        )
        .await
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::delete_map(self, map_id).await
    }

    async fn order_control_states(
        &self,
    ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
        PostgresProductionMapStore::order_control_states(self).await
    }

    async fn order_freeze_requests_for_audit(
        &self,
    ) -> Result<Vec<crate::core::production_map::OrderFreezeAuditRecord>, ProductionMapError> {
        PostgresProductionMapStore::order_freeze_requests_for_audit(self).await
    }

    async fn put_order_control_state(
        &self,
        record: OrderControlRecord,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_order_control_state(self, record).await
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        PostgresProductionMapStore::apparatus_sequences(self).await
    }

    async fn apparatus_sequences_with_revisions(
        &self,
    ) -> Result<(BTreeMap<String, Vec<String>>, BTreeMap<String, i64>), ProductionMapError> {
        let rows = sqlx::query_as::<_, (String, serde_json::Value, i64)>(
            "SELECT canonical_apparatus_id, order_ids, revision FROM mini_queue_sequences",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut sequences = BTreeMap::new();
        let mut revisions = BTreeMap::new();
        for (id, payload, revision) in rows {
            if !crate::core::production_map::queue_state::is_canonical_apparatus_id(&id) {
                continue;
            }
            let Ok(ids) = serde_json::from_value::<Vec<String>>(payload) else {
                continue;
            };
            revisions.insert(id.clone(), revision);
            sequences.insert(id, ids);
        }
        Ok((sequences, revisions))
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_apparatus_sequence(self, apparatus, order_ids).await
    }

    async fn queue_events_replay(
        &self,
        apparatus: &str,
        from_rev: i64,
        to_rev: i64,
    ) -> Result<Vec<serde_json::Value>, ProductionMapError> {
        PostgresProductionMapStore::queue_events_replay(self, apparatus, from_rev, to_rev).await
    }

    async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
        PostgresProductionMapStore::apparatus_downtimes(self).await
    }

    async fn put_apparatus_downtime(
        &self,
        downtime: ApparatusDowntime,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_apparatus_downtime(self, downtime).await
    }

    async fn apparatus_schedule_reservations(
        &self,
    ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
        PostgresProductionMapStore::apparatus_schedule_reservations(self).await
    }

    async fn apparatus_schedule_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
        PostgresProductionMapStore::apparatus_schedule_reservation_by_idempotency_key(self, idempotency_key).await
    }

    async fn put_apparatus_schedule_reservation(
        &self,
        reservation: ApparatusScheduleReservation,
        capacity_slots: u16,
        finite_capacity: bool,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        PostgresProductionMapStore::put_apparatus_schedule_reservation(self, reservation, capacity_slots, finite_capacity).await
    }

    async fn cancel_apparatus_schedule_reservation(
        &self,
        input: ApparatusScheduleCancelRequest,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        PostgresProductionMapStore::cancel_apparatus_schedule_reservation(self, input).await
    }

    async fn update_apparatus_schedule_reservation_status(
        &self,
        order_id: &str,
        apparatus_id: &ApparatusId,
        status: crate::core::production_map::ApparatusScheduleStatus,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::update_apparatus_schedule_reservation_status(self, order_id, apparatus_id, status, actor).await
    }

    async fn active_print_preflight_holds(
        &self,
    ) -> Result<Vec<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
        PostgresProductionMapStore::active_print_preflight_holds(self).await
    }

    async fn print_preflight_hold_by_id(
        &self,
        hold_id: &str,
    ) -> Result<Option<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
        PostgresProductionMapStore::print_preflight_hold_by_id(self, hold_id).await
    }

    async fn print_preflight_hold_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<crate::core::production_map::PrintPreflightHold>, ProductionMapError> {
        PostgresProductionMapStore::print_preflight_hold_by_idempotency_key(self, idempotency_key).await
    }

    async fn put_print_preflight_hold(
        &self,
        hold: crate::core::production_map::PrintPreflightHold,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_print_preflight_hold(self, hold).await
    }

    async fn update_print_preflight_hold(
        &self,
        hold: crate::core::production_map::PrintPreflightHold,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::update_print_preflight_hold(self, hold).await
    }

    async fn put_print_preflight_hold_with_event(
        &self,
        hold: crate::core::production_map::PrintPreflightHold,
    ) -> Result<Option<String>, ProductionMapError> {
        print_preflight::put_with_event(&self.pool, &hold).await.map(Some)
    }

    async fn update_print_preflight_hold_with_event(
        &self,
        hold: crate::core::production_map::PrintPreflightHold,
    ) -> Result<Option<String>, ProductionMapError> {
        print_preflight::update_with_event(&self.pool, &hold).await.map(Some)
    }

    async fn reopen_print_preflight_hold_with_event(
        &self,
        hold: crate::core::production_map::PrintPreflightHold,
    ) -> Result<Option<String>, ProductionMapError> {
        print_preflight::reopen_with_event(&self.pool, &hold).await.map(Some)
    }

    async fn consume_print_preflight_hold(
        &self,
        hold_id: &str,
        order_id: &str,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::consume_print_preflight_hold(
            self, hold_id, order_id, apparatus, actor,
        )
        .await
    }

    async fn cancel_print_preflight_hold(
        &self,
        hold_id: &str,
        order_id: &str,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::cancel_print_preflight_hold(
            self, hold_id, order_id, apparatus, actor,
        )
        .await
    }

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
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        PostgresProductionMapStore::active_order_run_sessions_for_worker(self, worker_refs, worker_display_name, limit).await
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

    async fn paddons(&self, limit: usize) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        PostgresProductionMapStore::paddons(self, limit).await
    }
    async fn selectable_rezka_paddons(&self, limit: usize) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        load_paddons(&self.pool, limit, true, None, None).await
    }
    async fn paddons_for_creator(&self, limit: usize, selectable_only: bool, creator_ref: Option<&str>) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        load_paddons(&self.pool, limit, selectable_only, creator_ref, None).await
    }
    async fn paddons_for_apparatus(&self, limit: usize, selectable_only: bool, creator_ref: Option<&str>, apparatus: Option<&str>) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        load_paddons(&self.pool, limit, selectable_only, creator_ref, apparatus).await
    }
    async fn active_rezka_paddon(&self, apparatus: &str, actor: &QueueActionActor) -> Result<Option<String>, ProductionMapError> {
        active_paddon::load(&self.pool, apparatus, actor).await
    }
    async fn set_active_rezka_paddon(&self, apparatus: &str, actor: &QueueActionActor, code: Option<&str>) -> Result<(), ProductionMapError> {
        active_paddon::save(&self.pool, apparatus, actor, code).await
    }

    async fn confirm_paddon_print(&self, code: &str, actor: &QueueActionActor) -> Result<crate::core::production_map::PaddonPrintConfirmation, ProductionMapError> {
        paddon_print_lock::confirm(&self.pool, code, actor).await
    }
    async fn can_unlock_paddon(&self, code: &str, actor: &QueueActionActor) -> Result<bool, ProductionMapError> {
        paddon_print_lock::can_unlock(&self.pool, code, actor).await
    }
    async fn unlock_paddon(&self, code: &str, actor: &QueueActionActor) -> Result<PaddonSummary, ProductionMapError> {
        paddon_print_lock::unlock(&self.pool, code, actor).await
    }
    async fn create_active_paddon_successor(&self, code: &str, apparatus: &str, actor: &QueueActionActor) -> Result<PaddonSummary, ProductionMapError> {
        paddon_print_lock::successor(&self.pool, code, apparatus, actor).await
    }

    async fn paddon_summary(
        &self,
        code: &str,
    ) -> Result<Option<PaddonSummary>, ProductionMapError> {
        PostgresProductionMapStore::paddon_summary(self, code).await
    }

    async fn create_paddon(
        &self,
        input: PaddonCreateInput,
    ) -> Result<PaddonSummary, ProductionMapError> {
        PostgresProductionMapStore::create_paddon(self, input).await
    }

    async fn delete_paddon(&self, code: &str) -> Result<(), ProductionMapError> {
        paddon_delete::delete_paddon(&self.pool, code).await
    }

    async fn paddon_snapshot(
        &self,
        code: &str,
    ) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
        PostgresProductionMapStore::paddon_snapshot(self, code).await
    }

    async fn paddon_scan_snapshot(
        &self,
        code: &str,
    ) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
        PostgresProductionMapStore::paddon_scan_snapshot(self, code).await
    }

    async fn add_paddon_item(
        &self,
        code: &str,
        progress_batch_id: &str,
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        PostgresProductionMapStore::add_paddon_item(self, code, progress_batch_id, actor).await
    }

    async fn add_paddon_items(
        &self,
        code: &str,
        progress_batch_ids: &[String],
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        PostgresProductionMapStore::add_paddon_items(self, code, progress_batch_ids, actor).await
    }

    async fn transfer_paddon_items(
        &self,
        input: &crate::core::production_map::PaddonTransferInput,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        paddon_transfer::transfer(&self.pool, input, actor, None).await
    }

    async fn paddon_snapshot_for_apparatuses(
        &self, code: &str, actor: &QueueActionActor, allowed: &[String],
    ) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
        paddon_helpers::load_scoped_snapshot(&self.pool, code, actor, allowed).await
    }

    async fn add_paddon_items_for_apparatuses(
        &self, code: &str, ids: &[String], actor: &QueueActionActor, allowed: &[String],
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        paddon_helpers::add_paddon_items_with_scope(&self.pool, code, ids, actor, Some(allowed)).await
    }

    async fn transfer_paddon_items_for_apparatuses(
        &self, input: &crate::core::production_map::PaddonTransferInput,
        actor: &QueueActionActor, allowed: &[String],
    ) -> Result<(), ProductionMapError> {
        paddon_transfer::transfer(&self.pool, input, actor, Some(allowed)).await
    }

    async fn remove_paddon_item(
        &self,
        code: &str,
        progress_batch_id: &str,
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        PostgresProductionMapStore::remove_paddon_item(self, code, progress_batch_id, actor).await
    }

    async fn remove_paddon_items(
        &self,
        code: &str,
        progress_batch_ids: &[String],
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        PostgresProductionMapStore::remove_paddon_items(self, code, progress_batch_ids, actor).await
    }

    async fn put_order_run_session(
        &self,
        session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_order_run_session(self, session).await
    }

    async fn put_order_progress_event(
        &self,
        event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_order_progress_event(self, event).await
    }

    async fn put_order_progress_batch(
        &self,
        batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_order_progress_batch(self, batch).await
    }

    async fn apparatus_transfer_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        PostgresProductionMapStore::apparatus_transfer_by_idempotency_key(self, idempotency_key).await
    }

    async fn commit_apparatus_transfer(
        &self,
        write: ProductionMapApparatusTransferWrite,
    ) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
        PostgresProductionMapStore::commit_apparatus_transfer(self, write).await
    }

    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        stock: FinishedGoodsStockEntry,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::receive_finished_goods_batch(self, batch, stock).await
    }

    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: &QueueActionProgressWrite,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        PostgresProductionMapStore::put_apparatus_queue_states_with_event_and_progress(self, write).await
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        PostgresProductionMapStore::raw_material_assignments(self).await
    }

    async fn raw_material_assignment_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        PostgresProductionMapStore::raw_material_assignment_by_barcode(self, barcode).await
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        PostgresProductionMapStore::put_raw_material_assignment(self, assignment).await
    }

    async fn receive_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
        actor: &QueueActionActor,
    ) -> Result<Vec<String>, ProductionMapError> {
        PostgresProductionMapStore::receive_raw_material_assignment(self, assignment, actor).await
    }

    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        PostgresProductionMapStore::delete_raw_material_assignment(self, order_id, barcode).await
    }
}
