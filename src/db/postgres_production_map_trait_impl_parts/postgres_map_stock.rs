postgres_map_execution! {
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

    async fn raw_material_assignments_for_transfer(
        &self, order_id: &str, apparatus: &str,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        material_helpers::load_transferable_raw_material_assignments(&self.pool, order_id, apparatus).await
    }

    async fn in_use_materials_at_apparatus(
        &self, order_id: &str, apparatus: &str,
    ) -> Result<Vec<String>, ProductionMapError> {
        material_helpers::in_use_materials_at_apparatus(
            &self.pool, order_id, apparatus, false,
        ).await
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
