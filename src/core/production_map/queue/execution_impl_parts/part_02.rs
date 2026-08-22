impl ProductionMapService {

    #[allow(clippy::too_many_arguments)]
    async fn has_unprocessed_previous_wips(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
        all_states: &ApparatusQueueStateMap,
        progress_batch_updates: &[OrderProgressBatch],
        ignored_batch_id: &str,
    ) -> Result<bool, ProductionMapError> {
        let Some(previous_apparatus) = chain::previous_work_stage_station(order_map, apparatus)
        else {
            return Ok(false);
        };
        let requires_previous_stage_completion = apparatus::is_laminatsiya_apparatus(canonical)
            || apparatus::is_rezka_apparatus(canonical);
        let previous_stage_completed = all_states.iter().any(|(candidate, states)| {
            super::super::types::apparatus_ids_match(candidate, &previous_apparatus)
                && states
                    .get(order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Completed)
        });
        if requires_previous_stage_completion && !previous_stage_completed {
            return Ok(true);
        }
        let mut batches = self
            .store
            .progress_batches_for_order(order_id)
            .await?
            .into_iter()
            .map(|batch| (batch.batch_id.trim().to_string(), batch))
            .collect::<BTreeMap<_, _>>();
        for batch in progress_batch_updates {
            batches.insert(batch.batch_id.trim().to_string(), batch.clone());
        }
        Ok(batches
            .values()
            .filter(|batch| {
                batch.order_id.trim() == order_id.trim()
                    && super::super::types::apparatus_ids_match(
                        &batch.apparatus,
                        &previous_apparatus,
                    )
                    && chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus)
            })
            .any(|batch| {
                if !ignored_batch_id.trim().is_empty()
                    && batch.batch_id.trim() == ignored_batch_id.trim()
                {
                    return false;
                }
                batch.wip_status == OrderProgressBatchWipStatus::Waiting
                    || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && super::super::types::apparatus_ids_match(
                            &batch.used_by_apparatus,
                            apparatus,
                        ))
                    || wip_batch_was_consumed_by_producer(batch)
            }))
    }

    async fn completion_input_batch_id(
        &self,
        apparatus: &str,
        order_id: &str,
        progress: &QueueProgressInput,
    ) -> Result<String, ProductionMapError> {
        if !progress.progress_batch_id.trim().is_empty() {
            return Ok(progress.progress_batch_id.trim().to_string());
        }
        if !progress.qr_payload.trim().is_empty() {
            return Ok(self
                .store
                .progress_batch_by_qr(progress.qr_payload.trim())
                .await?
                .map(|batch| batch.batch_id)
                .unwrap_or_default());
        }
        let Some(session) = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
        else {
            return Ok(String::new());
        };
        Ok(session_progress_links(&session).batch_id)
    }

    pub(crate) async fn commit_prepared_queue_action(
        &self,
        prepared: PreparedApparatusQueueAction,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.commit_prepared_queue_action_with_raw_material_stock(
            prepared,
            Vec::new(),
            Vec::new(),
            None,
        )
        .await
    }

    pub(crate) async fn commit_prepared_queue_action_with_raw_material_stock(
        &self,
        prepared: PreparedApparatusQueueAction,
        raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
        qolip_checkouts: Vec<crate::core::qolip::QolipCheckout>,
        returned_paint_report: Option<crate::core::returned_paint::ReturnedPaintRequest>,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let order_id = prepared.event.order_id.clone();
        let map_update = prepared
            .claimed_alternative_map
            .as_ref()
            .map(|update| update.updated.clone());
        let schedule_reservation_status =
            schedule_reservation_status_for_action(prepared.event.action);
        let order_control = prepared.order_control_update.clone();
        let write_result = self
            .store
            .put_apparatus_queue_states_with_event_and_progress(QueueActionProgressWrite {
                apparatus: prepared.apparatus.clone(),
                map_update,
                states: prepared.states.clone(),
                sequence_updates: prepared.sequence_updates.clone(),
                event: prepared.event,
                session: prepared.session.clone(),
                progress_event: prepared.progress_event.clone(),
                progress_batch: prepared.progress_batch.clone(),
                progress_batches: prepared.progress_batches.clone(),
                progress_batch_updates: prepared.progress_batch_updates.clone(),
                raw_material_stock_transitions,
                qolip_checkouts,
                returned_paint_report,
                order_control_update: prepared.order_control_update.clone(),
                schedule_reservation_status,
            })
            .await?;
        let order_status = self.order_status_detail(&order_id).await?;
        self.notify_live();
        Ok(ApparatusQueueActionResult {
            states: prepared.states,
            order_status,
            order_control,
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
            progress_batches: prepared.progress_batches,
            raw_material_stock_warehouses: write_result.raw_material_stock_warehouses,
            qolip_checkout_committed: write_result.qolip_checkout_committed,
        })
    }
}
