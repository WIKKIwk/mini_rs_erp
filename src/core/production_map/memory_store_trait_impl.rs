#[async_trait]
#[cfg(any(test, feature = "verification"))]
impl ProductionMapStorePort for MemoryProductionMapStore {
    async fn paddon_management_settings(&self) -> Result<PaddonManagementSettings, ProductionMapError> {
        Ok(self.paddon_management_settings.read().await.clone())
    }
    async fn update_paddon_management_settings(&self, free_movement_enabled: Option<bool>, worker_visibility_enabled: Option<bool>, _actor: &QueueActionActor) -> Result<PaddonManagementSettings, ProductionMapError> {
        let mut settings = self.paddon_management_settings.write().await;
        if let Some(enabled) = free_movement_enabled { settings.free_movement_enabled = enabled; }
        if let Some(enabled) = worker_visibility_enabled { settings.worker_visibility_enabled = enabled; }
        Ok(settings.clone())
    }
    async fn paddons_for_creator(&self, limit: usize, selectable_only: bool, creator_ref: Option<&str>) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        let mut paddons: Vec<_> = self.paddons.read().await.values()
            .filter(|p| creator_ref.is_none_or(|creator| p.created_by_ref.trim() == creator)
                && (!selectable_only || p.locked_at_unix.is_none())).cloned().collect();
        paddons.sort_by(|a, b| b.updated_at_unix.cmp(&a.updated_at_unix).then_with(|| a.code.cmp(&b.code)));
        paddons.truncate(limit);
        Ok(paddons)
    }
    async fn move_apparatus_sequence(
        &self,
        canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
        command: &SequenceMove,
        actor: &QueueActionActor,
    ) -> Result<SequenceMoveResult, ProductionMapError> {
        command.validate()?;
        let mut receipts = self.sequence_move_receipts.lock().await;
        let key = (command.apparatus.clone(), actor.role.clone(), actor.ref_.clone(), command.idempotency_key.clone());
        if let Some((previous, result)) = receipts.get(&key) {
            return if previous == command { Ok(result.clone()) }
                else { Err(ProductionMapError::QueueReorderIdempotencyConflict) };
        }
        let maps = self.maps().await?;
        let states = self.apparatus_queue_states().await?;
        let frozen = self.order_control_states().await?.into_iter()
            .filter_map(|(id,c)| (c.state == OrderControlState::Frozen).then_some(id)).collect();
        let holds = self.active_print_preflight_holds().await?.into_iter()
            .filter(|h| h.apparatus == command.apparatus && h.is_live_at(0)).map(|h| h.order_id).collect();
        let mut sequences = self.sequences.write().await;
        let state = SequenceMoveState::from_data(canonical, &maps,
            sequences.get(&command.apparatus).map(Vec::as_slice).unwrap_or_default(),
            states.get(&command.apparatus).unwrap_or(&BTreeMap::new()), &frozen, &holds);
        let mut result = state.apply(command)?;
        if result.event.is_some() {
            sequences.insert(command.apparatus.clone(), result.order_ids.clone());
            result.revision = Some(1);
        }
        receipts.insert(key, (command.clone(), result.clone()));
        Ok(result)
    }
    async fn active_rezka_paddon(&self, apparatus: &str, actor: &QueueActionActor) -> Result<Option<String>, ProductionMapError> {
        let code = self.active_paddons.read().await.get(&(actor.role.clone(), actor.ref_.clone(), apparatus.to_string())).cloned();
        if let Some(code) = code.as_deref().filter(|_| actor.role == "aparatchi") {
            let owned = self.paddons.read().await.get(code).is_some_and(|p| p.created_by_ref.trim() == actor.ref_.trim());
            if !owned && !self.paddon_management_settings.read().await.worker_visibility_enabled { return Ok(None); }
        }
        Ok(code)
    }
    async fn set_active_rezka_paddon(&self, apparatus: &str, actor: &QueueActionActor, code: Option<&str>) -> Result<(), ProductionMapError> {
        if let Some(code) = code {
            let paddons = self.paddons.read().await;
            let paddon = paddons.get(code).ok_or(ProductionMapError::PaddonNotFound)?;
            if paddon.locked_at_unix.is_some() { return Err(ProductionMapError::PaddonLocked); }
        }
        let key = (actor.role.clone(), actor.ref_.clone(), apparatus.to_string());
        let mut selections = self.active_paddons.write().await;
        if let Some(code) = code { selections.insert(key, code.to_string()); } else { selections.remove(&key); }
        Ok(())
    }
    async fn create_paddon(&self, input: PaddonCreateInput) -> Result<PaddonSummary, ProductionMapError> {
        let mut paddons = self.paddons.write().await;
        let code = format!("{:05}", paddons.len() + 1);
        let paddon = PaddonSummary {
            id: code.clone(), code: code.clone(), location: input.location, note: input.note,
            created_by_ref: input.actor_ref, created_by_display_name: input.actor_display_name,
            created_at_unix: 0, updated_at_unix: 0, item_count: 0,
            total_gross_kg: Some(0.0), total_net_kg: Some(0.0), locked_at_unix: None,
        };
        paddons.insert(code, paddon.clone());
        Ok(paddon)
    }
    async fn confirm_paddon_print(&self, code: &str, actor: &QueueActionActor) -> Result<PaddonPrintConfirmation, ProductionMapError> {
        let mut paddons = self.paddons.write().await;
        let paddon = paddons.get_mut(code).ok_or(ProductionMapError::PaddonNotFound)?;
        let newly_locked = paddon.locked_at_unix.is_none();
        if newly_locked {
            self.paddon_lock_owners.write().await.insert(code.to_string(), actor.ref_.trim().to_string());
        }
        paddon.locked_at_unix.get_or_insert(super::progress::unix_seconds());
        let mut active = self.active_paddons.write().await;
        let apparatuses = active.iter().filter(|((role, ref_, _), selected)|
            role == &actor.role && ref_ == &actor.ref_ && selected.as_str() == code)
            .map(|((_, _, apparatus), _)| apparatus.clone()).collect();
        active.retain(|_, selected| selected != code);
        Ok(PaddonPrintConfirmation { paddon: paddon.clone(), newly_locked, apparatuses })
    }
    async fn can_unlock_paddon(&self, code: &str, actor: &QueueActionActor) -> Result<bool, ProductionMapError> {
        let paddons = self.paddons.read().await;
        let paddon = paddons.get(code).ok_or(ProductionMapError::PaddonNotFound)?;
        if paddon.locked_at_unix.is_none() { return Ok(false); }
        let owners = self.paddon_lock_owners.read().await;
        let enabled = self.paddon_management_settings.read().await.free_movement_enabled;
        Ok(paddon_unlock_actor_allowed(owners.get(code).map(String::as_str).unwrap_or_default(), enabled, actor))
    }
    async fn unlock_paddon(&self, code: &str, actor: &QueueActionActor) -> Result<PaddonSummary, ProductionMapError> {
        let mut successors = self.paddon_successors.lock().await;
        let mut paddons = self.paddons.write().await;
        let paddon = paddons.get_mut(code).ok_or(ProductionMapError::PaddonNotFound)?;
        if paddon.locked_at_unix.is_some() {
            let mut owners = self.paddon_lock_owners.write().await;
            let enabled = self.paddon_management_settings.read().await.free_movement_enabled;
            if !paddon_unlock_actor_allowed(owners.get(code).map(String::as_str).unwrap_or_default(), enabled, actor) {
                return Err(ProductionMapError::PaddonUnlockForbidden);
            }
            paddon.locked_at_unix = None;
            paddon.updated_at_unix = super::progress::unix_seconds();
            owners.remove(code);
            successors.retain(|(source, _, _, _), _| source != code);
        }
        Ok(paddon.clone())
    }
    async fn paddon_snapshot(&self, code: &str) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
        let Some(paddon) = self.paddons.read().await.get(code).cloned() else { return Ok(None); };
        let enabled = self.paddon_management_settings.read().await.free_movement_enabled;
        Ok(Some(PaddonSnapshot {
            can_manage_items: paddon.locked_at_unix.is_none() || enabled,
            paddon, items: Vec::new(), available_items: Vec::new(), free_movement_enabled: enabled,
        }))
    }
    async fn paddon_scan_snapshot(&self, code: &str) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
        self.paddon_snapshot(code).await
    }
    async fn paddon_summary(&self, code: &str) -> Result<Option<PaddonSummary>, ProductionMapError> {
        Ok(self.paddons.read().await.get(code).cloned())
    }
    async fn create_active_paddon_successor(&self, code: &str, apparatus: &str, actor: &QueueActionActor) -> Result<PaddonSummary, ProductionMapError> {
        let mut successors = self.paddon_successors.lock().await;
        let key = (code.to_string(), actor.role.clone(), actor.ref_.clone(), apparatus.to_string());
        if let Some(next) = successors.get(&key) {
            self.set_active_rezka_paddon(apparatus, actor, Some(next)).await?;
            return self.paddons.read().await.get(next).cloned().ok_or(ProductionMapError::PaddonNotFound);
        }
        if self.paddons.read().await.get(code).ok_or(ProductionMapError::PaddonNotFound)?.locked_at_unix.is_none() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        let paddon = self.create_paddon(PaddonCreateInput { location: apparatus.to_string(), note: String::new(),
            actor_ref: actor.ref_.clone(), actor_display_name: actor.display_name.clone() }).await?;
        self.set_active_rezka_paddon(apparatus, actor, Some(&paddon.code)).await?;
        successors.insert(key, paddon.code.clone());
        Ok(paddon)
    }
    async fn commit_stage_astatka_report(&self, report: StageAstatkaReport,
        expected: Option<OrderRunSession>, actor: QueueActionActor) -> Result<(), ProductionMapError> {
        let (order_id, apparatus, report_id) = report.identity();
        let (order_id, apparatus, report_id) = (order_id.to_string(), apparatus.to_string(), report_id.to_string());
        let mut sessions = self.order_run_sessions.write().await;
        let current = sessions.values().filter(|s| s.order_id == order_id && s.apparatus == apparatus)
            .max_by(|a, b| (a.started_at_unix, &a.session_id).cmp(&(b.started_at_unix, &b.session_id)));
        if current != expected.as_ref() { return Err(ProductionMapError::QueueActionNotAllowed); }
        match report {
            StageAstatkaReport::Bosma(r) => { self.bosma_astatka_reports.write().await.push(r); }
            StageAstatkaReport::Laminate(r) => self.put_laminatsiya_astatka_report(r).await?,
            StageAstatkaReport::Cut(r) => self.put_rezka_astatka_report(r).await?,
        }
        if let Some(mut session) = expected.filter(|s| s.status == OrderRunStatus::Completed) {
            let sequence = sessions.values().filter(|s| s.order_id == order_id)
                .filter_map(super::stage_execution::work_report).map(|r| r.sequence).max().unwrap_or(0) + 1;
            super::stage_execution::stamp_work_report(&mut session, &report_id, sequence, &actor, super::stage_execution::report_now());
            sessions.insert(session.session_id.clone(), session);
        }
        drop(sessions);
        queue::refresh_production_order_lifecycles(self, &[order_id]).await
    }
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        MemoryProductionMapStore::maps(self).await
    }

    async fn production_order_lifecycles(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>, ProductionMapError> {
        MemoryProductionMapStore::production_order_lifecycles(self, order_ids).await
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_map(self, map).await
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_maps_batch(self, maps).await
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::delete_map(self, map_id).await
    }

    async fn order_control_states(
        &self,
    ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
        MemoryProductionMapStore::order_control_states(self).await
    }

    async fn order_freeze_requests_for_audit(
        &self,
    ) -> Result<Vec<OrderFreezeAuditRecord>, ProductionMapError> {
        MemoryProductionMapStore::order_freeze_requests_for_audit(self).await
    }

    async fn put_order_control_state(
        &self,
        record: OrderControlRecord,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_order_control_state(self, record).await
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        MemoryProductionMapStore::apparatus_sequences(self).await
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_apparatus_sequence(self, apparatus, order_ids).await
    }

    async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
        MemoryProductionMapStore::apparatus_downtimes(self).await
    }

    async fn put_apparatus_downtime(
        &self,
        downtime: ApparatusDowntime,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_apparatus_downtime(self, downtime).await
    }

    async fn apparatus_schedule_reservations(
        &self,
    ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
        MemoryProductionMapStore::apparatus_schedule_reservations(self).await
    }

    async fn apparatus_schedule_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
        MemoryProductionMapStore::apparatus_schedule_reservation_by_idempotency_key(
            self,
            idempotency_key,
        )
        .await
    }

    async fn put_apparatus_schedule_reservation(
        &self,
        reservation: ApparatusScheduleReservation,
        capacity_slots: u16,
        finite_capacity: bool,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        MemoryProductionMapStore::put_apparatus_schedule_reservation(
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
        MemoryProductionMapStore::cancel_apparatus_schedule_reservation(self, input).await
    }

    async fn update_apparatus_schedule_reservation_status(
        &self,
        order_id: &str,
        apparatus_id: &ApparatusId,
        status: ApparatusScheduleStatus,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::update_apparatus_schedule_reservation_status(
            self,
            order_id,
            apparatus_id,
            status,
            actor,
        )
        .await
    }

    async fn active_print_preflight_holds(
        &self,
    ) -> Result<Vec<PrintPreflightHold>, ProductionMapError> {
        let now = super::progress::unix_seconds();
        Ok(self
            .print_preflight_holds
            .read()
            .await
            .values()
            .filter(|hold| hold.is_live_at(now))
            .cloned()
            .collect())
    }

    async fn print_preflight_hold_by_id(
        &self,
        hold_id: &str,
    ) -> Result<Option<PrintPreflightHold>, ProductionMapError> {
        Ok(self.print_preflight_holds.read().await.get(hold_id.trim()).cloned())
    }

    async fn print_preflight_hold_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<PrintPreflightHold>, ProductionMapError> {
        Ok(self
            .print_preflight_holds
            .read()
            .await
            .values()
            .find(|hold| hold.idempotency_key.trim() == idempotency_key.trim())
            .cloned())
    }

    async fn put_print_preflight_hold(
        &self,
        hold: PrintPreflightHold,
    ) -> Result<(), ProductionMapError> {
        let order_id = hold.order_id.clone();
        self.queue_states
            .write()
            .await
            .entry(hold.apparatus.clone())
            .or_default()
            .insert(order_id.clone(), "print_preflight".to_string());
        self.print_preflight_holds
            .write()
            .await
            .insert(hold.hold_id.trim().to_string(), hold);
        queue::refresh_production_order_lifecycles(self, &[order_id]).await
    }

    async fn update_print_preflight_hold(
        &self,
        hold: PrintPreflightHold,
    ) -> Result<(), ProductionMapError> {
        let mut holds = self.print_preflight_holds.write().await;
        if !holds.contains_key(hold.hold_id.trim()) {
            return Err(ProductionMapError::PrintPreflightNotFound);
        }
        let order_id = hold.order_id.clone();
        let mut queue_states = self.queue_states.write().await;
        let states = queue_states.entry(hold.apparatus.clone()).or_default();
        if hold.status.reserves_apparatus() {
            states.insert(order_id.clone(), "print_preflight".to_string());
        } else if let Some(previous) = &hold.previous_queue_state {
            states.insert(order_id.clone(), previous.clone());
        } else {
            states.remove(&order_id);
        }
        holds.insert(hold.hold_id.trim().to_string(), hold);
        drop(queue_states);
        drop(holds);
        queue::refresh_production_order_lifecycles(self, &[order_id]).await
    }

    async fn consume_print_preflight_hold(
        &self,
        hold_id: &str,
        order_id: &str,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let now = super::progress::unix_seconds();
        let mut holds = self.print_preflight_holds.write().await;
        let hold = holds
            .get_mut(hold_id.trim())
            .ok_or(ProductionMapError::PrintPreflightNotFound)?;
        if hold.order_id.trim() != order_id.trim()
            || !queue_state::apparatus_ids_match(&hold.apparatus, apparatus)
            || hold.status != PrintPreflightStatus::Passed
            || !hold.is_live_at(now)
        {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }
        hold.status = PrintPreflightStatus::Consumed;
        hold.actor = actor.clone();
        hold.updated_at_unix = now;
        Ok(())
    }

    async fn cancel_print_preflight_hold(
        &self,
        hold_id: &str,
        order_id: &str,
        apparatus: &str,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let now = super::progress::unix_seconds();
        let mut holds = self.print_preflight_holds.write().await;
        let hold = holds
            .get_mut(hold_id.trim())
            .ok_or(ProductionMapError::PrintPreflightNotFound)?;
        if hold.order_id.trim() != order_id.trim()
            || !queue_state::apparatus_ids_match(&hold.apparatus, apparatus)
            || !matches!(
                hold.status,
                PrintPreflightStatus::Held
                    | PrintPreflightStatus::Running
                    | PrintPreflightStatus::Passed
            )
        {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }
        hold.status = PrintPreflightStatus::Cancelled;
        hold.actor = actor.clone();
        hold.updated_at_unix = now;
        Ok(())
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        MemoryProductionMapStore::apparatus_queue_states(self).await
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_apparatus_queue_states(self, apparatus, states).await
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::append_apparatus_queue_action_event(self, event).await
    }

    async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        MemoryProductionMapStore::completed_queue_orders_for_actor(self, actor_ref, limit).await
    }

    async fn completion_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        MemoryProductionMapStore::completion_requests(self, limit).await
    }

    async fn completion_request_by_event_id(
        &self,
        event_id: &str,
    ) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
        MemoryProductionMapStore::completion_request_by_event_id(self, event_id).await
    }

    async fn completion_request_decisions_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        MemoryProductionMapStore::completion_request_decisions_for_actor(self, actor_ref, limit)
            .await
    }

    async fn resolve_completion_request_decision(
        &self,
        request_event_id: &str,
        decision: CompletionRequestDecision,
        actor: &QueueActionActor,
        notification: &CompletionRequestDecisionNotification,
        state_resolution: Option<CompletionRequestStateResolution>,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        MemoryProductionMapStore::resolve_completion_request_decision(
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
        MemoryProductionMapStore::queue_action_logs_for_orders(self, order_ids).await
    }

    async fn queue_action_logs_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        MemoryProductionMapStore::queue_action_logs_for_worker(
            self,
            worker_refs,
            worker_display_name,
            limit,
        )
        .await
    }

    async fn active_order_run_session(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        MemoryProductionMapStore::active_order_run_session(self, apparatus, order_id).await
    }

    async fn active_order_run_sessions_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
        MemoryProductionMapStore::active_order_run_sessions_for_orders(self, order_ids).await
    }

    async fn active_order_run_session_for_qolip(
        &self,
        qolip_code: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        MemoryProductionMapStore::active_order_run_session_for_qolip(self, qolip_code).await
    }

    async fn active_order_run_sessions_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        MemoryProductionMapStore::active_order_run_sessions_for_worker(
            self,
            worker_refs,
            worker_display_name,
            limit,
        )
        .await
    }

    async fn order_run_session(
        &self,
        session_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        MemoryProductionMapStore::order_run_session(self, session_id).await
    }

    async fn order_run_sessions_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        MemoryProductionMapStore::order_run_sessions_for_order(self, order_id).await
    }

    async fn bosma_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<BosmaAstatkaReport>, ProductionMapError> {
        Ok(self.bosma_astatka_reports.read().await.iter()
            .filter(|report| report.order_id == order_id.trim()).cloned().collect())
    }

    async fn put_bosma_astatka_report(&self, report: BosmaAstatkaReport) -> Result<(), ProductionMapError> {
        self.bosma_astatka_reports.write().await.push(report);
        Ok(())
    }

    async fn laminatsiya_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<LaminatsiyaAstatkaReport>, ProductionMapError> {
        MemoryProductionMapStore::laminatsiya_astatka_reports_for_order(self, order_id).await
    }

    async fn put_laminatsiya_astatka_report(
        &self,
        report: LaminatsiyaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_laminatsiya_astatka_report(self, report).await
    }

    async fn rezka_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<RezkaAstatkaReport>, ProductionMapError> {
        MemoryProductionMapStore::rezka_astatka_reports_for_order(self, order_id).await
    }

    async fn put_rezka_astatka_report(
        &self,
        report: RezkaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_rezka_astatka_report(self, report).await
    }

    async fn order_run_sessions_for_audit(
        &self,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        MemoryProductionMapStore::order_run_sessions_for_audit(self).await
    }

    async fn progress_batch(
        &self,
        batch_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        MemoryProductionMapStore::progress_batch(self, batch_id).await
    }

    async fn progress_batch_by_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        MemoryProductionMapStore::progress_batch_by_qr(self, qr_payload).await
    }

    async fn progress_batches_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        MemoryProductionMapStore::progress_batches_for_worker(
            self,
            worker_refs,
            worker_display_name,
            limit,
        )
        .await
    }

    async fn progress_batches_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        MemoryProductionMapStore::progress_batches_for_order(self, order_id).await
    }

    async fn progress_batch_corrections_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ProgressBatchCorrectionRecord>, ProductionMapError> {
        MemoryProductionMapStore::progress_batch_corrections_for_order(self, order_id).await
    }

    async fn correct_progress_batch(
        &self,
        current: OrderProgressBatch,
        input: ProgressBatchCorrectionInput,
        actor: QueueActionActor,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        MemoryProductionMapStore::correct_progress_batch(self, current, input, actor).await
    }

    async fn progress_batches_for_audit(
        &self,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        MemoryProductionMapStore::progress_batches_for_audit(self).await
    }

    async fn apparatus_transfers_for_audit(
        &self,
    ) -> Result<Vec<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        MemoryProductionMapStore::apparatus_transfers_for_audit(self).await
    }

    async fn wip_progress_batches(
        &self,
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        MemoryProductionMapStore::wip_progress_batches(self, query).await
    }

    async fn opening_wip_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<OpeningWipRecord>, ProductionMapError> {
        MemoryProductionMapStore::opening_wip_by_idempotency_key(self, idempotency_key).await
    }

    async fn opening_wip_records(
        &self,
        query: OpeningWipQuery,
    ) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
        MemoryProductionMapStore::opening_wip_records(self, query).await
    }

    async fn opening_wip_batch(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Result<Option<OpeningWipBatchRecord>, ProductionMapError> {
        MemoryProductionMapStore::opening_wip_batch(self, batch_id, qr_payload).await
    }

    async fn create_opening_wip(
        &self,
        write: OpeningWipCreateWrite,
    ) -> Result<OpeningWipRecord, ProductionMapError> {
        MemoryProductionMapStore::create_opening_wip(self, write).await
    }

    async fn delete_opening_wip_batch(
        &self,
        write: OpeningWipDeleteWrite,
    ) -> Result<OpeningWipBatchRecord, ProductionMapError> {
        MemoryProductionMapStore::delete_opening_wip_batch(self, write).await
    }

    async fn put_order_run_session(
        &self,
        session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_order_run_session(self, session).await
    }

    async fn put_order_progress_event(
        &self,
        event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_order_progress_event(self, event).await
    }

    async fn put_order_progress_batch(
        &self,
        batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_order_progress_batch(self, batch).await
    }

    async fn apparatus_transfer_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        MemoryProductionMapStore::apparatus_transfer_by_idempotency_key(self, idempotency_key).await
    }

    async fn commit_apparatus_transfer(
        &self,
        write: ProductionMapApparatusTransferWrite,
    ) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
        MemoryProductionMapStore::commit_apparatus_transfer(self, write).await
    }

    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: &QueueActionProgressWrite,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        MemoryProductionMapStore::put_apparatus_queue_states_with_event_and_progress(self, write)
            .await
    }

    async fn warehouse_wip_snapshot(&self, qr: &str) -> Result<Option<WarehouseWipSnapshot>, ProductionMapError> {
        MemoryProductionMapStore::warehouse_wip_snapshot(self, qr).await
    }
    async fn receive_warehouse_wip(&self, write: WarehouseWipReceiveWrite) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::receive_warehouse_wip(self, write).await
    }
    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        stock: FinishedGoodsStockEntry,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::receive_finished_goods_batch(self, batch, stock).await
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        MemoryProductionMapStore::raw_material_assignments(self).await
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        MemoryProductionMapStore::put_raw_material_assignment(self, assignment).await
    }

    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        MemoryProductionMapStore::delete_raw_material_assignment(self, order_id, barcode).await
    }
}
