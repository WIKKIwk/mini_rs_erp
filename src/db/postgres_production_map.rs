use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusCapacityProfile, ApparatusDowntime, ApparatusMaterialRule, ApparatusQueueActionEvent,
    ApparatusQueuePolicy, ApparatusScheduleCancelRequest, ApparatusScheduleCandidate,
    ApparatusScheduleReservation, CompletedQueueOrder, CompletionRequestDecision,
    CompletionRequestDecisionNotification, CompletionRequestNotification,
    CompletionRequestStateResolution, FinishedGoodsStockEntry, LaminatsiyaAstatkaReport,
    OrderControlRecord, OrderProgressBatch, OrderProgressEvent, OrderRunSession, PaddonCreateInput,
    PaddonSnapshot, PaddonSummary, ProductionMapApparatusTransferRecord,
    ProductionMapApparatusTransferWrite, ProductionMapDefinition, ProductionMapError,
    ProductionMapStorePort, ProductionOrderLogEntry, ProgressBatchCorrectionInput,
    ProgressBatchCorrectionRecord, QueueActionActor, QueueActionProgressWrite,
    QueueActionProgressWriteResult, RawMaterialAssignment, RawMaterialStockTransition,
    RawMaterialStockTransitionKind, RezkaAstatkaReport, WipProgressBatchQuery,
    validate_queue_progress_write,
};
use crate::core::qolip::QolipError;

mod astatka_helpers;
mod capacity_helpers;
mod catalog_helpers;
mod completion_helpers;
mod map_helpers;
mod material_helpers;
mod order_control_helpers;
mod order_query_helpers;
mod paddon_helpers;
mod progress_helpers;
mod qolip_session_helpers;
mod queue_helpers;
mod raw_material_stock_helpers;
mod transaction_locks;
mod transfer_helpers;
mod wip_query_helpers;

use self::astatka_helpers::{
    load_laminatsiya_astatka_reports_for_order, load_rezka_astatka_reports_for_order,
    put_laminatsiya_astatka_report, put_rezka_astatka_report,
};
use self::capacity_helpers::{
    cancel_apparatus_schedule_reservation, load_apparatus_capacity_profiles,
    load_apparatus_downtimes, load_apparatus_schedule_reservation_by_idempotency_key,
    load_apparatus_schedule_reservations, put_apparatus_capacity_profile, put_apparatus_downtime,
    put_apparatus_schedule_reservation, resolve_apparatus_identity,
    update_apparatus_schedule_reservation_status_tx,
};
use self::catalog_helpers::{
    apply_apparatus_sequence_delta_tx, delete_map_by_id, load_apparatus_queue_policies,
    load_apparatus_queue_states, load_apparatus_sequences, load_maps,
    save_apparatus_queue_policy, save_apparatus_sequence, save_apparatus_sequence_tx,
};
use self::completion_helpers::{
    load_completion_request_by_event_id, load_completion_request_decisions_for_actor,
    load_completion_requests, resolve_completion_request_decision as resolve_completion_request,
};
use self::map_helpers::{
    put_map_inner, put_map_inner_tx, reject_duplicate_order_number,
    reject_duplicate_order_number_tx, reject_order_number_immutable,
    reject_order_number_immutable_tx,
};
use self::material_helpers::{
    delete_raw_material_assignment, load_apparatus_material_rules, load_raw_material_assignments,
    save_apparatus_material_rule, save_raw_material_assignment,
};
use self::order_control_helpers::{
    load_order_control_states, load_order_freeze_requests_for_audit, save_order_control_state,
    save_order_control_state_tx,
};
use self::order_query_helpers::{
    load_active_order_run_session, load_active_order_run_session_for_qolip,
    load_active_order_run_sessions_for_worker, load_completed_queue_orders_for_actor,
    load_order_run_session, load_order_run_sessions_for_audit, load_order_run_sessions_for_order,
    load_order_run_sessions_for_orders, load_progress_batch, load_progress_batch_by_qr,
    load_progress_batches_for_audit, load_progress_batches_for_order,
    load_progress_batches_for_orders, load_progress_batches_for_worker,
    load_queue_action_logs_for_orders, load_queue_action_logs_for_worker,
};
use self::paddon_helpers::{
    add_paddon_item, add_paddon_items, create_paddon, load_paddon_scan_snapshot,
    load_paddon_snapshot, load_paddon_summary, load_paddons, remove_paddon_item,
    remove_paddon_items,
};
use self::progress_helpers::{
    correct_progress_batch, load_progress_batch_corrections_for_order, put_order_progress_batch,
    put_order_progress_batch_tx, put_order_progress_event, put_order_progress_event_tx,
    put_order_run_session, put_order_run_session_tx, receive_finished_goods_batch_tx,
};
use self::qolip_session_helpers::reject_qolip_in_use_tx;
use self::queue_helpers::{
    insert_queue_action_event_tx, put_queue_action_state_tx, queue_action_event_replay_tx,
    put_queue_states_tx, validate_queue_action_event_transition_tx,
};
use self::raw_material_stock_helpers::apply_raw_material_stock_transitions_tx;
use self::transaction_locks::{
    lock_order_and_apparatuses_tx, lock_orders_and_apparatuses_tx,
};
use self::transfer_helpers::{
    commit_apparatus_transfer as commit_apparatus_transfer_record,
    load_apparatus_transfer_by_idempotency_key, load_apparatus_transfers_for_audit,
};
use self::wip_query_helpers::load_wip_progress_batches;

#[derive(Clone)]
pub struct PostgresProductionMapStore {
    pool: PgPool,
}

impl PostgresProductionMapStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProductionMapStorePort for PostgresProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        load_maps(&self.pool).await
    }

    async fn next_order_number(&self) -> Result<String, ProductionMapError> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT CASE
                WHEN is_called AND last_value >= 9999 THEN NULL
                ELSE lpad(nextval('mini_production_order_number_seq')::text, 4, '0')
             END
             FROM mini_production_order_number_seq",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::OrderNumberExhausted)
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        reject_order_number_immutable(&self.pool, &map).await?;
        reject_duplicate_order_number(&self.pool, &map).await?;
        put_map_inner(&self.pool, &map).await
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        for map in maps {
            reject_order_number_immutable_tx(&mut tx, map).await?;
            reject_duplicate_order_number_tx(&mut tx, map).await?;
            put_map_inner_tx(&mut tx, map).await?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        delete_map_by_id(&self.pool, map_id).await
    }

    async fn order_control_states(
        &self,
    ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
        load_order_control_states(&self.pool).await
    }

    async fn order_freeze_requests_for_audit(
        &self,
    ) -> Result<Vec<crate::core::production_map::OrderFreezeAuditRecord>, ProductionMapError> {
        load_order_freeze_requests_for_audit(&self.pool).await
    }

    async fn put_order_control_state(
        &self,
        record: OrderControlRecord,
    ) -> Result<(), ProductionMapError> {
        save_order_control_state(&self.pool, &record).await
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        load_apparatus_sequences(&self.pool).await
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        save_apparatus_sequence(&self.pool, apparatus, order_ids).await
    }

    async fn resolve_apparatus_identity(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<ApparatusScheduleCandidate>, ProductionMapError> {
        resolve_apparatus_identity(&self.pool, apparatus_id).await
    }

    async fn apparatus_capacity_profiles(
        &self,
    ) -> Result<Vec<ApparatusCapacityProfile>, ProductionMapError> {
        load_apparatus_capacity_profiles(&self.pool).await
    }

    async fn put_apparatus_capacity_profile(
        &self,
        profile: ApparatusCapacityProfile,
    ) -> Result<(), ProductionMapError> {
        put_apparatus_capacity_profile(&self.pool, profile).await
    }

    async fn apparatus_downtimes(&self) -> Result<Vec<ApparatusDowntime>, ProductionMapError> {
        load_apparatus_downtimes(&self.pool).await
    }

    async fn put_apparatus_downtime(
        &self,
        downtime: ApparatusDowntime,
    ) -> Result<(), ProductionMapError> {
        put_apparatus_downtime(&self.pool, downtime).await
    }

    async fn apparatus_schedule_reservations(
        &self,
    ) -> Result<Vec<ApparatusScheduleReservation>, ProductionMapError> {
        load_apparatus_schedule_reservations(&self.pool).await
    }

    async fn apparatus_schedule_reservation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ApparatusScheduleReservation>, ProductionMapError> {
        load_apparatus_schedule_reservation_by_idempotency_key(&self.pool, idempotency_key).await
    }

    async fn put_apparatus_schedule_reservation(
        &self,
        reservation: ApparatusScheduleReservation,
        capacity_slots: u16,
        finite_capacity: bool,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        put_apparatus_schedule_reservation(&self.pool, reservation, capacity_slots, finite_capacity)
            .await
    }

    async fn cancel_apparatus_schedule_reservation(
        &self,
        input: ApparatusScheduleCancelRequest,
    ) -> Result<ApparatusScheduleReservation, ProductionMapError> {
        cancel_apparatus_schedule_reservation(&self.pool, input).await
    }

    async fn update_apparatus_schedule_reservation_status(
        &self,
        order_id: &str,
        apparatus_id: &ApparatusId,
        status: crate::core::production_map::ApparatusScheduleStatus,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        update_apparatus_schedule_reservation_status_tx(
            &mut tx,
            order_id,
            apparatus_id,
            status,
            actor,
        )
        .await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        load_apparatus_queue_states(&self.pool).await
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        put_queue_states_tx(&mut tx, apparatus, states).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<crate::core::production_map::ApparatusQueuePolicyMap, ProductionMapError> {
        load_apparatus_queue_policies(&self.pool).await
    }

    async fn put_apparatus_queue_policy(
        &self,
        apparatus_id: &ApparatusId,
        apparatus_display: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        save_apparatus_queue_policy(&self.pool, apparatus_id, apparatus_display, policy, actor)
            .await
    }

    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        _states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        lock_order_and_apparatuses_tx(&mut tx, &event.order_id, &[apparatus, &event.apparatus])
            .await?;
        if queue_action_event_replay_tx(&mut tx, &event).await? {
            tx.commit()
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            return Ok(());
        }
        validate_queue_action_event_transition_tx(&mut tx, &event).await?;
        put_queue_action_state_tx(&mut tx, &event).await?;
        insert_queue_action_event_tx(&mut tx, &event).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        lock_order_and_apparatuses_tx(&mut tx, &event.order_id, &[&event.apparatus]).await?;
        if queue_action_event_replay_tx(&mut tx, &event).await? {
            tx.commit()
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            return Ok(());
        }
        insert_queue_action_event_tx(&mut tx, &event).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        load_completed_queue_orders_for_actor(&self.pool, actor_ref, limit).await
    }

    async fn completion_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        load_completion_requests(&self.pool, limit).await
    }

    async fn completion_request_by_event_id(
        &self,
        event_id: &str,
    ) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
        load_completion_request_by_event_id(&self.pool, event_id).await
    }

    async fn completion_request_decisions_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        load_completion_request_decisions_for_actor(&self.pool, actor_ref, limit).await
    }

    async fn resolve_completion_request_decision(
        &self,
        request_event_id: &str,
        decision: CompletionRequestDecision,
        actor: &QueueActionActor,
        notification: &CompletionRequestDecisionNotification,
        state_resolution: Option<CompletionRequestStateResolution>,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        resolve_completion_request(
            &self.pool,
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
        load_queue_action_logs_for_orders(&self.pool, order_ids).await
    }

    async fn queue_action_logs_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        load_queue_action_logs_for_worker(&self.pool, worker_refs, worker_display_name, limit).await
    }

    async fn active_order_run_session(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        load_active_order_run_session(&self.pool, apparatus, order_id).await
    }

    async fn active_order_run_session_for_qolip(
        &self,
        qolip_code: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        load_active_order_run_session_for_qolip(&self.pool, qolip_code).await
    }

    async fn active_order_run_sessions_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        load_active_order_run_sessions_for_worker(
            &self.pool,
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
        load_order_run_session(&self.pool, session_id).await
    }

    async fn order_run_sessions_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        load_order_run_sessions_for_order(&self.pool, order_id).await
    }

    async fn order_run_sessions_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
        load_order_run_sessions_for_orders(&self.pool, order_ids).await
    }

    async fn laminatsiya_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<LaminatsiyaAstatkaReport>, ProductionMapError> {
        load_laminatsiya_astatka_reports_for_order(&self.pool, order_id).await
    }

    async fn put_laminatsiya_astatka_report(
        &self,
        report: LaminatsiyaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        put_laminatsiya_astatka_report(&self.pool, &report).await
    }

    async fn rezka_astatka_reports_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<RezkaAstatkaReport>, ProductionMapError> {
        load_rezka_astatka_reports_for_order(&self.pool, order_id).await
    }

    async fn put_rezka_astatka_report(
        &self,
        report: RezkaAstatkaReport,
    ) -> Result<(), ProductionMapError> {
        put_rezka_astatka_report(&self.pool, &report).await
    }

    async fn order_run_sessions_for_audit(
        &self,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        load_order_run_sessions_for_audit(&self.pool).await
    }

    async fn progress_batch(
        &self,
        batch_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        load_progress_batch(&self.pool, batch_id).await
    }

    async fn progress_batch_by_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        load_progress_batch_by_qr(&self.pool, qr_payload).await
    }

    async fn progress_batches_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_progress_batches_for_worker(&self.pool, worker_refs, worker_display_name, limit).await
    }

    async fn progress_batches_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_progress_batches_for_order(&self.pool, order_id).await
    }

    async fn progress_batches_for_orders(
        &self,
        order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderProgressBatch>>, ProductionMapError> {
        load_progress_batches_for_orders(&self.pool, order_ids).await
    }

    async fn progress_batch_corrections_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ProgressBatchCorrectionRecord>, ProductionMapError> {
        load_progress_batch_corrections_for_order(&self.pool, order_id).await
    }

    async fn correct_progress_batch(
        &self,
        current: OrderProgressBatch,
        input: ProgressBatchCorrectionInput,
        actor: QueueActionActor,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        correct_progress_batch(&self.pool, &current, &input, &actor).await
    }

    async fn progress_batches_for_audit(
        &self,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_progress_batches_for_audit(&self.pool).await
    }

    async fn apparatus_transfers_for_audit(
        &self,
    ) -> Result<Vec<ProductionMapApparatusTransferRecord>, ProductionMapError> {
        load_apparatus_transfers_for_audit(&self.pool).await
    }

    async fn wip_progress_batches(
        &self,
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_wip_progress_batches(&self.pool, query).await
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
        write: QueueActionProgressWrite,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        validate_queue_progress_write(&write)?;
        let apparatus = write.apparatus.trim();
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
            if let Some(record) = &write.order_control_update {
                locked_orders.push(record.order_id.as_str());
            }
            let mut locked_apparatuses = vec![write.apparatus.as_str(), write.event.apparatus.as_str()];
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
            if let Some(report) = &write.returned_paint_report {
                locked_apparatuses.push(report.apparatus.as_str());
            }
            if let Some(record) = &write.order_control_update {
                if let Some(request) = &record.freeze_request {
                    if !request.target_apparatus.trim().is_empty() {
                        locked_apparatuses.push(request.target_apparatus.as_str());
                    }
                }
            }
            lock_orders_and_apparatuses_tx(&mut tx, &locked_orders, &locked_apparatuses)
                .await?;
        }
        if queue_action_event_replay_tx(&mut tx, &write.event).await? {
            tx.commit()
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            return Ok(QueueActionProgressWriteResult::default());
        }
        validate_queue_action_event_transition_tx(&mut tx, &write.event).await?;
        let current_session_id = write
            .session
            .as_ref()
            .map(|session| session.session_id.trim())
            .filter(|session_id| !session_id.is_empty())
            .map(str::to_string);
        let sequence_updates = write.sequence_updates;
        if let Some(map) = &write.map_update {
            put_map_inner_tx(&mut tx, map).await?;
        }
        if let Some(session) = &write.session {
            reject_qolip_in_use_tx(&mut tx, session).await?;
        }
        put_queue_action_state_tx(&mut tx, &write.event).await?;
        let remove_order_from_sequence = matches!(
            write.event.action,
            crate::core::production_map::queue_state::ApparatusQueueAction::Freeze
        ) || write
            .event
            .payload_json
            .get("admin_unfreeze")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let append_order_to_sequence = write
            .event
            .payload_json
            .get("admin_unfreeze")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        for (sequence_apparatus, order_ids) in &sequence_updates {
            apply_apparatus_sequence_delta_tx(
                &mut tx,
                sequence_apparatus,
                &write.event.order_id,
                order_ids,
                remove_order_from_sequence,
                append_order_to_sequence,
            )
            .await?;
        }
        insert_queue_action_event_tx(&mut tx, &write.event).await?;
        if let Some(status) = write.schedule_reservation_status {
            let apparatus_id = ApparatusId::new(apparatus.to_string())
                .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
            update_apparatus_schedule_reservation_status_tx(
                &mut tx,
                &write.event.order_id,
                &apparatus_id,
                status,
                &write.event.actor,
            )
            .await?;
        }
        if let Some(session) = write.session {
            put_order_run_session_tx(&mut tx, &session).await?;
        }
        if let Some(event) = write.progress_event {
            put_order_progress_event_tx(&mut tx, &event).await?;
        }
        let progress_batches = write.progress_batches;
        if progress_batches.is_empty() {
            if let Some(batch) = write.progress_batch {
                put_order_progress_batch_tx(&mut tx, &batch).await?;
            }
        } else {
            for batch in progress_batches {
                put_order_progress_batch_tx(&mut tx, &batch).await?;
            }
        }
        for batch in write.progress_batch_updates {
            put_order_progress_batch_tx(&mut tx, &batch).await?;
        }
        if let Some(record) = &write.order_control_update {
            save_order_control_state_tx(&mut tx, record).await?;
        }
        let qolip_checkout_committed = !write.qolip_checkouts.is_empty();
        for checkout in &write.qolip_checkouts {
            super::postgres_qolip::save_checkout_tx(
                &mut tx,
                checkout,
                current_session_id.as_deref(),
            )
            .await
            .map_err(production_map_qolip_checkout_error)?;
        }
        let raw_material_stock_warehouses = apply_raw_material_stock_transitions_tx(
            &mut tx,
            &write.raw_material_stock_transitions,
            &write.event.actor,
            &write.event.apparatus,
        )
        .await?;
        if let Some(report) = &write.returned_paint_report {
            super::postgres_returned_paint::insert_returned_paint_request_tx(&mut tx, report)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(QueueActionProgressWriteResult {
            raw_material_stock_warehouses,
            qolip_checkout_committed,
        })
    }

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        load_apparatus_material_rules(&self.pool).await
    }

    async fn put_apparatus_material_rule(
        &self,
        rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        save_apparatus_material_rule(&self.pool, rule).await
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
        let warehouses = apply_raw_material_stock_transitions_tx(
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

fn production_map_qolip_checkout_error(error: QolipError) -> ProductionMapError {
    match error {
        QolipError::LocationNotFound => ProductionMapError::QolipLocationNotFound,
        QolipError::QolipCodeNotFound | QolipError::QolipCodeMismatch => {
            ProductionMapError::QolipCodeMismatch
        }
        QolipError::InsufficientStock => ProductionMapError::QolipInsufficientStock,
        QolipError::LocationIdentityMismatch => ProductionMapError::QolipLocationIdentityMismatch,
        QolipError::QolipInUse => ProductionMapError::QolipAlreadyInUse,
        _ => ProductionMapError::StoreFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qolip_in_use_checkout_error_is_returned_as_business_error() {
        assert_eq!(
            production_map_qolip_checkout_error(QolipError::QolipInUse),
            ProductionMapError::QolipAlreadyInUse
        );
    }
}
