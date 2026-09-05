#[path = "capacity/memory_store.rs"]
mod capacity;
#[path = "catalog/memory_store.rs"]
mod maps;
#[path = "materials/memory_store.rs"]
mod materials;
mod queue;
#[path = "progress_session/runs.rs"]
mod runs;
mod state;
mod transfers;

use super::*;

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use async_trait::async_trait;

use crate::core::apparatus_standard::ApparatusId;

pub use state::MemoryProductionMapStore;

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
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        runs::active_order_run_sessions_for_worker(self, worker_refs, worker_display_name, limit)
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

    async fn progress_batches_for_audit(
        &self,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        Ok(self
            .order_progress_batches
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn apparatus_transfers_for_audit(
        &self,
    ) -> Result<Vec<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        Ok(self
            .apparatus_transfers
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn wip_progress_batches(
        &self,
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        runs::wip_progress_batches(self, query).await
    }

    async fn put_order_run_session(
        &self,
        session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        runs::put_order_run_session(self, session).await
    }

    async fn put_order_progress_event(
        &self,
        event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        runs::put_order_progress_event(self, event).await
    }
}

impl MemoryProductionMapStore {

    async fn opening_wip_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<OpeningWipRecord>, ProductionMapError> {
        Ok(self
            .opening_wip_records
            .read()
            .await
            .values()
            .find(|record| record.intake.idempotency_key.trim() == idempotency_key.trim())
            .cloned())
    }

    async fn opening_wip_records(
        &self,
        query: OpeningWipQuery,
    ) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
        let mut records = self
            .opening_wip_records
            .read()
            .await
            .values()
            .filter(|record| {
                (query.order_id.trim().is_empty()
                    || record.intake.order_id.trim() == query.order_id.trim())
                    && query.wip_status.is_none_or(|status| {
                        record.batches.iter().any(|batch| batch.wip_status == status)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .intake
                .created_at_unix
                .cmp(&left.intake.created_at_unix)
                .then_with(|| right.intake.intake_id.cmp(&left.intake.intake_id))
        });
        records.truncate(query.limit.max(1));
        Ok(records)
    }

    async fn create_opening_wip(
        &self,
        write: OpeningWipCreateWrite,
    ) -> Result<OpeningWipRecord, ProductionMapError> {
        let mut records = self.opening_wip_records.write().await;
        if let Some(existing) = records.values().find(|record| {
            record.intake.idempotency_key.trim()
                == write.record.intake.idempotency_key.trim()
        }) {
            return if existing.intake.request_fingerprint
                == write.record.intake.request_fingerprint
            {
                Ok(existing.clone())
            } else {
                Err(ProductionMapError::OpeningWipIdempotencyConflict)
            };
        }
        records.insert(
            write.record.intake.intake_id.trim().to_string(),
            write.record.clone(),
        );
        Ok(write.record)
    }

    async fn opening_wip_batch(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Result<Option<OpeningWipBatchRecord>, ProductionMapError> {
        for record in self.opening_wip_records.read().await.values() {
            if let Some(batch) = record.batches.iter().find(|batch| {
                (!batch_id.trim().is_empty() && batch.batch_id.trim() == batch_id.trim())
                    || (!qr_payload.trim().is_empty()
                        && batch.qr_payload.trim() == qr_payload.trim())
            }) {
                return Ok(Some(OpeningWipBatchRecord {
                    intake: record.intake.clone(),
                    batch: batch.clone(),
                }));
            }
        }
        Ok(None)
    }

    async fn delete_opening_wip_batch(
        &self,
        write: OpeningWipDeleteWrite,
    ) -> Result<OpeningWipBatchRecord, ProductionMapError> {
        let batch_id = write.batch_id.trim();
        if batch_id.is_empty() {
            return Err(ProductionMapError::OpeningWipInvalidInput);
        }
        let mut records = self.opening_wip_records.write().await;
        let record = records
            .values_mut()
            .find(|record| {
                record
                    .batches
                    .iter()
                    .any(|batch| batch.batch_id.trim() == batch_id)
            })
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let batch_index = record
            .batches
            .iter()
            .position(|batch| batch.batch_id.trim() == batch_id)
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let current = &record.batches[batch_index];
        if current.wip_status == OpeningWipBatchStatus::Void {
            return Ok(OpeningWipBatchRecord {
                intake: record.intake.clone(),
                batch: current.clone(),
            });
        }
        if record.intake.status != OpeningWipIntakeStatus::Confirmed
            || current.wip_status != OpeningWipBatchStatus::Waiting
            || !current.used_by_session_id.trim().is_empty()
            || !current.used_by_apparatus.trim().is_empty()
            || !current.processed_by_session_id.trim().is_empty()
            || !current.processed_by_apparatus.trim().is_empty()
        {
            return Err(ProductionMapError::OpeningWipDeleteLocked);
        }
        let payload_uses_batch = |payload: &serde_json::Value| {
            payload
                .get("input_wip_source_kind")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|source| source.eq_ignore_ascii_case("opening_wip"))
                && payload
                    .get("input_progress_batch_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value.trim() == batch_id)
        };
        let has_lineage = self
            .order_run_sessions
            .read()
            .await
            .values()
            .any(|session| payload_uses_batch(&session.payload_json))
            || self
                .order_progress_events
                .read()
                .await
                .iter()
                .any(|event| payload_uses_batch(&event.payload_json))
            || self
                .order_progress_batches
                .read()
                .await
                .values()
                .any(|batch| batch.parent_batch_id.trim() == batch_id);
        if has_lineage {
            return Err(ProductionMapError::OpeningWipDeleteLocked);
        }

        let batch = &mut record.batches[batch_index];
        batch.wip_status = OpeningWipBatchStatus::Void;
        batch.updated_at_unix = write.deleted_at_unix;
        if record
            .batches
            .iter()
            .all(|batch| batch.wip_status == OpeningWipBatchStatus::Void)
        {
            record.intake.status = OpeningWipIntakeStatus::Cancelled;
            record.intake.updated_at_unix = write.deleted_at_unix;
        }
        Ok(OpeningWipBatchRecord {
            intake: record.intake.clone(),
            batch: record.batches[batch_index].clone(),
        })
    }

    async fn put_order_progress_batch(
        &self,
        batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        runs::put_order_progress_batch(self, batch).await
    }

    async fn apparatus_transfer_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        transfers::apparatus_transfer_by_idempotency_key(self, idempotency_key).await
    }

    async fn commit_apparatus_transfer(
        &self,
        write: ProductionMapApparatusTransferWrite,
    ) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
        transfers::commit_apparatus_transfer(self, write).await
    }

    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: &QueueActionProgressWrite,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        validate_queue_progress_write(write)?;
        if let Some(expected) = write.event.payload_json.get("rezka_expected_output_revision") {
            let current = self.active_order_run_session(&write.apparatus, &write.event.order_id)
                .await?.ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
            if Some(current.session_id.as_str()) != write.event.payload_json
                .get("rezka_expected_session_id").and_then(serde_json::Value::as_str)
                || current.payload_json.get("rezka_output_revision").cloned()
                    .unwrap_or_else(|| serde_json::json!(0)) != *expected
            {
                return Err(ProductionMapError::RezkaOutputCycleConflict);
            }
        }
        let write = write.clone();
        if self
            .fail_next_queue_progress_commit
            .swap(false, Ordering::SeqCst)
        {
            return Err(ProductionMapError::StoreFailed);
        }
        if let Some(session) = &write.session
            && matches!(
                session.status,
                OrderRunStatus::Active
                    | OrderRunStatus::Paused
                    | OrderRunStatus::Frozen
                    | OrderRunStatus::RollDetached
            )
        {
            for qolip_code in runs::session_qolip_codes(session) {
                if self
                    .active_order_run_session_for_qolip(&qolip_code)
                    .await?
                    .is_some_and(|active| active.session_id != session.session_id)
                {
                    return Err(ProductionMapError::QolipAlreadyInUse);
                }
            }
        }
        if let Some(map) = write.map_update.clone() {
            maps::put_map(self, map).await?;
        }
        let schedule_reservation_status = write.schedule_reservation_status;
        let sequence_updates = write.sequence_updates;
        let event_order_id = write.event.order_id.clone();
        let event_actor = write.event.actor.clone();
        let event_apparatus = write.apparatus.clone();
        if let Some(session) = write.session {
            self.put_order_run_session(session).await?;
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

        if let Some(event) = write.progress_event {
            self.put_order_progress_event(event).await?;
        }
        for (apparatus, order_ids) in sequence_updates {
            self.put_apparatus_sequence(&apparatus, order_ids).await?;
        }
        self.put_apparatus_queue_states_with_event(&write.apparatus, write.states, write.event)
            .await?;
        if !write.opening_wip_batch_updates.is_empty() {
            let mut records = self.opening_wip_records.write().await;
            for update in write.opening_wip_batch_updates {
                let record = records
                    .get_mut(update.intake_id.trim())
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                let current = record
                    .batches
                    .iter_mut()
                    .find(|batch| batch.batch_id.trim() == update.batch_id.trim())
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                let valid_transition = match update.wip_status {
                    OpeningWipBatchStatus::InUse => {
                        current.wip_status == OpeningWipBatchStatus::Waiting
                    }
                    OpeningWipBatchStatus::Processed => {
                        current.wip_status == OpeningWipBatchStatus::InUse
                            && current.used_by_session_id.trim()
                                == update.used_by_session_id.trim()
                    }
                    OpeningWipBatchStatus::Waiting => {
                        current.wip_status == OpeningWipBatchStatus::InUse
                    }
                    OpeningWipBatchStatus::Void => false,
                };
                if !valid_transition {
                    return Err(ProductionMapError::ProgressBatchNotAccepted);
                }
                *current = update;
            }
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
        if let Some(report) = write.returned_paint_report {
            self.returned_paint_requests
                .write()
                .await
                .entry(report.id.clone())
                .or_insert(report);
        }
        Ok(QueueActionProgressWriteResult::default())
    }

    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        stock: FinishedGoodsStockEntry,
    ) -> Result<(), ProductionMapError> {
        runs::receive_finished_goods_batch(self, batch, stock).await
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        materials::raw_material_assignments(self).await
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        materials::put_raw_material_assignment(self, assignment).await
    }

    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        materials::delete_raw_material_assignment(self, order_id, barcode).await
    }
}


include!("memory_store_trait_impl.rs");
