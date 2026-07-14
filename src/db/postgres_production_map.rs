use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::production_map::{
    ApparatusMaterialRule, ApparatusQueueActionEvent, ApparatusQueuePolicy, CompletedQueueOrder,
    CompletionRequestDecision, CompletionRequestDecisionNotification,
    CompletionRequestNotification, CompletionRequestStateResolution, FinishedGoodsStockEntry,
    OrderProgressBatch, OrderProgressEvent, OrderRunSession, ProductionMapDefinition,
    ProductionMapError, ProductionMapStorePort, ProductionOrderLogEntry, QueueActionActor,
    QueueActionProgressWrite, QueueActionProgressWriteResult, RawMaterialAssignment,
    WipProgressBatchQuery,
};
use crate::core::qolip::QolipError;

mod catalog_helpers;
mod completion_helpers;
mod map_helpers;
mod material_helpers;
mod order_query_helpers;
mod progress_helpers;
mod qolip_session_helpers;
mod queue_helpers;
mod raw_material_stock_helpers;
mod wip_query_helpers;

use self::catalog_helpers::{
    delete_map_by_id, load_apparatus_queue_policies, load_apparatus_queue_states,
    load_apparatus_sequences, load_maps, save_apparatus_queue_policy, save_apparatus_sequence,
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
use self::order_query_helpers::{
    load_active_order_run_session, load_active_order_run_session_for_qolip,
    load_active_order_run_sessions_for_worker,
    load_completed_queue_orders_for_actor, load_order_run_session,
    load_order_run_sessions_for_audit, load_order_run_sessions_for_order, load_progress_batch,
    load_progress_batch_by_qr, load_progress_batches_for_audit, load_progress_batches_for_order,
    load_progress_batches_for_worker, load_queue_action_logs_for_orders,
    load_queue_action_logs_for_worker,
};
use self::progress_helpers::{
    put_order_progress_batch, put_order_progress_batch_tx, put_order_progress_event,
    put_order_progress_event_tx, put_order_run_session, put_order_run_session_tx,
    receive_finished_goods_batch_tx,
};
use self::qolip_session_helpers::reject_qolip_in_use_tx;
use self::queue_helpers::{insert_queue_action_event_tx, put_queue_states_tx};
use self::raw_material_stock_helpers::apply_raw_material_stock_transitions_tx;
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
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        load_apparatus_queue_policies(&self.pool).await
    }

    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        save_apparatus_queue_policy(&self.pool, apparatus, policy, actor).await
    }

    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        put_queue_states_tx(&mut tx, apparatus, states).await?;
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

    async fn progress_batches_for_audit(
        &self,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_progress_batches_for_audit(&self.pool).await
    }

    async fn wip_progress_batches(
        &self,
        query: WipProgressBatchQuery,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        load_wip_progress_batches(&self.pool, query).await
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
        let apparatus = write.apparatus.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(session) = &write.session {
            reject_qolip_in_use_tx(&mut tx, session).await?;
        }
        put_queue_states_tx(&mut tx, apparatus, write.states).await?;
        insert_queue_action_event_tx(&mut tx, &write.event).await?;
        if let Some(session) = write.session {
            put_order_run_session_tx(&mut tx, &session).await?;
        }
        if let Some(event) = write.progress_event {
            put_order_progress_event_tx(&mut tx, &event).await?;
        }
        if let Some(batch) = write.progress_batch {
            put_order_progress_batch_tx(&mut tx, &batch).await?;
        }
        for batch in write.progress_batch_updates {
            put_order_progress_batch_tx(&mut tx, &batch).await?;
        }
        let qolip_checkout_committed = if let Some(checkout) = &write.qolip_checkout {
            super::postgres_qolip::save_checkout_tx(&mut tx, checkout)
                .await
                .map_err(production_map_qolip_checkout_error)?;
            true
        } else {
            false
        };
        let raw_material_stock_warehouses =
            apply_raw_material_stock_transitions_tx(
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
        QolipError::LocationIdentityMismatch => {
            ProductionMapError::QolipLocationIdentityMismatch
        }
        _ => ProductionMapError::StoreFailed,
    }
}
