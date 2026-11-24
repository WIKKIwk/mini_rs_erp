production_map_queue_methods! {
    async fn paddons(&self, _limit: usize) -> StoreResult<Vec<PaddonSummary>> {
        Ok(Vec::new())
    }
    async fn paddons_for_creator(&self, limit: usize, selectable_only: bool, creator_ref: Option<&str>) -> StoreResult<Vec<PaddonSummary>> {
        let paddons = if selectable_only { self.selectable_rezka_paddons(200).await? } else { self.paddons(200).await? };
        Ok(paddons.into_iter().filter(|p| creator_ref.is_none_or(|creator| p.created_by_ref.trim() == creator)).take(limit).collect())
    }
    async fn paddons_for_apparatus(&self, limit: usize, selectable_only: bool, creator_ref: Option<&str>, apparatus: Option<&str>) -> StoreResult<Vec<PaddonSummary>> {
        Ok(self.paddons_for_creator(200, selectable_only, creator_ref).await?.into_iter()
            .filter(|p| apparatus.is_none_or(|id| p.apparatus_id == id)).take(limit).collect())
    }
    async fn paddon_management_settings(&self) -> StoreResult<PaddonManagementSettings> {
        Ok(PaddonManagementSettings::default())
    }
    async fn update_paddon_management_settings(&self, _free_movement_enabled: Option<bool>, _worker_visibility_enabled: Option<bool>, _actor: &QueueActionActor) -> StoreResult<PaddonManagementSettings> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn selectable_rezka_paddons(&self, limit: usize) -> StoreResult<Vec<PaddonSummary>> {
        Ok(self.paddons(200).await?.into_iter().filter(|p| p.locked_at_unix.is_none()).take(limit).collect())
    }
    async fn active_rezka_paddon(&self, _apparatus: &str, _actor: &QueueActionActor) -> StoreResult<Option<String>> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn set_active_rezka_paddon(&self, _apparatus: &str, _actor: &QueueActionActor, _code: Option<&str>) -> StoreResult<()> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn paddon_summary(&self, _code: &str) -> StoreResult<Option<PaddonSummary>> {
        Ok(None)
    }
    async fn create_paddon(&self, _input: PaddonCreateInput) -> StoreResult<PaddonSummary> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn confirm_paddon_print(&self, _code: &str, _actor: &QueueActionActor) -> StoreResult<PaddonPrintConfirmation> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn can_unlock_paddon(&self, _code: &str, _actor: &QueueActionActor) -> StoreResult<bool> {
        Ok(false)
    }
    async fn unlock_paddon(&self, _code: &str, _actor: &QueueActionActor) -> StoreResult<PaddonSummary> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn create_active_paddon_successor(&self, _code: &str, _apparatus: &str, _actor: &QueueActionActor) -> StoreResult<PaddonSummary> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn delete_paddon(&self, _code: &str) -> StoreResult<()> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn paddon_snapshot(&self, _code: &str) -> StoreResult<Option<PaddonSnapshot>> {
        Ok(None)
    }
    async fn paddon_scan_snapshot(&self, _code: &str) -> StoreResult<Option<PaddonSnapshot>> {
        Ok(None)
    }
    async fn paddon_snapshot_for_apparatuses(
        &self, code: &str, actor: &QueueActionActor, allowed_apparatuses: &[String],
    ) -> StoreResult<Option<PaddonSnapshot>> {
        let mut snapshot = self.paddon_snapshot(code).await?;
        if let Some(snapshot) = &mut snapshot {
            if actor.role == "aparatchi" && !allowed_apparatuses.contains(&snapshot.paddon.apparatus_id) {
                return Err(ProductionMapError::PaddonTransferForbidden);
            }
            snapshot.available_items.retain(|batch| allowed_apparatuses.contains(&batch.apparatus));
        }
        Ok(snapshot)
    }
    /// The apparatus scope comes from authenticated server-side assignments.
    async fn add_paddon_items_for_apparatuses(
        &self, _code: &str, _ids: &[String], _actor: &QueueActionActor, _allowed: &[String],
    ) -> StoreResult<PaddonSnapshot> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn transfer_paddon_items_for_apparatuses(
        &self, _input: &PaddonTransferInput, _actor: &QueueActionActor, _allowed: &[String],
    ) -> StoreResult<()> {
        Err(ProductionMapError::StoreFailed)
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
    async fn transfer_paddon_items(
        &self,
        _input: &PaddonTransferInput,
        _actor: &QueueActionActor,
    ) -> StoreResult<()> {
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
    async fn warehouse_wip_snapshot(&self, _qr_payload: &str) -> StoreResult<Option<super::WarehouseWipSnapshot>> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn receive_warehouse_wip(&self, _write: super::WarehouseWipReceiveWrite) -> StoreResult<()> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        _stock: FinishedGoodsStockEntry,
    ) -> StoreResult<()> {
        self.put_order_progress_batch(batch).await
    }
    async fn paddon_receipt(&self, _code: &str) -> StoreResult<Option<PaddonReceipt>> {
        Ok(None)
    }
    async fn receive_paddon(&self, _write: PaddonReceiveWrite) -> StoreResult<PaddonReceipt> {
        Err(ProductionMapError::StoreFailed)
    }
    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: &QueueActionProgressWrite,
    ) -> StoreResult<QueueActionProgressWriteResult> {
        validate_queue_progress_write(write)?;
        let write = write.clone();
        if let Some(hold_id) = write.print_preflight_hold_id.as_deref() {
            self.consume_print_preflight_hold(
                hold_id,
                &write.event.order_id,
                &write.apparatus,
                &write.event.actor,
            )
            .await?;
        }
        if let Some(hold_id) = write.print_preflight_cancel_hold_id.as_deref() {
            self.cancel_print_preflight_hold(
                hold_id,
                &write.event.order_id,
                &write.apparatus,
                &write.event.actor,
            )
            .await?;
        }
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
    /// Only live material reservations follow unfinished printing work.
    async fn raw_material_assignments_for_transfer(&self, order_id: &str, apparatus: &str) -> StoreResult<Vec<RawMaterialAssignment>> {
        Ok(self.raw_material_assignments_for_order(order_id).await?.into_iter()
            .filter(|assignment| assignment.apparatus_id.as_str() == apparatus).collect())
    }
    async fn in_use_materials_at_apparatus(
        &self,
        _order_id: &str,
        _apparatus: &str,
    ) -> StoreResult<Vec<String>> {
        Ok(Vec::new())
    }
    async fn raw_material_assignment_by_barcode(&self, barcode: &str) -> StoreResult<Option<RawMaterialAssignment>> {
        Ok(self.raw_material_assignments().await?.into_iter()
            .find(|assignment| assignment.barcode.trim().eq_ignore_ascii_case(barcode.trim())))
    }
    async fn raw_material_assignments_for_order(&self, order_id: &str) -> StoreResult<Vec<RawMaterialAssignment>> {
        Ok(self.raw_material_assignments().await?.into_iter()
            .filter(|assignment| assignment.order_id.trim() == order_id.trim()).collect())
    }
    async fn raw_material_assignments_for_orders(&self, order_ids: &[String]) -> StoreResult<Vec<RawMaterialAssignment>> {
        let requested = order_ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
        if requested.is_empty() { return Ok(Vec::new()); }
        Ok(self.raw_material_assignments().await?.into_iter()
            .filter(|assignment| requested.contains(assignment.order_id.trim())).collect())
    }
    async fn opening_wip_records_for_orders(&self, order_ids: &[String]) -> StoreResult<Vec<OpeningWipRecord>> {
        let requested = order_ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
        if requested.is_empty() { return Ok(Vec::new()); }
        Ok(self.opening_wip_records(OpeningWipQuery {
            order_id: String::new(), wip_status: None, limit: 100_000,
        }).await?.into_iter()
            .filter(|record| requested.contains(record.intake.order_id.trim())).collect())
    }
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
