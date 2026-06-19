use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::materials::{ApparatusMaterialRule, RawMaterialAssignment};
use super::queue_state;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapDefinition {
    pub id: String,
    pub product_code: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub order_number: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll_count: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_length: Option<f64>,
    #[serde(default)]
    pub nodes: Vec<ProductionMapNode>,
    #[serde(default)]
    pub edges: Vec<ProductionMapEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapNode {
    pub id: String,
    pub kind: ProductionMapNodeKind,
    pub title: String,
    #[serde(default)]
    pub formula: Option<ProductionFormula>,
    #[serde(default)]
    pub role_code: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub qty_formula: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative_group_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative_group_label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alternative_assigned_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_kadr_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_label_length: Option<f64>,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionMapNodeKind {
    Start,
    Location,
    Material,
    Apparatus,
    KkProduct,
    Formula,
    Condition,
    Task,
    Wait,
    Output,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionFormula {
    pub target: String,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionMapEdge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionMapProgram {
    pub map_id: String,
    pub product_code: String,
    pub operations: Vec<ProductionMapOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionMapOperation {
    pub order: usize,
    pub node_id: String,
    pub op_code: String,
    pub args: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapSaved {
    pub map: ProductionMapDefinition,
    pub program: ProductionMapProgram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapMoveRequest {
    #[serde(default)]
    pub map_id: String,
    #[serde(default)]
    pub from_apparatus: String,
    #[serde(default)]
    pub to_apparatus: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapBatchMoveRequest {
    #[serde(default)]
    pub from_apparatus: String,
    #[serde(default)]
    pub to_apparatus: String,
    #[serde(default)]
    pub map_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapRunRequest {
    #[serde(default)]
    pub map_id: String,
    #[serde(default)]
    pub product_code: String,
    pub order_qty: f64,
    #[serde(default)]
    pub variables: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionTaskDraft {
    pub order: usize,
    pub node_id: String,
    pub task_kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to_location: String,
    pub qty: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapRunResult {
    pub map_id: String,
    pub product_code: String,
    pub order_qty: f64,
    pub variables: BTreeMap<String, f64>,
    pub tasks: Vec<ProductionTaskDraft>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visited_node_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub awaiting_node_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub awaiting_variable: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub awaiting_expression: String,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ProductionMapError {
    #[error("map id is required")]
    MissingId,
    #[error("product code is required")]
    MissingProductCode,
    #[error("map title is required")]
    MissingTitle,
    #[error("map needs one start node")]
    MissingStart,
    #[error("map needs one end node")]
    MissingEnd,
    #[error("duplicate node id: {0}")]
    DuplicateNode(String),
    #[error("order number already belongs to another zakaz")]
    DuplicateOrderNumber,
    #[error("order number cannot be changed")]
    OrderNumberImmutable,
    #[error("edge references missing node: {0}")]
    MissingEdgeNode(String),
    #[error("map has a cycle")]
    Cycle,
    #[error("formula target is required")]
    MissingFormulaTarget,
    #[error("formula expression is required")]
    MissingFormulaExpression,
    #[error("invalid formula target: {0}")]
    InvalidFormulaTarget(String),
    #[error("invalid formula expression: {0}")]
    InvalidFormulaExpression(String),
    #[error("map not found")]
    MapNotFound,
    #[error("order quantity must be positive")]
    InvalidOrderQty,
    #[error("node quantity must be positive: {0}")]
    InvalidNodeQty(String),
    #[error("invalid location: {0}")]
    InvalidLocation(String),
    #[error("unknown formula variable: {0}")]
    UnknownFormulaVariable(String),
    #[error("formula division by zero")]
    FormulaDivisionByZero,
    #[error("condition needs true and false branches")]
    MissingConditionBranch,
    #[error("order is not allowed on the target apparatus")]
    MoveNotAllowed,
    #[error("store failed")]
    StoreFailed,
    #[error("queue action is not allowed")]
    QueueActionNotAllowed,
    #[error("previous production stage is not completed")]
    PreviousStageNotCompleted,
    #[error("apparatus is not assigned to this operator")]
    ApparatusNotAssigned,
    #[error("laminatsiya is not allowed when rubber size is above 1050")]
    LaminatsiyaRubberTooLarge,
    #[error("apparatus queue policy is locked")]
    ApparatusQueuePolicyLocked,
    #[error("raw material input is invalid")]
    RawMaterialInvalidInput,
    #[error("raw material group is not allowed for this order")]
    RawMaterialGroupNotAllowed,
    #[error("raw material group matches multiple apparatus")]
    RawMaterialGroupAmbiguous,
    #[error("raw material is already assigned")]
    RawMaterialAlreadyAssigned,
    #[error("raw material is already assigned to this order")]
    RawMaterialAlreadyAssignedToOrder,
    #[error("raw material assignment is required")]
    RawMaterialAssignmentNotFound,
    #[error("raw material scan is required")]
    RawMaterialScanRequired,
    #[error("raw material scan does not match assigned material")]
    RawMaterialMismatch,
    #[error("progress input is invalid")]
    ProgressInputInvalid,
    #[error("progress batch not found")]
    ProgressBatchNotFound,
    #[error("progress batch cannot resume")]
    ProgressBatchNotResumable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusQueuePolicy {
    StrictSequence,
    FreePick,
}

impl ApparatusQueuePolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "strict_sequence" => Some(Self::StrictSequence),
            "free_pick" => Some(Self::FreePick),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrictSequence => "strict_sequence",
            Self::FreePick => "free_pick",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusQueuePolicyRecord {
    pub apparatus: String,
    pub policy: ApparatusQueuePolicy,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueActionActor {
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusQueueActionEvent {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub from_state: queue_state::ApparatusQueueOrderState,
    pub to_state: queue_state::ApparatusQueueOrderState,
    pub policy: ApparatusQueuePolicy,
    pub actor: QueueActionActor,
    #[serde(default)]
    pub assigned_apparatus: Vec<String>,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedQueueOrder {
    pub apparatus: String,
    pub order_id: String,
    pub completed_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRequestNotification {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub order_number: String,
    pub order_title: String,
    pub product_code: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub description: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionRequestDecision {
    Approved,
    Rejected,
}

impl CompletionRequestDecision {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "approve" | "approved" => Some(Self::Approved),
            "reject" | "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRequestDecisionNotification {
    pub event_id: String,
    pub request_event_id: String,
    pub decision: String,
    pub apparatus: String,
    pub order_id: String,
    pub order_number: String,
    pub order_title: String,
    pub product_code: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub decided_by_role: String,
    pub decided_by_ref: String,
    pub decided_by_display_name: String,
    pub description: String,
    pub message: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionOrderLogEntry {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub from_state: queue_state::ApparatusQueueOrderState,
    pub to_state: queue_state::ApparatusQueueOrderState,
    pub actor_role: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub created_at_unix: i64,
    #[serde(default)]
    pub completed_with_issue: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullyCompletedProductionOrder {
    pub order_id: String,
    pub order_number: String,
    pub title: String,
    pub product_code: String,
    pub completed_at_unix: i64,
    pub closed_by_role: String,
    pub closed_by_ref: String,
    pub closed_by_display_name: String,
    pub logs: Vec<ProductionOrderLogEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunStatus {
    Active,
    Paused,
    Completed,
}

impl OrderRunStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchStatus {
    Paused,
    Completed,
    Resumed,
}

impl OrderProgressBatchStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "resumed" => Some(Self::Resumed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Resumed => "resumed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRunSession {
    pub session_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub status: OrderRunStatus,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub started_at_unix: i64,
    pub updated_at_unix: i64,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderProgressEvent {
    pub event_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub batch_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub produced_qty: f64,
    pub uom: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub qr_payload: String,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderProgressBatch {
    pub batch_id: String,
    pub session_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub status: OrderProgressBatchStatus,
    pub produced_qty: f64,
    pub uom: String,
    pub qr_payload: String,
    pub label_item_code: String,
    pub label_item_name: String,
    pub executor_name: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueProgressInput {
    pub produced_qty: Option<f64>,
    pub uom: String,
    pub progress_batch_id: String,
    pub qr_payload: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApparatusQueueActionResult {
    pub states: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<OrderRunSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_event: Option<OrderProgressEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_batch: Option<OrderProgressBatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionRequestResult {
    pub states: BTreeMap<String, String>,
    pub completion_request: CompletionRequestNotification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionRequestDecisionResult {
    pub states: BTreeMap<String, String>,
    pub decision: CompletionRequestDecisionNotification,
}

#[derive(Debug, Clone)]
pub struct CompletionRequestStateResolution {
    pub apparatus: String,
    pub states: BTreeMap<String, String>,
    pub event: ApparatusQueueActionEvent,
    pub session: Option<OrderRunSession>,
}

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
    async fn active_order_run_session(
        &self,
        _apparatus: &str,
        _order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        Ok(None)
    }
    async fn order_run_session(
        &self,
        _session_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        Ok(None)
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
    async fn put_apparatus_queue_states_with_event_and_progress(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
        session: Option<OrderRunSession>,
        progress_event: Option<OrderProgressEvent>,
        progress_batch: Option<OrderProgressBatch>,
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
}
