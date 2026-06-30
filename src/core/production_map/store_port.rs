use std::collections::BTreeMap;

use async_trait::async_trait;

use super::materials::{ApparatusMaterialRule, RawMaterialAssignment};
use super::types::*;

pub type StoreResult<T> = Result<T, ProductionMapError>;
pub type ApparatusSequenceMap = BTreeMap<String, Vec<String>>;
pub type QueueStateMap = BTreeMap<String, String>;
pub type ApparatusQueueStateMap = BTreeMap<String, QueueStateMap>;
pub type ApparatusQueuePolicyMap = BTreeMap<String, ApparatusQueuePolicy>;
pub type OrderLogMap = BTreeMap<String, Vec<ProductionOrderLogEntry>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawMaterialStockTransitionKind {
    InUse,
    Consumed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMaterialStockTransition {
    pub kind: RawMaterialStockTransitionKind,
    pub barcodes: Vec<String>,
    pub order_id: String,
}

impl RawMaterialStockTransition {
    pub fn new(
        kind: RawMaterialStockTransitionKind,
        barcodes: Vec<String>,
        order_id: &str,
    ) -> Self {
        Self {
            kind,
            barcodes: barcodes
                .into_iter()
                .map(|barcode| barcode.trim().to_string())
                .filter(|barcode| !barcode.is_empty())
                .collect(),
            order_id: order_id.trim().to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.barcodes.is_empty() || self.order_id.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueActionProgressWriteResult {
    pub raw_material_stock_warehouses: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QueueActionProgressWrite {
    pub apparatus: String,
    pub states: QueueStateMap,
    pub event: ApparatusQueueActionEvent,
    pub session: Option<OrderRunSession>,
    pub progress_event: Option<OrderProgressEvent>,
    pub progress_batch: Option<OrderProgressBatch>,
    pub progress_batch_updates: Vec<OrderProgressBatch>,
    pub raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
}

#[async_trait]
pub trait ProductionMapStorePort: Send + Sync {
    // Maps and apparatus sequence persistence.
    async fn maps(&self) -> StoreResult<Vec<ProductionMapDefinition>>;
    async fn put_map(&self, map: ProductionMapDefinition) -> StoreResult<()>;
    async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> StoreResult<()>;
    async fn delete_map(&self, map_id: &str) -> StoreResult<()>;
    async fn apparatus_sequences(&self) -> StoreResult<ApparatusSequenceMap>;
    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> StoreResult<()>;

    // Queue state, policy, log, and completion-request persistence.
    async fn apparatus_queue_states(&self) -> StoreResult<ApparatusQueueStateMap>;
    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: QueueStateMap,
    ) -> StoreResult<()>;
    async fn apparatus_queue_policies(&self) -> StoreResult<ApparatusQueuePolicyMap>;
    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> StoreResult<()>;
    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        states: QueueStateMap,
        event: ApparatusQueueActionEvent,
    ) -> StoreResult<()> {
        self.put_apparatus_queue_states(apparatus, states).await?;
        self.append_apparatus_queue_action_event(event).await
    }
    async fn append_apparatus_queue_action_event(
        &self,
        _event: ApparatusQueueActionEvent,
    ) -> StoreResult<()> {
        Ok(())
    }
    async fn completed_queue_orders_for_actor(
        &self,
        _actor_ref: &str,
        _limit: usize,
    ) -> StoreResult<Vec<CompletedQueueOrder>> {
        Ok(Vec::new())
    }
    async fn completion_requests(
        &self,
        _limit: usize,
    ) -> StoreResult<Vec<CompletionRequestNotification>> {
        Ok(Vec::new())
    }
    async fn completion_request_by_event_id(
        &self,
        _event_id: &str,
    ) -> StoreResult<Option<CompletionRequestNotification>> {
        Ok(None)
    }
    async fn completion_request_decisions_for_actor(
        &self,
        _actor_ref: &str,
        _limit: usize,
    ) -> StoreResult<Vec<CompletionRequestDecisionNotification>> {
        Ok(Vec::new())
    }
    async fn resolve_completion_request_decision(
        &self,
        _request_event_id: &str,
        _decision: CompletionRequestDecision,
        _actor: &QueueActionActor,
        _notification: &CompletionRequestDecisionNotification,
        _state_resolution: Option<CompletionRequestStateResolution>,
    ) -> StoreResult<QueueActionProgressWriteResult> {
        Ok(QueueActionProgressWriteResult::default())
    }
    async fn queue_action_logs_for_orders(
        &self,
        _order_ids: &[String],
    ) -> StoreResult<OrderLogMap> {
        Ok(BTreeMap::new())
    }
    async fn queue_action_logs_for_worker(
        &self,
        _worker_refs: &[String],
        _worker_display_name: &str,
        _limit: usize,
    ) -> StoreResult<Vec<ProductionOrderLogEntry>> {
        Ok(Vec::new())
    }

    // Run session, progress event, WIP, and finished-goods persistence.
    async fn active_order_run_session(
        &self,
        _apparatus: &str,
        _order_id: &str,
    ) -> StoreResult<Option<OrderRunSession>> {
        Ok(None)
    }
    async fn active_order_run_sessions_for_worker(
        &self,
        _worker_refs: &[String],
        _worker_display_name: &str,
        _limit: usize,
    ) -> StoreResult<Vec<OrderRunSession>> {
        Ok(Vec::new())
    }
    async fn order_run_session(&self, _session_id: &str) -> StoreResult<Option<OrderRunSession>> {
        Ok(None)
    }
    async fn order_run_sessions_for_order(
        &self,
        _order_id: &str,
    ) -> StoreResult<Vec<OrderRunSession>> {
        Ok(Vec::new())
    }
    async fn order_run_sessions_for_audit(&self) -> StoreResult<Vec<OrderRunSession>> {
        Ok(Vec::new())
    }
    async fn progress_batch(&self, _batch_id: &str) -> StoreResult<Option<OrderProgressBatch>> {
        Ok(None)
    }
    async fn progress_batch_by_qr(
        &self,
        _qr_payload: &str,
    ) -> StoreResult<Option<OrderProgressBatch>> {
        Ok(None)
    }
    async fn progress_batches_for_worker(
        &self,
        _worker_refs: &[String],
        _worker_display_name: &str,
        _limit: usize,
    ) -> StoreResult<Vec<OrderProgressBatch>> {
        Ok(Vec::new())
    }
    async fn progress_batches_for_order(
        &self,
        _order_id: &str,
    ) -> StoreResult<Vec<OrderProgressBatch>> {
        Ok(Vec::new())
    }
    async fn progress_batches_for_audit(&self) -> StoreResult<Vec<OrderProgressBatch>> {
        self.wip_progress_batches(WipProgressBatchQuery::new(
            "", "", "", None, true, "", 10_000,
        ))
        .await
    }
    async fn wip_progress_batches(
        &self,
        _query: WipProgressBatchQuery,
    ) -> StoreResult<Vec<OrderProgressBatch>> {
        Ok(Vec::new())
    }
    async fn put_order_run_session(&self, _session: OrderRunSession) -> StoreResult<()> {
        Ok(())
    }
    async fn put_order_progress_event(&self, _event: OrderProgressEvent) -> StoreResult<()> {
        Ok(())
    }
    async fn put_order_progress_batch(&self, _batch: OrderProgressBatch) -> StoreResult<()> {
        Ok(())
    }
    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        _stock: FinishedGoodsStockEntry,
    ) -> StoreResult<()> {
        self.put_order_progress_batch(batch).await
    }
    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        write: QueueActionProgressWrite,
    ) -> StoreResult<QueueActionProgressWriteResult> {
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
        Ok(QueueActionProgressWriteResult::default())
    }

    // Raw material rule and assignment persistence.
    async fn apparatus_material_rules(&self) -> StoreResult<Vec<ApparatusMaterialRule>>;
    async fn put_apparatus_material_rule(&self, rule: ApparatusMaterialRule) -> StoreResult<()>;
    async fn raw_material_assignments(&self) -> StoreResult<Vec<RawMaterialAssignment>>;
    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> StoreResult<()>;
    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> StoreResult<Option<RawMaterialAssignment>>;
}
