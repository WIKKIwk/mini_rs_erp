use std::collections::BTreeMap;

use async_trait::async_trait;

use super::materials::{ApparatusMaterialRule, RawMaterialAssignment};
use super::types::*;

#[async_trait]
pub trait ProductionMapStorePort: Send + Sync {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError>;
    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError>;
    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError>;
    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError>;
    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError>;
    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError>;
    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError>;
    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError>;
    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError>;
    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError>;
    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        self.put_apparatus_queue_states(apparatus, states).await?;
        self.append_apparatus_queue_action_event(event).await
    }
    async fn append_apparatus_queue_action_event(
        &self,
        _event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        Ok(())
    }
    async fn completed_queue_orders_for_actor(
        &self,
        _actor_ref: &str,
        _limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn completion_requests(
        &self,
        _limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn completion_request_by_event_id(
        &self,
        _event_id: &str,
    ) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
        Ok(None)
    }
    async fn completion_request_decisions_for_actor(
        &self,
        _actor_ref: &str,
        _limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn resolve_completion_request_decision(
        &self,
        _request_event_id: &str,
        _decision: CompletionRequestDecision,
        _actor: &QueueActionActor,
        _notification: &CompletionRequestDecisionNotification,
        _state_resolution: Option<CompletionRequestStateResolution>,
    ) -> Result<(), ProductionMapError> {
        Ok(())
    }
    async fn queue_action_logs_for_orders(
        &self,
        _order_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
        Ok(BTreeMap::new())
    }
    async fn queue_action_logs_for_worker(
        &self,
        _worker_refs: &[String],
        _worker_display_name: &str,
        _limit: usize,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn active_order_run_session(
        &self,
        _apparatus: &str,
        _order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        Ok(None)
    }
    async fn active_order_run_sessions_for_worker(
        &self,
        _worker_refs: &[String],
        _worker_display_name: &str,
        _limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn order_run_session(
        &self,
        _session_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        Ok(None)
    }
    async fn order_run_sessions_for_order(
        &self,
        _order_id: &str,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn progress_batch(
        &self,
        _batch_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        Ok(None)
    }
    async fn progress_batch_by_qr(
        &self,
        _qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        Ok(None)
    }
    async fn progress_batches_for_worker(
        &self,
        _worker_refs: &[String],
        _worker_display_name: &str,
        _limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn progress_batches_for_order(
        &self,
        _order_id: &str,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn wip_progress_batches(
        &self,
        _apparatus: &str,
        _current_location: &str,
        _status: Option<OrderProgressBatchWipStatus>,
        _order_id: &str,
        _limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        Ok(Vec::new())
    }
    async fn put_order_run_session(
        &self,
        _session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        Ok(())
    }
    async fn put_order_progress_event(
        &self,
        _event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        Ok(())
    }
    async fn put_order_progress_batch(
        &self,
        _batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        Ok(())
    }
    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        _stock: FinishedGoodsStockEntry,
    ) -> Result<(), ProductionMapError> {
        self.put_order_progress_batch(batch).await
    }
    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
        session: Option<OrderRunSession>,
        progress_event: Option<OrderProgressEvent>,
        progress_batch: Option<OrderProgressBatch>,
        progress_batch_updates: Vec<OrderProgressBatch>,
    ) -> Result<(), ProductionMapError> {
        self.put_apparatus_queue_states_with_event(apparatus, states, event)
            .await?;
        if let Some(session) = session {
            self.put_order_run_session(session).await?;
        }
        if let Some(event) = progress_event {
            self.put_order_progress_event(event).await?;
        }
        if let Some(batch) = progress_batch {
            self.put_order_progress_batch(batch).await?;
        }
        for batch in progress_batch_updates {
            self.put_order_progress_batch(batch).await?;
        }
        Ok(())
    }
    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError>;
    async fn put_apparatus_material_rule(
        &self,
        rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError>;
    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError>;
    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError>;
    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError>;
}
