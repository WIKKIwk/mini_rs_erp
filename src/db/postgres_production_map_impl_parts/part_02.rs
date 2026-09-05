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
        load_paddons(&self.pool, limit).await
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
        validate_queue_progress_write(write)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        {
            let mut locked_orders = vec![write.event.order_id.as_str()];
            locked_orders.extend(
                write
                    .raw_material_stock_transitions
                    .iter()
                    .filter(|transition| !transition.order_id.trim().is_empty())
                    .map(|transition| transition.order_id.as_str()),
            );
            locked_orders.extend(
                write
                    .progress_batch
                    .iter()
                    .chain(write.progress_batches.iter())
                    .chain(write.progress_batch_updates.iter())
                    .map(|batch| batch.order_id.as_str()),
            );
            locked_orders.extend(
                write
                    .opening_wip_batch_updates
                    .iter()
                    .map(|batch| batch.order_id.as_str()),
            );
            if let Some(record) = &write.order_control_update {
                locked_orders.push(record.order_id.as_str());
            }
            let mut locked_apparatuses =
                vec![write.apparatus.as_str(), write.event.apparatus.as_str()];
            locked_apparatuses.extend(write.event.assigned_apparatus.iter().map(String::as_str));
            locked_apparatuses.extend(write.sequence_updates.keys().map(String::as_str));
            if let Some(session) = &write.session {
                locked_apparatuses.push(session.apparatus.as_str());
            }
            if let Some(event) = &write.progress_event {
                locked_apparatuses.push(event.apparatus.as_str());
            }
            for batch in write
                .progress_batch
                .iter()
                .chain(write.progress_batches.iter())
                .chain(write.progress_batch_updates.iter())
            {
                locked_apparatuses.push(batch.apparatus.as_str());
                for value in [
                    batch.current_apparatus.as_str(),
                    batch.next_apparatus.as_str(),
                    batch.used_by_apparatus.as_str(),
                    batch.processed_by_apparatus.as_str(),
                ] {
                    if !value.trim().is_empty()
                        && !value.trim().to_ascii_lowercase().starts_with("warehouse:")
                    {
                        locked_apparatuses.push(value);
                    }
                }
            }
            for batch in &write.opening_wip_batch_updates {
                for value in [
                    batch.used_by_apparatus.as_str(),
                    batch.processed_by_apparatus.as_str(),
                ] {
                    if !value.trim().is_empty() {
                        locked_apparatuses.push(value);
                    }
                }
            }
            if let Some(report) = &write.returned_paint_report {
                locked_apparatuses.push(report.apparatus.as_str());
            }
            if let Some(record) = &write.order_control_update
                && let Some(request) = &record.freeze_request
                && !request.target_apparatus.trim().is_empty()
            {
                locked_apparatuses.push(request.target_apparatus.as_str());
            }
            lock_orders_and_apparatuses_tx(&mut tx, &locked_orders, &locked_apparatuses).await?;
        }
        if queue_action_event_replay_tx(&mut tx, &write.event).await? {
            let raw_material_stock_committed = !write.raw_material_stock_transitions.is_empty();
            tx.commit()
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            return Ok(QueueActionProgressWriteResult {
                raw_material_stock_committed,
                ..QueueActionProgressWriteResult::default()
            });
        }
        validate_queue_action_event_transition_tx(&mut tx, &write.event).await?;
        if let Some(expected) = write.event.payload_json.get("rezka_expected_output_revision") {
            let session_id = write.event.payload_json.get("rezka_expected_session_id")
                .and_then(serde_json::Value::as_str).unwrap_or_default();
            let current = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT payload_json FROM mini_order_run_sessions
                 WHERE session_id = $1 AND order_id = $2 FOR UPDATE",
            ).bind(session_id).bind(&write.event.order_id)
                .fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?
                .ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
            if current.get("rezka_output_revision").cloned()
                .unwrap_or_else(|| serde_json::json!(0)) != *expected
            {
                return Err(ProductionMapError::RezkaOutputCycleConflict);
            }
        }
        validate_merge_session_transition_tx(&mut tx, &write.event, write.session.as_ref()).await?;
        let raw_material_stock_committed = !write.raw_material_stock_transitions.is_empty();
        let raw_material_outcome = apply_raw_material_stock_transitions_tx(
            &mut tx,
            &write.raw_material_stock_transitions,
            &write.event.actor,
            &write.event.apparatus,
        )
        .await?;
        let mut augmented_event = None;
        if !raw_material_outcome.unused_unlinks.is_empty() {
            let mut event = write.event.clone();
            event.payload_json["unused_raw_material_unlinked_on_complete"] =
                serde_json::Value::Bool(true);
            event.payload_json["unused_raw_material_unlinks"] = serde_json::Value::Array(
                raw_material_outcome
                    .unused_unlinks
                    .iter()
                    .map(|unlink| {
                        serde_json::json!({
                            "barcode": unlink.barcode.as_str(),
                            "stock_status": unlink.stock_status.as_str(),
                            "reserved_order_id": unlink.reserved_order_id.as_str(),
                            "warehouse": unlink.warehouse.as_str(),
                        })
                    })
                    .collect(),
            );
            augmented_event = Some(event);
        }
        let event = augmented_event.as_ref().unwrap_or(&write.event);
        let raw_material_stock_warehouses = raw_material_outcome.warehouses;
        let current_session_id = write
            .session
            .as_ref()
            .map(|session| session.session_id.trim())
            .filter(|session_id| !session_id.is_empty());
        if let Some(map) = &write.map_update {
            put_map_inner_tx(&mut tx, map).await?;
        }
        if let Some(session) = &write.session {
            reject_qolip_in_use_tx(&mut tx, session).await?;
        }
        put_queue_action_state_tx(&mut tx, event).await?;
        let remove_order_from_sequence = matches!(
            event.action,
            crate::core::production_map::queue_state::ApparatusQueueAction::Freeze
        ) || event
            .payload_json
            .get("admin_unfreeze")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let append_order_to_sequence = event
            .payload_json
            .get("admin_unfreeze")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        for (sequence_apparatus, order_ids) in &write.sequence_updates {
            apply_apparatus_sequence_delta_tx(
                &mut tx,
                sequence_apparatus,
                &event.order_id,
                order_ids,
                remove_order_from_sequence,
                append_order_to_sequence,
            )
            .await?;
        }
        insert_queue_action_event_tx(&mut tx, event).await?;
        if let Some(status) = write.schedule_reservation_status {
            let apparatus_id = ApparatusId::new(write.apparatus.trim().to_string())
                .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
            update_apparatus_schedule_reservation_status_tx(
                &mut tx,
                &event.order_id,
                &apparatus_id,
                status,
                &event.actor,
            )
            .await?;
        }
        if let Some(session) = &write.session {
            put_order_run_session_tx(&mut tx, session).await?;
            super::postgres_qolip::return_completed_session_checkouts_tx(&mut tx, session)
                .await
                .map_err(production_map_qolip_checkout_error)?;
        }
        if let Some(progress_event) = &write.progress_event {
            put_order_progress_event_tx(&mut tx, progress_event).await?;
        }
        if write.progress_batches.is_empty() {
            if let Some(batch) = &write.progress_batch {
                put_order_progress_batch_tx(&mut tx, batch).await?;
            }
        } else {
            for batch in &write.progress_batches {
                put_order_progress_batch_tx(&mut tx, batch).await?;
            }
        }
        for batch in &write.progress_batch_updates {
            put_order_progress_batch_tx(&mut tx, batch).await?;
        }
        for batch in &write.opening_wip_batch_updates {
            update_opening_wip_batch_tx(&mut tx, batch).await?;
        }
        if let Some(record) = &write.order_control_update {
            save_order_control_state_tx(&mut tx, record).await?;
        }
        for checkout in &write.qolip_checkouts {
            super::postgres_qolip::save_checkout_tx(
                &mut tx,
                checkout,
                current_session_id,
            )
            .await
            .map_err(production_map_qolip_checkout_error)?;
        }
        if let Some(report) = &write.returned_paint_report {
            super::postgres_returned_paint::insert_returned_paint_request_tx(&mut tx, report)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        refresh_production_order_lifecycle_tx(
            &mut tx,
            &event.order_id,
            &event.actor,
            &event.event_id,
            "queue_action_progress",
        )
        .await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(QueueActionProgressWriteResult {
            raw_material_stock_warehouses,
            raw_material_stock_committed,
        })
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        load_raw_material_assignments(&self.pool).await
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
        let active_state = sqlx::query_scalar::<_, String>(
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
        .is_some_and(crate::core::production_map::queue_state::ApparatusQueueOrderState::is_active);
        if !active_state {
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
        let outcome = apply_raw_material_stock_transitions_tx(
            &mut tx,
            &[RawMaterialStockTransition::new(
                RawMaterialStockTransitionKind::InUse,
                vec![assignment.barcode.clone()],
                &assignment.order_id,
            )],
            actor,
            &assignment.apparatus,
        )
        .await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(outcome.warehouses)
    }

    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        delete_raw_material_assignment(&self.pool, order_id, barcode).await
    }
}
