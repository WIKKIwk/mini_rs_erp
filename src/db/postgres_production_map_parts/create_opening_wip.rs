impl PostgresProductionMapStore {

    async fn create_opening_wip(
        &self,
        write: OpeningWipCreateWrite,
    ) -> Result<OpeningWipRecord, ProductionMapError> {
        create_opening_wip(&self.pool, write).await
    }

    async fn delete_opening_wip_batch(
        &self,
        write: OpeningWipDeleteWrite,
    ) -> Result<OpeningWipBatchRecord, ProductionMapError> {
        delete_opening_wip_batch(&self.pool, write).await
    }

    async fn paddons(&self, limit: usize) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        load_paddons(&self.pool, limit, false, None, None).await
    }

    async fn paddon_summary(
        &self,
        code: &str,
    ) -> Result<Option<PaddonSummary>, ProductionMapError> {
        load_paddon_summary(&self.pool, code).await
    }

    async fn create_paddon(
        &self,
        input: PaddonCreateInput,
    ) -> Result<PaddonSummary, ProductionMapError> {
        create_paddon(&self.pool, input).await
    }

    async fn paddon_snapshot(
        &self,
        code: &str,
    ) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
        load_paddon_snapshot(&self.pool, code).await
    }

    async fn paddon_scan_snapshot(
        &self,
        code: &str,
    ) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
        load_paddon_scan_snapshot(&self.pool, code).await
    }

    async fn add_paddon_item(
        &self,
        code: &str,
        progress_batch_id: &str,
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        add_paddon_item(&self.pool, code, progress_batch_id, actor).await
    }

    async fn add_paddon_items(
        &self,
        code: &str,
        progress_batch_ids: &[String],
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        add_paddon_items(&self.pool, code, progress_batch_ids, actor).await
    }

    async fn remove_paddon_item(
        &self,
        code: &str,
        progress_batch_id: &str,
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        remove_paddon_item(&self.pool, code, progress_batch_id, actor).await
    }

    async fn remove_paddon_items(
        &self,
        code: &str,
        progress_batch_ids: &[String],
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        remove_paddon_items(&self.pool, code, progress_batch_ids, actor).await
    }

    async fn put_order_run_session(
        &self,
        session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        put_order_run_session(&self.pool, &session).await
    }

    async fn put_order_progress_event(
        &self,
        event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        put_order_progress_event(&self.pool, &event).await
    }

    async fn put_order_progress_batch(
        &self,
        batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        put_order_progress_batch(&self.pool, &batch).await
    }

    async fn apparatus_transfer_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        load_apparatus_transfer_by_idempotency_key(&self.pool, idempotency_key).await
    }

    async fn commit_apparatus_transfer(
        &self,
        write: ProductionMapApparatusTransferWrite,
    ) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
        commit_apparatus_transfer_record(&self.pool, write).await
    }

    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        stock: FinishedGoodsStockEntry,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        receive_finished_goods_batch_tx(&mut tx, &batch, &stock).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: &QueueActionProgressWrite,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        let mut tx = self.pool.begin().await.map_err(|_| ProductionMapError::StoreFailed)?;
        let result = apply_queue_progress_tx(&mut tx, write).await?;
        tx.commit().await.map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(result)
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        load_raw_material_assignments(&self.pool).await
    }

    async fn raw_material_assignment_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        material_helpers::load_raw_material_assignment_by_barcode(&self.pool, barcode).await
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        save_raw_material_assignment(&self.pool, assignment).await
    }

    async fn receive_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
        actor: &QueueActionActor,
    ) -> Result<Vec<String>, ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        lock_order_and_apparatuses_tx(
            &mut tx,
            &assignment.order_id,
            &[assignment.apparatus_id.as_str()],
        )
        .await?;
        let intake_allowed = sqlx::query_scalar::<_, String>(
            "SELECT state
             FROM mini_queue_states
             WHERE canonical_apparatus_id = $1
               AND order_id = $2
             FOR UPDATE",
        )
        .bind(assignment.apparatus_id.as_str())
        .bind(assignment.order_id.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .and_then(|state| {
            crate::core::production_map::queue_state::ApparatusQueueOrderState::parse(&state)
        })
        .is_some_and(
            crate::core::production_map::queue_state::ApparatusQueueOrderState::allows_material_intake,
        );
        if !intake_allowed {
            return Err(ProductionMapError::RawMaterialOrderNotActive);
        }
        let control_state = sqlx::query_scalar::<_, String>(
            "SELECT state
             FROM mini_order_control_states
             WHERE order_id = $1
             FOR UPDATE",
        )
        .bind(assignment.order_id.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(control_state) = control_state {
            match crate::core::production_map::OrderControlState::parse(&control_state)
                .ok_or(ProductionMapError::StoreFailed)?
            {
                crate::core::production_map::OrderControlState::Active => {}
                crate::core::production_map::OrderControlState::FreezeRequested => {
                    return Err(ProductionMapError::OrderFreezeRequested);
                }
                crate::core::production_map::OrderControlState::Frozen => {
                    return Err(ProductionMapError::OrderFrozen);
                }
            }
        }
        let existing_assignment = sqlx::query_as::<_, (String, String)>(
            "SELECT canonical_apparatus_id, order_id
             FROM mini_raw_material_assignments
             WHERE lower(barcode) = lower($1)
             FOR UPDATE",
        )
        .bind(assignment.barcode.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .map(|(apparatus_id, order_id)| {
            let apparatus_id = crate::core::apparatus_standard::ApparatusId::new(apparatus_id)
                .map_err(|_| ProductionMapError::StoreFailed)?;
            Ok((apparatus_id, order_id))
        })
        .transpose()?;
        match existing_assignment {
            Some((existing_apparatus, existing_order_id))
                if existing_order_id.trim() == assignment.order_id.trim()
                    && existing_apparatus == assignment.apparatus_id => {}
            Some(_) => return Err(ProductionMapError::RawMaterialAlreadyAssigned),
            None => return Err(ProductionMapError::RawMaterialAssignmentNotFound),
        }
        let stock_available = sqlx::query_scalar::<_, bool>(
            "SELECT lower(status) = 'available'
                    AND COALESCE(reserved_order_id, '') = ''
             FROM mini_raw_material_stock
             WHERE lower(barcode) = lower($1)
             FOR UPDATE",
        )
        .bind(assignment.barcode.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if stock_available != Some(true) {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
        let warehouses = worker_material_use::receive_assigned_material_tx(
            &mut tx, &assignment, actor,
        ).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(warehouses)
    }

    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        delete_raw_material_assignment(&self.pool, order_id, barcode).await
    }
}


include!("../postgres_production_map_trait_impl.rs");

fn production_map_qolip_checkout_error(error: QolipError) -> ProductionMapError {
    match error {
        QolipError::LocationNotFound => ProductionMapError::QolipLocationNotFound,
        QolipError::QolipCodeNotFound | QolipError::QolipCodeMismatch => {
            ProductionMapError::QolipCodeMismatch
        }
        QolipError::InsufficientStock => ProductionMapError::QolipInsufficientStock,
        QolipError::LocationIdentityMismatch => ProductionMapError::QolipLocationIdentityMismatch,
        QolipError::QolipInUse => ProductionMapError::QolipAlreadyInUse,
        QolipError::QolipInRework => ProductionMapError::QolipInRework,
        _ => ProductionMapError::StoreFailed,
    }
}
