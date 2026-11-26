/// Reuse the authoritative queue/session/output persistence in a larger atomic
/// business transaction. The caller owns the commit boundary.
pub(super) async fn apply_queue_progress_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        validate_queue_progress_write(write)?;
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
            // Assigned peers are map metadata, not resources mutated by a normal card.
            if write.event.action == crate::core::production_map::queue_state::ApparatusQueueAction::Freeze
                || write.map_update.is_some() {
                locked_apparatuses.extend(write.event.assigned_apparatus.iter().map(String::as_str));
            }
            locked_apparatuses.extend(write.sequence_updates.keys().map(String::as_str));
            if let Some(session) = &write.session {
                locked_apparatuses.push(session.apparatus.as_str());
            }
            if let Some(event) = &write.progress_event {
                locked_apparatuses.push(event.apparatus.as_str());
            }
            // WIP producer/nominal next IDs describe route history, not queues
            // being changed. The order lock, batch row ownership lock and CAS
            // protect those rolls without blocking unrelated peer machines.
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
            lock_orders_and_apparatuses_tx(tx, &locked_orders, &locked_apparatuses).await?;
        }
        if queue_action_event_replay_tx(tx, &write.event).await? {
            let raw_material_stock_committed = !write.raw_material_stock_transitions.is_empty();
            return Ok(QueueActionProgressWriteResult {
                raw_material_stock_committed,
                ..QueueActionProgressWriteResult::default()
            });
        }
        if let Some(raw) = write.event.payload_json.get("qolip_confirmation") {
            let confirmation: crate::core::production_map::AutomaticQolipConfirmation = serde_json::from_value(raw.clone())
                .map_err(|_| ProductionMapError::QueueActionNotAllowed)?;
            let released_resume = write.event.action == crate::core::production_map::queue_state::ApparatusQueueAction::Resume
                && write.event.payload_json.get("qolip_reacquired_after_freeze")
                    .and_then(serde_json::Value::as_bool) == Some(true);
            if (write.event.action != crate::core::production_map::queue_state::ApparatusQueueAction::Start
                && !released_resume) || !confirmation.is_valid() {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            qolip_start_settings::require_automatic_tx(tx, &confirmation).await?;
        }
        order_control_helpers::validate_execution_controls_tx(tx, write).await?;
        qolip_returns::validate_returned_qolip_resume_tx(tx, write).await?;
        if matches!(write.event.action,
            crate::core::production_map::queue_state::ApparatusQueueAction::Start
                | crate::core::production_map::queue_state::ApparatusQueueAction::Resume)
        {
            qolip_rework_availability::require_order_qolips_available_tx(
                tx, &write.event.order_id,
            ).await?;
        }
        if let Some(hold_id) = write.print_preflight_hold_id.as_deref() {
            consume_print_preflight_hold_tx(
                tx,
                hold_id,
                &write.event.order_id,
                &write.apparatus,
                &write.event.actor,
            )
            .await?;
        }
        if let Some(hold_id) = write.print_preflight_cancel_hold_id.as_deref() {
            cancel_print_preflight_hold_tx(
                tx,
                hold_id,
                &write.event.order_id,
                &write.apparatus,
                &write.event.actor,
            )
            .await?;
        }
        validate_queue_action_event_transition_tx(tx, &write.event).await?;
        if write.event.payload_json["print_transfer_materials_reused"].as_bool() == Some(true) {
            let session = write.session.as_ref().ok_or(ProductionMapError::RawMaterialScanRequired)?;
            material_helpers::validate_reused_print_materials_tx(tx, session).await?;
        }
        re_pause::validate_resume_checkpoint_tx(tx, write).await?;
        temporary_pause::validate_write_tx(tx, write).await?;
        stage_execution::validate_input_claims_tx(tx, write).await?;
        let output_paddon = output_paddon_assignment::lock_for_output(tx, write).await?;
        if let Some(expected) = write.event.payload_json.get("bosma_closing_expected_payload") {
            let session_id = write.event.payload_json.get("bosma_closing_session_id")
                .and_then(serde_json::Value::as_str).unwrap_or_default();
            let current = sqlx::query_as::<_, (String, serde_json::Value)>(
                "SELECT status, payload_json FROM mini_order_run_sessions
                 WHERE session_id = $1 AND order_id = $2 AND canonical_apparatus_id = $3 FOR UPDATE",
            ).bind(session_id).bind(&write.event.order_id).bind(&write.apparatus)
                .fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?
                .ok_or(ProductionMapError::QueueActionNotAllowed)?;
            if current.0 != "roll_detached" || current.1 != *expected {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
        }
        if let Some(expected) = write.event.payload_json.get("rezka_expected_output_revision") {
            let session_id = write.event.payload_json.get("rezka_expected_session_id")
                .and_then(serde_json::Value::as_str).unwrap_or_default();
            let expected_bobina_mode = write.event.payload_json
                .get("rezka_expected_bobina_mode");
            let (current, bobina_mode) = sqlx::query_as::<_, (serde_json::Value, Option<bool>)>(
                "SELECT payload_json, CASE WHEN $3 THEN (
                    SELECT (map_json->>'include_bobina_in_net')::boolean
                    FROM mini_production_maps WHERE id = $2
                 ) END FROM mini_order_run_sessions
                 WHERE session_id = $1 AND order_id = $2 FOR UPDATE",
            ).bind(session_id).bind(&write.event.order_id)
                .bind(expected_bobina_mode.is_some())
                .fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?
                .ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
            if current.get("rezka_output_revision").cloned()
                .unwrap_or_else(|| serde_json::json!(0)) != *expected
            {
                return Err(ProductionMapError::RezkaOutputCycleConflict);
            }
            if expected_bobina_mode.is_some_and(|expected| expected.as_bool() != bobina_mode) {
                return Err(ProductionMapError::RezkaBobinaModeChanged(bobina_mode.unwrap_or(false)));
            }
        }
        validate_merge_session_transition_tx(tx, &write.event, write.session.as_ref()).await?;
        for batch in &write.opening_wip_batch_updates {
            opening_wip_helpers::validate_opening_remainder_resume_tx(tx, write, batch).await?;
        }
        let start_checkpoint = cancel_start::before_start_tx(tx, write).await?;
        let scanned_input = scanned_wip::before_claim_tx(tx, write).await?;
        let raw_material_stock_committed = !write.raw_material_stock_transitions.is_empty();
        let raw_material_outcome = raw_material_stock_helpers::apply_raw_material_stock_transitions_for_event_tx(
            tx,
            &write.raw_material_stock_transitions,
            &write.event.actor,
            &write.event.apparatus,
            start_checkpoint.as_ref().map(|_| write.event.event_id.as_str()),
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
            put_map_inner_tx(tx, map).await?;
        }
        if let Some(session) = &write.session {
            // Composition writers take the exclusive form before mold locks.
            // Keep the effective requirement stable through session commit.
            sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended('qolip:product-composition', 0))")
                .execute(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
            reject_qolip_in_use_tx(tx, session).await?;
            if matches!(event.action, crate::core::production_map::queue_state::ApparatusQueueAction::Start)
                || event.payload_json.get("qolip_reacquired_after_freeze")
                    .and_then(serde_json::Value::as_bool) == Some(true) {
                if event.payload_json.get("qolip_confirmation").is_some() {
                    qolip_session_helpers::validate_automatic_qolip_set_tx(tx, session).await?;
                } else {
                    // Only this command's confirmation activates automatic
                    // policy validation; session history alone never does.
                    qolip_session_helpers::validate_qolip_set_tx(tx, session).await?;
                }
            }
        }
        put_queue_action_state_tx(tx, event).await?;
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
                tx,
                sequence_apparatus,
                &event.order_id,
                order_ids,
                remove_order_from_sequence,
                append_order_to_sequence,
            )
            .await?;
        }
        insert_queue_action_event_tx(tx, event).await?;
        if let Some(status) = write.schedule_reservation_status {
            let apparatus_id = ApparatusId::new(write.apparatus.trim().to_string())
                .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
            update_apparatus_schedule_reservation_status_tx(
                tx,
                &event.order_id,
                &apparatus_id,
                status,
                &event.actor,
            )
            .await?;
        }
        if let Some(session) = &write.session {
            let mut stored_session = session.clone();
            if (event.action == crate::core::production_map::queue_state::ApparatusQueueAction::Start
                || event.payload_json.get("lamination_raw_reacquisition")
                    .and_then(serde_json::Value::as_bool) == Some(true)
                || event.payload_json.get("print_transfer_material_reacquisition")
                    .and_then(serde_json::Value::as_bool) == Some(true))
                && let Some(mut resources) = ProductionQrSessionResources::recorded(session)
                && resources.raw_materials_available
                && !raw_material_outcome.started_materials.is_empty()
                && resources.raw_materials.len() == raw_material_outcome.started_materials.len()
                && resources.raw_materials.iter().all(|material| {
                    raw_material_outcome.started_materials.iter().any(|stock| {
                        stock.barcode.trim().eq_ignore_ascii_case(material.barcode.trim())
                    })
                })
            {
                // Metadata from the exact locked stock rows used by this claim,
                // in its existing transaction. No new consumption is recorded.
                resources.raw_materials = raw_material_outcome.started_materials;
                resources.write_to_session(&mut stored_session);
            }
            if event.payload_json.get("stage_work_report_submitted").and_then(serde_json::Value::as_bool) == Some(true) {
                stage_execution::stamp_report_tx(tx, &mut stored_session, &event.event_id, &event.actor).await?;
            }
            if stored_session.status == crate::core::production_map::OrderRunStatus::Completed
                && event.action == crate::core::production_map::queue_state::ApparatusQueueAction::Complete {
                if let Some(previous) = sqlx::query_scalar::<_, serde_json::Value>("SELECT payload_json FROM mini_order_run_sessions WHERE session_id=$1 FOR UPDATE")
                    .bind(&stored_session.session_id).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)? {
                    stored_session.payload_json["_completion_reactivation_payload"] = previous;
                }
                stored_session.payload_json["_completion_queue_event_id"] = serde_json::json!(event.event_id);
            }
            put_order_run_session_tx(tx, &stored_session).await?;
        }
        if let Some(progress_event) = &write.progress_event {
            put_order_progress_event_tx(tx, progress_event).await?;
            waste::record_progress(tx, progress_event, &write.event.actor).await?;
        }
        if write.progress_batches.is_empty() {
            if let Some(batch) = &write.progress_batch {
                put_order_progress_batch_tx(tx, batch).await?;
            }
        } else {
            for batch in &write.progress_batches {
                put_order_progress_batch_tx(tx, batch).await?;
            }
        }
        for batch in &write.progress_batch_updates {
            put_order_progress_batch_tx(tx, batch).await?;
        }
        if let Some(paddon_id) = output_paddon {
            output_paddon_assignment::assign_outputs(tx, &paddon_id, write).await?;
        }
        for batch in &write.opening_wip_batch_updates {
            update_opening_wip_batch_tx(tx, batch).await?;
        }
        if let Some(record) = &write.order_control_update {
            save_order_control_state_tx(tx, record).await?;
        }
        for checkout in &write.qolip_checkouts {
            super::postgres_qolip::save_checkout_tx(
                tx,
                checkout,
                current_session_id,
            )
            .await
            .map_err(production_map_qolip_checkout_error)?;
        }
        if event.payload_json.get("qolip_reacquired_after_freeze")
            .and_then(serde_json::Value::as_bool) == Some(true)
            && let Some(session) = &write.session {
            qolip_session_helpers::validate_reacquired_qolip_checkouts_tx(tx, session).await?;
        }
        if let Some(report) = &write.returned_paint_report {
            super::postgres_returned_paint::insert_returned_paint_request_tx(tx, report)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        cancel_start::after_start_tx(tx, write, start_checkpoint).await?;
        scanned_wip::after_claim_tx(tx, write, scanned_input).await?;
        order_control_helpers::finalize_global_stop_tx(tx, &event.order_id).await?;
        if !lifecycle::refresh_recorded_rezka_output_tx(tx, write).await? {
            refresh_production_order_lifecycle_tx(
                tx,
                &event.order_id,
                &event.actor,
                &event.event_id,
                "queue_action_progress",
            )
            .await?;
        }
        Ok(QueueActionProgressWriteResult {
            raw_material_stock_warehouses,
            raw_material_stock_committed,
        })
}
