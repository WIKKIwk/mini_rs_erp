mod maps;
mod materials;
mod queue;
mod runs;
mod state;

use super::*;

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use async_trait::async_trait;

pub use state::MemoryProductionMapStore;

#[async_trait]
#[cfg(test)]
impl ProductionMapStorePort for MemoryProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        maps::maps(self).await
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

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        queue::apparatus_queue_policies(self).await
    }

    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        queue::put_apparatus_queue_policy(self, apparatus, policy, actor).await
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

    async fn put_order_progress_batch(
        &self,
        batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        runs::put_order_progress_batch(self, batch).await
    }

    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: QueueActionProgressWrite,
    ) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
        if self
            .fail_next_queue_progress_commit
            .swap(false, Ordering::SeqCst)
        {
            return Err(ProductionMapError::StoreFailed);
        }
        if let Some(session) = &write.session
            && matches!(session.status, OrderRunStatus::Active | OrderRunStatus::Paused)
            && let Some(qolip_code) = session
                .payload_json
                .get("qolip_code")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            && self
                .active_order_run_session_for_qolip(qolip_code)
                .await?
                .is_some_and(|active| active.session_id != session.session_id)
        {
            return Err(ProductionMapError::QolipAlreadyInUse);
        }
        self.put_apparatus_queue_states_with_event(&write.apparatus, write.states, write.event)
            .await?;
        if let Some(session) = write.session {
            self.put_order_run_session(session).await?;
        }
        if let Some(event) = write.progress_event {
            self.put_order_progress_event(event).await?;
        }
        if let Some(batch) = write.progress_batch {
            self.put_order_progress_batch(batch).await?;
        }
        for batch in write.progress_batch_updates {
            self.put_order_progress_batch(batch).await?;
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

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        materials::apparatus_material_rules(self).await
    }

    async fn put_apparatus_material_rule(
        &self,
        rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        materials::put_apparatus_material_rule(self, rule).await
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
