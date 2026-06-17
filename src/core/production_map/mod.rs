use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(test)]
use tokio::sync::RwLock;
use tokio::sync::broadcast;

pub mod chain;
pub mod materials;
pub mod pechat;
pub mod queue_state;

pub use materials::{
    ApparatusMaterialRule, ApparatusMaterialRuleUpsert, RawMaterialAssignment,
    RawMaterialAssignmentInput,
};

const MAX_LAMINATSIYA_RUBBER_SIZE_MM: i64 = 1050;

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
    #[error("raw material assignment is required")]
    RawMaterialAssignmentNotFound,
    #[error("raw material scan is required")]
    RawMaterialScanRequired,
    #[error("raw material scan does not match assigned material")]
    RawMaterialMismatch,
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

#[cfg(test)]
pub struct MemoryProductionMapStore {
    maps: RwLock<BTreeMap<String, ProductionMapDefinition>>,
    sequences: RwLock<BTreeMap<String, Vec<String>>>,
    queue_states: RwLock<BTreeMap<String, BTreeMap<String, String>>>,
    queue_policies: RwLock<BTreeMap<String, ApparatusQueuePolicy>>,
    queue_events: RwLock<Vec<ApparatusQueueActionEvent>>,
    material_rules: RwLock<BTreeMap<String, ApparatusMaterialRule>>,
    material_assignments: RwLock<BTreeMap<String, RawMaterialAssignment>>,
}

#[cfg(test)]
impl MemoryProductionMapStore {
    pub fn new() -> Self {
        Self {
            maps: RwLock::new(BTreeMap::new()),
            sequences: RwLock::new(BTreeMap::new()),
            queue_states: RwLock::new(BTreeMap::new()),
            queue_policies: RwLock::new(BTreeMap::new()),
            queue_events: RwLock::new(Vec::new()),
            material_rules: RwLock::new(BTreeMap::new()),
            material_assignments: RwLock::new(BTreeMap::new()),
        }
    }
}

#[cfg(test)]
impl Default for MemoryProductionMapStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
#[cfg(test)]
impl ProductionMapStorePort for MemoryProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        Ok(self.maps.read().await.values().cloned().collect())
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        let mut maps = self.maps.write().await;
        reject_order_number_immutable(&maps, &map)?;
        let order_number = map.order_number.trim();
        if !order_number.is_empty() {
            let duplicate = maps.values().any(|existing| {
                existing.order_number.trim() == order_number && existing.id.trim() != map.id.trim()
            });
            if duplicate {
                return Err(ProductionMapError::DuplicateOrderNumber);
            }
        }
        maps.insert(map.id.clone(), map);
        Ok(())
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        let mut store = self.maps.write().await;
        for map in maps {
            reject_order_number_immutable(&store, map)?;
            let order_number = map.order_number.trim();
            if !order_number.is_empty() {
                let duplicate = store.values().any(|existing| {
                    existing.order_number.trim() == order_number
                        && existing.id.trim() != map.id.trim()
                });
                if duplicate {
                    return Err(ProductionMapError::DuplicateOrderNumber);
                }
            }
        }
        for map in maps {
            store.insert(map.id.clone(), map.clone());
        }
        Ok(())
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        self.maps.write().await.remove(map_id.trim());
        Ok(())
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        Ok(self.sequences.read().await.clone())
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        self.sequences
            .write()
            .await
            .insert(apparatus.trim().to_string(), order_ids);
        Ok(())
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        Ok(self.queue_states.read().await.clone())
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        self.queue_states
            .write()
            .await
            .insert(apparatus.trim().to_string(), states);
        Ok(())
    }

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        Ok(self.queue_policies.read().await.clone())
    }

    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        _actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        self.queue_policies
            .write()
            .await
            .insert(apparatus.trim().to_string(), policy);
        Ok(())
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        self.queue_events.write().await.push(event);
        Ok(())
    }

    async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        let actor_ref = actor_ref.trim();
        if actor_ref.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let events = self.queue_events.read().await;
        let mut seen = BTreeSet::new();
        let mut completed = Vec::new();
        for (index, event) in events.iter().enumerate().rev() {
            if event.actor.ref_.trim() != actor_ref
                || event.action != queue_state::ApparatusQueueAction::Complete
                || event.to_state != queue_state::ApparatusQueueOrderState::Completed
            {
                continue;
            }
            let order_id = event.order_id.trim();
            if order_id.is_empty() || !seen.insert(order_id.to_string()) {
                continue;
            }
            completed.push(CompletedQueueOrder {
                apparatus: event.apparatus.trim().to_string(),
                order_id: order_id.to_string(),
                completed_at_unix: index as i64 + 1,
            });
            if completed.len() >= limit {
                break;
            }
        }
        Ok(completed)
    }

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        Ok(self.material_rules.read().await.values().cloned().collect())
    }

    async fn put_apparatus_material_rule(
        &self,
        rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        self.material_rules
            .write()
            .await
            .insert(rule.apparatus.to_lowercase(), rule);
        Ok(())
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        Ok(self
            .material_assignments
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        self.material_assignments
            .write()
            .await
            .insert(assignment.barcode.to_uppercase(), assignment);
        Ok(())
    }
}

const LIVE_NOTIFY_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
pub struct ProductionMapLiveSnapshot {
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
}

#[derive(Clone)]
pub struct ProductionMapService {
    store: std::sync::Arc<dyn ProductionMapStorePort>,
    live_notify: broadcast::Sender<()>,
}

impl ProductionMapService {
    pub fn new(store: std::sync::Arc<dyn ProductionMapStorePort>) -> Self {
        let (live_notify, _) = broadcast::channel(LIVE_NOTIFY_CAPACITY);
        Self { store, live_notify }
    }

    pub fn subscribe_live(&self) -> broadcast::Receiver<()> {
        self.live_notify.subscribe()
    }

    fn notify_live(&self) {
        let _ = self.live_notify.send(());
    }

    pub async fn live_snapshot(&self) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        Ok(ProductionMapLiveSnapshot {
            maps: self.maps().await?,
            sequences: self.apparatus_sequences().await?,
            queue_states: self.apparatus_queue_states().await?,
            queue_policies: self.apparatus_queue_policy_records().await?,
        })
    }

    pub async fn maps(&self) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let maps = self.store.maps().await?;
        let mut saved = Vec::with_capacity(maps.len());
        for mut map in maps {
            // Legacy maps saved before `code` existed: expose the order
            // number as the code so clients never need a fallback.
            if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
                map.code = map.order_number.trim().to_string();
            }
            match compile_map(&map) {
                Ok(program) => saved.push(ProductionMapSaved { map, program }),
                Err(error) => {
                    tracing::warn!(
                        map_id = %map.id,
                        error = ?error,
                        "skipping invalid production map in list response"
                    );
                }
            }
        }
        Ok(saved)
    }

    pub async fn map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapSaved>, ProductionMapError> {
        let map_id = map_id.trim();
        if map_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let Some(mut map) = self.raw_map(map_id).await? else {
            return Ok(None);
        };
        if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
            map.code = map.order_number.trim().to_string();
        }
        let program = compile_map(&map)?;
        Ok(Some(ProductionMapSaved { map, program }))
    }

    pub async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        self.store.apparatus_sequences().await
    }

    pub async fn set_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let order_ids = order_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect();
        self.store
            .put_apparatus_sequence(apparatus, order_ids)
            .await?;
        self.notify_live();
        Ok(())
    }

    pub async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        self.store.apparatus_queue_states().await
    }

    pub async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        self.store
            .completed_queue_orders_for_actor(actor_ref, limit)
            .await
    }

    pub async fn apparatus_queue_policy_records(
        &self,
    ) -> Result<Vec<ApparatusQueuePolicyRecord>, ProductionMapError> {
        Ok(self
            .store
            .apparatus_queue_policies()
            .await?
            .into_iter()
            .map(|(apparatus, policy)| effective_apparatus_queue_policy_record(&apparatus, policy))
            .collect())
    }

    pub async fn set_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<ApparatusQueuePolicyRecord, ProductionMapError> {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let record = effective_apparatus_queue_policy_record(apparatus, policy);
        if record.locked && record.policy != policy {
            return Err(ProductionMapError::ApparatusQueuePolicyLocked);
        }
        self.store
            .put_apparatus_queue_policy(apparatus, record.policy, actor)
            .await?;
        self.notify_live();
        Ok(record)
    }

    pub async fn apply_apparatus_queue_action(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
    ) -> Result<BTreeMap<String, String>, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if !queue_state::apparatus_matches_assigned(apparatus, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .map(|key| key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let policy = effective_apparatus_queue_policy(
            apparatus,
            policies
                .get(&storage_key)
                .copied()
                .or_else(|| policies.get(apparatus).copied())
                .or_else(|| {
                    policies.iter().find_map(|(key, policy)| {
                        queue_state::apparatus_titles_match(key, apparatus).then_some(*policy)
                    })
                }),
        );
        let stored_sequence = sequences.get(&storage_key).cloned().unwrap_or_default();
        let all_maps = self.store.maps().await?;
        let visible_order_ids = visible_order_ids_for_apparatus(&all_maps, apparatus);
        let sequence =
            queue_state::effective_apparatus_sequence(&stored_sequence, &visible_order_ids);
        if !sequence.iter().any(|id| id.trim() == order_id) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let order_map = all_maps
            .iter()
            .find(|map| map.id.trim() == order_id)
            .ok_or(ProductionMapError::MapNotFound)?;
        if matches!(action, queue_state::ApparatusQueueAction::Start)
            && !chain::order_ready_for_station(
                order_map,
                order_id,
                apparatus,
                &all_states,
                &known_keys,
            )
        {
            return Err(ProductionMapError::PreviousStageNotCompleted);
        }
        let states = all_states.get(&storage_key).cloned().unwrap_or_default();
        let mut parsed = BTreeMap::new();
        for (id, value) in states {
            if let Some(state) = queue_state::ApparatusQueueOrderState::parse(&value) {
                parsed.insert(id, state);
            }
        }
        let from_state = parsed
            .get(order_id)
            .copied()
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        match policy {
            ApparatusQueuePolicy::StrictSequence => {
                queue_state::apply_queue_action(&sequence, &mut parsed, order_id, action)?;
            }
            ApparatusQueuePolicy::FreePick => {
                queue_state::apply_unordered_queue_action(&mut parsed, order_id, action)?;
            }
        }
        let to_state = parsed
            .get(order_id)
            .copied()
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let saved = parsed
            .into_iter()
            .map(|(id, state)| (id, state.as_str().to_string()))
            .collect::<BTreeMap<_, _>>();
        let event = ApparatusQueueActionEvent {
            event_id: queue_action_event_id(&storage_key, order_id, action),
            apparatus: storage_key.clone(),
            order_id: order_id.to_string(),
            action,
            from_state,
            to_state,
            policy,
            actor,
            assigned_apparatus: assigned_apparatus
                .iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            payload_json: serde_json::json!({
                "requested_apparatus": apparatus,
                "storage_key": storage_key,
                "sequence": sequence,
                "visible_order_ids": visible_order_ids,
                "from_state": from_state.as_str(),
                "to_state": to_state.as_str(),
                "policy": policy.as_str(),
            }),
        };
        self.store
            .put_apparatus_queue_states_with_event(&storage_key, saved.clone(), event)
            .await?;
        self.notify_live();
        Ok(saved)
    }

    pub async fn upsert_map(
        &self,
        mut map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        normalize_map(&mut map);
        let program = compile_map(&map)?;
        self.store.put_map(map.clone()).await?;
        self.notify_live();
        Ok(ProductionMapSaved { map, program })
    }

    #[allow(dead_code)]
    pub async fn upsert_maps_batch(
        &self,
        maps: Vec<ProductionMapDefinition>,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let mut normalized = Vec::with_capacity(maps.len());
        let mut saved = Vec::with_capacity(maps.len());
        for mut map in maps {
            normalize_map(&mut map);
            let program = compile_map(&map)?;
            saved.push(ProductionMapSaved {
                map: map.clone(),
                program,
            });
            normalized.push(map);
        }
        self.store.put_maps_batch(&normalized).await?;
        self.notify_live();
        Ok(saved)
    }

    pub async fn raw_map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapDefinition>, ProductionMapError> {
        let map_id = map_id.trim().to_ascii_lowercase();
        Ok(self
            .store
            .maps()
            .await?
            .into_iter()
            .find(|map| map.id.trim() == map_id))
    }

    pub async fn restore_map(
        &self,
        previous: Option<&ProductionMapDefinition>,
        map_id: &str,
    ) -> Result<(), ProductionMapError> {
        let result = match previous {
            Some(map) => self.store.put_map(map.clone()).await,
            None => self.store.delete_map(map_id).await,
        };
        if result.is_ok() {
            self.notify_live();
        }
        result
    }

    /// Moves multiple orders atomically: either every move succeeds or none
    /// are persisted.
    pub async fn move_apparatus_batch(
        &self,
        input: ProductionMapBatchMoveRequest,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if from.is_empty() || to.is_empty() || from == to {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let map_ids: Vec<String> = input
            .map_ids
            .iter()
            .map(|id| id.trim().to_ascii_lowercase())
            .filter(|id| !id.is_empty())
            .collect();
        if map_ids.is_empty() {
            return Err(ProductionMapError::MissingId);
        }

        let maps = self.store.maps().await?;
        let mut updated = Vec::with_capacity(map_ids.len());
        for map_id in &map_ids {
            let Some(map) = maps.iter().find(|item| item.id.trim() == map_id).cloned() else {
                return Err(ProductionMapError::MapNotFound);
            };
            if !move_allowed(&map, from, to) {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            let mut next = map;
            if !reassign_alternative_apparatus_assignment(&mut next, from, to)
                && !reassign_apparatus_nodes(&mut next, from, to)
            {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            updated.push(next);
        }

        self.store.put_maps_batch(&updated).await?;
        self.notify_live();
        updated
            .into_iter()
            .map(|map| {
                let program = compile_map(&map)?;
                Ok(ProductionMapSaved { map, program })
            })
            .collect()
    }

    /// Moves an order between apparatus, validating pechat rules server-side.
    pub async fn move_apparatus(
        &self,
        input: ProductionMapMoveRequest,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        let map_id = input.map_id.trim().to_ascii_lowercase();
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if map_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if to.is_empty() || from == to {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let maps = self.store.maps().await?;
        let Some(map) = maps.into_iter().find(|map| map.id.trim() == map_id) else {
            return Err(ProductionMapError::MapNotFound);
        };
        if !move_allowed(&map, from, to) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let mut next = map;
        if !reassign_alternative_apparatus_assignment(&mut next, from, to)
            && !reassign_apparatus_nodes(&mut next, from, to)
        {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        self.upsert_map(next).await
    }

    pub async fn run_map(
        &self,
        input: ProductionMapRunRequest,
    ) -> Result<ProductionMapRunResult, ProductionMapError> {
        if input.order_qty <= 0.0 {
            return Err(ProductionMapError::InvalidOrderQty);
        }
        let map_id = input.map_id.trim().to_ascii_lowercase();
        let product_code = input.product_code.trim();
        let maps = self.store.maps().await?;
        let Some(map) = maps.into_iter().find(|map| {
            (!map_id.is_empty() && map.id == map_id)
                || (!product_code.is_empty() && map.product_code == product_code)
        }) else {
            return Err(ProductionMapError::MapNotFound);
        };
        run_map_with_variables(&map, input.order_qty, input.variables)
    }
}

fn visible_order_ids_for_apparatus(
    maps: &[ProductionMapDefinition],
    apparatus: &str,
) -> Vec<String> {
    maps.iter()
        .filter(|map| {
            !flexo_order_blocked_for_color_pechat(map, apparatus)
                && chain::map_has_work_stage_for_station(map, apparatus)
        })
        .map(|map| map.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

fn move_allowed(map: &ProductionMapDefinition, from: &str, to: &str) -> bool {
    let from_is_laminatsiya = is_laminatsiya_title(from);
    let to_is_laminatsiya = is_laminatsiya_title(to);
    if from_is_laminatsiya || to_is_laminatsiya {
        return from_is_laminatsiya
            && to_is_laminatsiya
            && alternative_assigned_group_contains_target(map, from, to);
    }
    let Some(target_color) = pechat::pechat_color_count(to) else {
        return true;
    };
    if is_flexo_order(map) {
        return false;
    }
    let source_color = pechat::pechat_color_count(from).or_else(|| {
        pechat::order_pechat_color_count(
            map.nodes
                .iter()
                .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
                .map(|node| node.title.as_str()),
        )
    });
    pechat::pechat_can_move_order(target_color, map.roll_count, map.width_mm, source_color)
}

fn flexo_order_blocked_for_color_pechat(map: &ProductionMapDefinition, apparatus: &str) -> bool {
    is_flexo_order(map) && pechat::pechat_color_count(apparatus).is_some()
}

fn is_flexo_order(map: &ProductionMapDefinition) -> bool {
    let mut haystack = format!("{} {} {}", map.title, map.product_code, map.code).to_lowercase();
    for node in &map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus {
            continue;
        }
        haystack.push(' ');
        haystack.push_str(&node.title.to_lowercase());
        haystack.push(' ');
        haystack.push_str(&node.item_code.to_lowercase());
    }
    ["fleksa", "fleska", "flex", "flexe", "flexo"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
}

fn alternative_assigned_group_contains_target(
    map: &ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    let candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && queue_state::apparatus_titles_match(&node.alternative_assigned_title, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    if candidate_groups.is_empty() {
        return true;
    }
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
            && queue_state::apparatus_titles_match(&node.title, to)
    })
}

fn reassign_apparatus_nodes(map: &mut ProductionMapDefinition, from: &str, to: &str) -> bool {
    let to = to.trim();
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && queue_state::apparatus_titles_match(&node.title, from)
        {
            node.title = to.to_string();
            changed = true;
        }
    }
    changed
}

fn reassign_alternative_apparatus_assignment(
    map: &mut ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    let to = to.trim();
    if to.is_empty() {
        return false;
    }
    let candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && queue_state::apparatus_titles_match(&node.alternative_assigned_title, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    if candidate_groups.is_empty() {
        return false;
    }
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_title = to.to_string();
            changed = true;
        }
    }
    changed
}

pub fn compile_map(
    map: &ProductionMapDefinition,
) -> Result<ProductionMapProgram, ProductionMapError> {
    validate_map(map)?;
    let order = topological_order(map)?;
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut operations = Vec::with_capacity(order.len());
    for (index, node_id) in order.into_iter().enumerate() {
        let node = node_by_id
            .get(node_id.as_str())
            .expect("topological order only contains known node ids");
        operations.push(compile_node(index + 1, node)?);
    }
    Ok(ProductionMapProgram {
        map_id: map.id.clone(),
        product_code: map.product_code.clone(),
        operations,
    })
}

#[cfg(test)]
fn reject_order_number_immutable(
    maps: &BTreeMap<String, ProductionMapDefinition>,
    next: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let id = next.id.trim();
    if !id.starts_with("zakaz-") {
        return Ok(());
    }
    let order_number = next.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let Some(existing) = maps.get(id) else {
        return Ok(());
    };
    let existing_number = existing.order_number.trim();
    if !existing_number.is_empty() && existing_number != order_number {
        return Err(ProductionMapError::OrderNumberImmutable);
    }
    Ok(())
}

fn normalize_map(map: &mut ProductionMapDefinition) {
    map.id = map.id.trim().to_ascii_lowercase();
    map.product_code = map.product_code.trim().to_string();
    map.title = map.title.trim().to_string();
    map.code = map.code.trim().to_string();
    map.order_number = map.order_number.trim().to_string();
    if map
        .roll_count
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.roll_count = None;
    }
    if map
        .width_mm
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.width_mm = None;
    }
    for node in &mut map.nodes {
        node.id = node.id.trim().to_ascii_lowercase();
        node.title = node.title.trim().to_string();
        node.role_code = node.role_code.trim().to_string();
        node.item_code = node.item_code.trim().to_string();
        node.qty_formula = node.qty_formula.trim().to_string();
        node.from_location = node.from_location.trim().to_string();
        node.to_location = node.to_location.trim().to_string();
        node.alternative_group_id = node.alternative_group_id.trim().to_string();
        node.alternative_group_label = node.alternative_group_label.trim().to_string();
        node.alternative_assigned_title = node.alternative_assigned_title.trim().to_string();
        if !node.x.is_finite() {
            node.x = 0.0;
        }
        if !node.y.is_finite() {
            node.y = 0.0;
        }
        if let Some(formula) = &mut node.formula {
            formula.target = formula.target.trim().to_string();
            formula.expression = formula.expression.trim().to_string();
        }
    }
    for edge in &mut map.edges {
        edge.from = edge.from.trim().to_ascii_lowercase();
        edge.to = edge.to.trim().to_ascii_lowercase();
        edge.branch = normalize_branch(&edge.branch);
    }
}

fn validate_map(map: &ProductionMapDefinition) -> Result<(), ProductionMapError> {
    if map.id.trim().is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if map.product_code.trim().is_empty() {
        return Err(ProductionMapError::MissingProductCode);
    }
    if map.title.trim().is_empty() {
        return Err(ProductionMapError::MissingTitle);
    }
    if laminatsiya_rubber_too_large(map) {
        return Err(ProductionMapError::LaminatsiyaRubberTooLarge);
    }

    let mut ids = BTreeSet::new();
    let mut start_count = 0;
    let mut end_count = 0;
    for node in &map.nodes {
        if !ids.insert(node.id.as_str()) {
            return Err(ProductionMapError::DuplicateNode(node.id.clone()));
        }
        match node.kind {
            ProductionMapNodeKind::Start => start_count += 1,
            ProductionMapNodeKind::End => end_count += 1,
            ProductionMapNodeKind::Formula => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                if formula.target.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaTarget);
                }
                if formula.expression.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaExpression);
                }
                validate_formula_target(&formula.target)?;
                validate_formula_expression(&formula.expression)?;
            }
            ProductionMapNodeKind::Condition => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                if formula.expression.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaExpression);
                }
                validate_condition_expression(&formula.expression)?;
            }
            ProductionMapNodeKind::Location => {}
            ProductionMapNodeKind::Material
            | ProductionMapNodeKind::Apparatus
            | ProductionMapNodeKind::KkProduct
            | ProductionMapNodeKind::Task
            | ProductionMapNodeKind::Wait
            | ProductionMapNodeKind::Output => {
                if !node.qty_formula.trim().is_empty() {
                    validate_formula_expression(&node.qty_formula)?;
                }
            }
        }
        validate_location_ref(&node.from_location)?;
        validate_location_ref(&node.to_location)?;
    }
    if start_count != 1 {
        return Err(ProductionMapError::MissingStart);
    }
    if end_count != 1 {
        return Err(ProductionMapError::MissingEnd);
    }
    for edge in &map.edges {
        if !ids.contains(edge.from.as_str()) {
            return Err(ProductionMapError::MissingEdgeNode(edge.from.clone()));
        }
        if !ids.contains(edge.to.as_str()) {
            return Err(ProductionMapError::MissingEdgeNode(edge.to.clone()));
        }
    }
    for node in &map.nodes {
        if node.kind != ProductionMapNodeKind::Condition {
            continue;
        }
        let mut has_true = false;
        let mut has_false = false;
        for edge in map.edges.iter().filter(|edge| edge.from == node.id) {
            match normalize_branch(&edge.branch).as_str() {
                "true" => has_true = true,
                "false" => has_false = true,
                _ => {}
            }
        }
        if !has_true || !has_false {
            return Err(ProductionMapError::MissingConditionBranch);
        }
    }
    Ok(())
}

fn laminatsiya_rubber_too_large(map: &ProductionMapDefinition) -> bool {
    let Some(width_mm) = map.width_mm.filter(|value| *value > 0.0) else {
        return false;
    };
    if pechat::rubber_size_from_width(width_mm) <= MAX_LAMINATSIYA_RUBBER_SIZE_MM {
        return false;
    }
    map.nodes.iter().any(|node| {
        matches!(
            node.kind,
            ProductionMapNodeKind::Apparatus | ProductionMapNodeKind::Task
        ) && is_laminatsiya_title(&node.title)
    })
}

fn is_laminatsiya_title(title: &str) -> bool {
    title.trim().to_lowercase().contains("laminatsiya")
}

fn effective_apparatus_queue_policy(
    apparatus: &str,
    stored: Option<ApparatusQueuePolicy>,
) -> ApparatusQueuePolicy {
    if pechat::pechat_color_count(apparatus).is_some() {
        ApparatusQueuePolicy::StrictSequence
    } else {
        stored.unwrap_or(ApparatusQueuePolicy::StrictSequence)
    }
}

fn effective_apparatus_queue_policy_record(
    apparatus: &str,
    stored: ApparatusQueuePolicy,
) -> ApparatusQueuePolicyRecord {
    let locked = pechat::pechat_color_count(apparatus).is_some();
    ApparatusQueuePolicyRecord {
        apparatus: apparatus.trim().to_string(),
        policy: if locked {
            ApparatusQueuePolicy::StrictSequence
        } else {
            stored
        },
        locked,
        reason: if locked {
            "pechat_always_strict".to_string()
        } else {
            String::new()
        },
    }
}

fn queue_action_event_id(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!(
        "production-map-queue:{nanos}:{}:{}:{}",
        apparatus.trim(),
        order_id.trim(),
        match action {
            queue_state::ApparatusQueueAction::Start => "start",
            queue_state::ApparatusQueueAction::Complete => "complete",
        }
    )
}

fn validate_formula_target(target: &str) -> Result<(), ProductionMapError> {
    if is_identifier(target.trim()) {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidFormulaTarget(target.to_string()))
    }
}

fn validate_location_ref(location: &str) -> Result<(), ProductionMapError> {
    let location = location.trim();
    if location.is_empty() {
        return Ok(());
    }
    let valid = location.len() <= 120
        && location.chars().any(char::is_alphanumeric)
        && location.chars().all(|ch| {
            ch.is_alphanumeric()
                || ch.is_whitespace()
                || matches!(ch, '-' | '_' | '.' | '/' | '(' | ')')
        });
    if valid {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidLocation(location.to_string()))
    }
}

fn validate_formula_expression(expression: &str) -> Result<(), ProductionMapError> {
    let mut parser = FormulaParser::new(expression);
    parser.parse_expression()?;
    parser.skip_whitespace();
    if parser.is_eof() {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidFormulaExpression(
            expression.to_string(),
        ))
    }
}

fn validate_condition_expression(expression: &str) -> Result<(), ProductionMapError> {
    evaluate_condition(expression, &BTreeMap::new())
        .map(|_| ())
        .or_else(|error| {
            if matches!(error, ProductionMapError::UnknownFormulaVariable(_)) {
                Ok(())
            } else {
                Err(error)
            }
        })
}

fn evaluate_formula(
    expression: &str,
    variables: &BTreeMap<String, f64>,
) -> Result<f64, ProductionMapError> {
    let mut parser = FormulaParser::new(expression);
    let value = parser.evaluate_expression(variables)?;
    parser.skip_whitespace();
    if parser.is_eof() {
        Ok(value)
    } else {
        Err(ProductionMapError::InvalidFormulaExpression(
            expression.to_string(),
        ))
    }
}

fn evaluate_condition(
    expression: &str,
    variables: &BTreeMap<String, f64>,
) -> Result<bool, ProductionMapError> {
    if let Some((left, operator, right)) = split_condition(expression) {
        let left = evaluate_formula(left, variables)?;
        let right = evaluate_formula(right, variables)?;
        return match operator {
            ">" => Ok(left > right),
            ">=" => Ok(left >= right),
            "<" => Ok(left < right),
            "<=" => Ok(left <= right),
            "==" => Ok((left - right).abs() < f64::EPSILON),
            "!=" => Ok((left - right).abs() >= f64::EPSILON),
            _ => Err(ProductionMapError::InvalidFormulaExpression(
                expression.to_string(),
            )),
        };
    }
    Ok(evaluate_formula(expression, variables)? != 0.0)
}

fn split_condition(expression: &str) -> Option<(&str, &str, &str)> {
    let mut depth = 0usize;
    let bytes = expression.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                for operator in [">=", "<=", "==", "!=", ">", "<"] {
                    if expression[index..].starts_with(operator) {
                        let left = expression[..index].trim();
                        let right = expression[index + operator.len()..].trim();
                        if !left.is_empty() && !right.is_empty() {
                            return Some((left, operator, right));
                        }
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn normalize_branch(branch: &str) -> String {
    match branch.trim().to_ascii_lowercase().as_str() {
        "ha" | "yes" | "true" | "1" => "true".to_string(),
        "yo'q" | "yoq" | "no" | "false" | "0" => "false".to_string(),
        value => value.to_string(),
    }
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

struct FormulaParser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> FormulaParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn parse_expression(&mut self) -> Result<(), ProductionMapError> {
        self.parse_term()?;
        loop {
            self.skip_whitespace();
            if self.consume('+') || self.consume('-') {
                self.parse_term()?;
            } else {
                return Ok(());
            }
        }
    }

    fn evaluate_expression(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        let mut value = self.evaluate_term(variables)?;
        loop {
            self.skip_whitespace();
            if self.consume('+') {
                value += self.evaluate_term(variables)?;
            } else if self.consume('-') {
                value -= self.evaluate_term(variables)?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_term(&mut self) -> Result<(), ProductionMapError> {
        self.parse_factor()?;
        loop {
            self.skip_whitespace();
            if self.consume('*') || self.consume('/') {
                self.parse_factor()?;
            } else {
                return Ok(());
            }
        }
    }

    fn evaluate_term(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        let mut value = self.evaluate_factor(variables)?;
        loop {
            self.skip_whitespace();
            if self.consume('*') {
                value *= self.evaluate_factor(variables)?;
            } else if self.consume('/') {
                let divisor = self.evaluate_factor(variables)?;
                if divisor == 0.0 {
                    return Err(ProductionMapError::FormulaDivisionByZero);
                }
                value /= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_factor(&mut self) -> Result<(), ProductionMapError> {
        self.skip_whitespace();
        if self.consume('-') {
            return self.parse_factor();
        }
        if self.consume('(') {
            self.parse_expression()?;
            self.skip_whitespace();
            return if self.consume(')') {
                Ok(())
            } else {
                self.invalid()
            };
        }
        if self.parse_identifier() || self.parse_number() {
            Ok(())
        } else {
            self.invalid()
        }
    }

    fn evaluate_factor(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        self.skip_whitespace();
        if self.consume('-') {
            return Ok(-self.evaluate_factor(variables)?);
        }
        if self.consume('(') {
            let value = self.evaluate_expression(variables)?;
            self.skip_whitespace();
            return if self.consume(')') {
                Ok(value)
            } else {
                self.invalid()
            };
        }
        if let Some(identifier) = self.read_identifier() {
            return variables
                .get(&identifier)
                .copied()
                .ok_or(ProductionMapError::UnknownFormulaVariable(identifier));
        }
        if let Some(number) = self.read_number() {
            return Ok(number);
        }
        self.invalid()
    }

    fn parse_identifier(&mut self) -> bool {
        self.read_identifier().is_some()
    }

    fn read_identifier(&mut self) -> Option<String> {
        let start = self.position;
        while let Some(ch) = self.peek() {
            if self.position == start {
                if ch.is_ascii_alphabetic() || ch == '_' {
                    self.position += ch.len_utf8();
                } else {
                    break;
                }
            } else if ch.is_ascii_alphanumeric() || ch == '_' {
                self.position += ch.len_utf8();
            } else {
                break;
            }
        }
        (self.position > start).then(|| self.input[start..self.position].to_string())
    }

    fn parse_number(&mut self) -> bool {
        self.read_number().is_some()
    }

    fn read_number(&mut self) -> Option<f64> {
        let start = self.position;
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
            self.position += 1;
        }
        if self.consume('.') {
            while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
                self.position += 1;
            }
        }
        (self.position > start)
            .then(|| self.input[start..self.position].parse::<f64>().ok())
            .flatten()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.position >= self.input.len()
    }

    fn invalid<T>(&self) -> Result<T, ProductionMapError> {
        Err(ProductionMapError::InvalidFormulaExpression(
            self.input.to_string(),
        ))
    }
}

#[cfg(test)]
pub fn run_map(
    map: &ProductionMapDefinition,
    order_qty: f64,
) -> Result<ProductionMapRunResult, ProductionMapError> {
    run_map_with_variables(map, order_qty, BTreeMap::new())
}

pub fn run_map_with_variables(
    map: &ProductionMapDefinition,
    order_qty: f64,
    run_variables: BTreeMap<String, f64>,
) -> Result<ProductionMapRunResult, ProductionMapError> {
    if order_qty <= 0.0 {
        return Err(ProductionMapError::InvalidOrderQty);
    }
    compile_map(map)?;
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut outgoing = BTreeMap::<&str, Vec<&ProductionMapEdge>>::new();
    for edge in &map.edges {
        outgoing.entry(edge.from.as_str()).or_default().push(edge);
    }
    let mut variables = input_variables(order_qty, run_variables);
    let mut tasks = Vec::new();
    let Some(mut current_id) = map
        .nodes
        .iter()
        .find(|node| node.kind == ProductionMapNodeKind::Start)
        .map(|node| node.id.as_str())
    else {
        return Err(ProductionMapError::MissingStart);
    };
    let mut visited = BTreeSet::new();
    let mut visited_node_ids = Vec::new();
    while visited.insert(current_id.to_string()) {
        let node = node_by_id
            .get(current_id)
            .expect("compiled map only contains known node ids");
        visited_node_ids.push(node.id.clone());
        if node.kind == ProductionMapNodeKind::End {
            break;
        }
        match node.kind {
            ProductionMapNodeKind::Formula => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                let value = evaluate_formula(&formula.expression, &variables)?;
                variables.insert(formula.target.clone(), value);
            }
            ProductionMapNodeKind::Condition => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                let result = match evaluate_condition(&formula.expression, &variables) {
                    Ok(result) => result,
                    Err(ProductionMapError::UnknownFormulaVariable(variable)) => {
                        return Ok(ProductionMapRunResult {
                            map_id: map.id.clone(),
                            product_code: map.product_code.clone(),
                            order_qty,
                            variables,
                            tasks,
                            visited_node_ids,
                            awaiting_node_id: node.id.clone(),
                            awaiting_variable: variable,
                            awaiting_expression: formula.expression.clone(),
                        });
                    }
                    Err(error) => return Err(error),
                };
                variables.insert(node.id.clone(), if result { 1.0 } else { 0.0 });
            }
            ProductionMapNodeKind::Location => {}
            ProductionMapNodeKind::Material
            | ProductionMapNodeKind::Apparatus
            | ProductionMapNodeKind::KkProduct
            | ProductionMapNodeKind::Task
            | ProductionMapNodeKind::Wait
            | ProductionMapNodeKind::Output => {
                let qty = node_qty(node, order_qty, &variables)?;
                tasks.push(ProductionTaskDraft {
                    order: tasks.len() + 1,
                    node_id: node.id.clone(),
                    task_kind: compile_node(tasks.len() + 1, node)?.op_code,
                    title: node.title.clone(),
                    role_code: node.role_code.clone(),
                    item_code: node.item_code.clone(),
                    from_location: node.from_location.clone(),
                    to_location: node.to_location.clone(),
                    qty,
                })
            }
            ProductionMapNodeKind::Start | ProductionMapNodeKind::End => {}
        }
        let edges = outgoing.get(current_id).cloned().unwrap_or_default();
        if node.kind == ProductionMapNodeKind::Condition {
            let branch = if variables.get(&node.id).copied().unwrap_or(0.0) != 0.0 {
                "true"
            } else {
                "false"
            };
            let Some(next) = edges
                .into_iter()
                .find(|edge| normalize_branch(&edge.branch) == branch)
            else {
                return Err(ProductionMapError::MissingConditionBranch);
            };
            current_id = next.to.as_str();
        } else {
            let Some(next) = edges.first() else {
                break;
            };
            current_id = next.to.as_str();
        }
    }
    Ok(ProductionMapRunResult {
        map_id: map.id.clone(),
        product_code: map.product_code.clone(),
        order_qty,
        variables,
        tasks,
        visited_node_ids,
        awaiting_node_id: String::new(),
        awaiting_variable: String::new(),
        awaiting_expression: String::new(),
    })
}

fn input_variables(order_qty: f64, mut variables: BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    variables.insert("order_qty".to_string(), order_qty);
    variables
}

fn node_qty(
    node: &ProductionMapNode,
    order_qty: f64,
    variables: &BTreeMap<String, f64>,
) -> Result<f64, ProductionMapError> {
    let qty = if node.qty_formula.trim().is_empty() {
        order_qty
    } else {
        evaluate_formula(&node.qty_formula, variables)?
    };
    if qty.is_finite() && qty > 0.0 {
        Ok(qty)
    } else {
        Err(ProductionMapError::InvalidNodeQty(node.id.clone()))
    }
}

fn topological_order(map: &ProductionMapDefinition) -> Result<Vec<String>, ProductionMapError> {
    let mut indegree = BTreeMap::<String, usize>::new();
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();
    for node in &map.nodes {
        indegree.insert(node.id.clone(), 0);
        outgoing.insert(node.id.clone(), Vec::new());
    }
    for edge in &map.edges {
        *indegree
            .get_mut(&edge.to)
            .expect("validated edge target exists") += 1;
        outgoing
            .get_mut(&edge.from)
            .expect("validated edge source exists")
            .push(edge.to.clone());
    }

    let mut queue = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect::<VecDeque<_>>();
    let mut order = Vec::new();
    while let Some(id) = queue.pop_front() {
        order.push(id.clone());
        for child in outgoing.get(&id).into_iter().flatten() {
            let count = indegree
                .get_mut(child)
                .expect("validated child exists in indegree map");
            *count = count.saturating_sub(1);
            if *count == 0 {
                queue.push_back(child.clone());
            }
        }
    }
    if order.len() != map.nodes.len() {
        return Err(ProductionMapError::Cycle);
    }
    Ok(order)
}

fn compile_node(
    order: usize,
    node: &ProductionMapNode,
) -> Result<ProductionMapOperation, ProductionMapError> {
    let mut args = BTreeMap::new();
    args.insert("title".to_string(), node.title.clone());
    if !node.role_code.is_empty() {
        args.insert("role_code".to_string(), node.role_code.clone());
    }
    if !node.item_code.is_empty() {
        args.insert("item_code".to_string(), node.item_code.clone());
    }
    if !node.qty_formula.is_empty() {
        args.insert("qty_formula".to_string(), node.qty_formula.clone());
    }
    if !node.from_location.is_empty() {
        args.insert("from_location".to_string(), node.from_location.clone());
    }
    if !node.to_location.is_empty() {
        args.insert("to_location".to_string(), node.to_location.clone());
    }
    if !node.alternative_group_id.is_empty() {
        args.insert(
            "alternative_group_id".to_string(),
            node.alternative_group_id.clone(),
        );
    }
    if !node.alternative_group_label.is_empty() {
        args.insert(
            "alternative_group_label".to_string(),
            node.alternative_group_label.clone(),
        );
    }
    if !node.alternative_assigned_title.is_empty() {
        args.insert(
            "alternative_assigned_title".to_string(),
            node.alternative_assigned_title.clone(),
        );
    }
    let op_code = match node.kind {
        ProductionMapNodeKind::Start => "start",
        ProductionMapNodeKind::Location => "warehouse_location",
        ProductionMapNodeKind::Material => "require_material",
        ProductionMapNodeKind::Apparatus => "apparatus",
        ProductionMapNodeKind::KkProduct => "kk_product",
        ProductionMapNodeKind::Formula => {
            let Some(formula) = &node.formula else {
                return Err(ProductionMapError::MissingFormulaExpression);
            };
            args.insert("target".to_string(), formula.target.clone());
            args.insert("expression".to_string(), formula.expression.clone());
            "calculate"
        }
        ProductionMapNodeKind::Condition => {
            let Some(formula) = &node.formula else {
                return Err(ProductionMapError::MissingFormulaExpression);
            };
            args.insert("expression".to_string(), formula.expression.clone());
            "condition"
        }
        ProductionMapNodeKind::Task => "create_task",
        ProductionMapNodeKind::Wait => "wait_dependency",
        ProductionMapNodeKind::Output => "produce_output",
        ProductionMapNodeKind::End => "end",
    };
    Ok(ProductionMapOperation {
        order,
        node_id: node.id.clone(),
        op_code: op_code.to_string(),
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_map_turns_visual_nodes_into_ordered_operations() {
        let map = sample_map();
        let program = compile_map(&map).expect("compile");

        assert_eq!(program.map_id, "hotlunch-test");
        assert_eq!(program.operations.len(), 4);
        assert_eq!(program.operations[1].op_code, "calculate");
        assert_eq!(
            program.operations[1]
                .args
                .get("expression")
                .map(String::as_str),
            Some("order_qty * 1.08")
        );
        assert_eq!(program.operations[2].op_code, "create_task");
    }

    #[test]
    fn compile_map_accepts_location_markers_without_task_drafts() {
        let mut map = sample_map();
        map.nodes.insert(
            1,
            ProductionMapNode {
                id: "cpp_warehouse".to_string(),
                kind: ProductionMapNodeKind::Location,
                title: "CPP ombor".to_string(),
                formula: None,
                role_code: String::new(),
                item_code: String::new(),
                qty_formula: String::new(),
                from_location: String::new(),
                to_location: String::new(),
                alternative_group_id: String::new(),
                alternative_group_label: String::new(),
                alternative_assigned_title: String::new(),
                x: 0.0,
                y: 0.0,
            },
        );
        map.edges[0].to = "cpp_warehouse".to_string();
        map.edges.insert(
            1,
            ProductionMapEdge {
                from: "cpp_warehouse".to_string(),
                to: "formula".to_string(),
                branch: String::new(),
            },
        );

        let program = compile_map(&map).expect("compile");
        assert_eq!(program.operations[1].op_code, "warehouse_location");

        let result = run_map(&map, 100.0).expect("run map");
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks[0].node_id, "task");
    }

    #[test]
    fn compile_map_rejects_cycles() {
        let mut map = sample_map();
        map.edges.push(ProductionMapEdge {
            from: "task".to_string(),
            to: "formula".to_string(),
            branch: String::new(),
        });

        assert_eq!(compile_map(&map), Err(ProductionMapError::Cycle));
    }

    #[test]
    fn compile_map_rejects_invalid_formula_expression() {
        let mut map = sample_map();
        map.nodes[1].formula = Some(ProductionFormula {
            target: "cpp_kg".to_string(),
            expression: "order_qty; drop".to_string(),
        });

        assert_eq!(
            compile_map(&map),
            Err(ProductionMapError::InvalidFormulaExpression(
                "order_qty; drop".to_string()
            ))
        );
    }

    #[test]
    fn run_map_evaluates_formulas_and_generates_task_drafts() {
        let result = run_map(&sample_map(), 100.0).expect("run map");

        assert_eq!(result.map_id, "hotlunch-test");
        assert_eq!(result.variables.get("order_qty"), Some(&100.0));
        assert_eq!(result.variables.get("cpp_kg"), Some(&108.0));
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks[0].task_kind, "create_task");
        assert_eq!(result.tasks[0].role_code, "rezkachi");
        assert_eq!(result.tasks[0].qty, 108.0);
        assert_eq!(result.tasks[0].from_location, "CPP ombor");
        assert_eq!(result.tasks[0].to_location, "Rezka apparat");
        assert_eq!(result.visited_node_ids, ["start", "formula", "task", "end"]);
    }

    #[test]
    fn run_map_follows_condition_branch() {
        let result = run_map(&condition_map(), 120.0).expect("run map");

        assert_eq!(result.variables.get("large_order"), Some(&1.0));
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks[0].node_id, "large_task");

        let result = run_map(&condition_map(), 60.0).expect("run map");
        assert_eq!(result.variables.get("large_order"), Some(&0.0));
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks[0].node_id, "small_task");
    }

    #[test]
    fn run_map_conditions_can_use_runtime_variables() {
        let mut map = condition_map();
        map.nodes[1].formula = Some(ProductionFormula {
            target: String::new(),
            expression: "pechat_ok == 1".to_string(),
        });

        let result = run_map_with_variables(
            &map,
            100.0,
            BTreeMap::from([("pechat_ok".to_string(), 1.0)]),
        )
        .expect("run map with ok result");

        assert_eq!(result.variables.get("pechat_ok"), Some(&1.0));
        assert!(result.awaiting_variable.is_empty());
        assert_eq!(result.tasks[0].node_id, "large_task");

        let result = run_map_with_variables(
            &map,
            100.0,
            BTreeMap::from([("pechat_ok".to_string(), 0.0)]),
        )
        .expect("run map with failed result");
        assert_eq!(result.tasks[0].node_id, "small_task");
    }

    #[test]
    fn run_map_stops_at_condition_when_runtime_variable_is_missing() {
        let mut map = condition_map();
        map.nodes[1].formula = Some(ProductionFormula {
            target: String::new(),
            expression: "pechat_ok == 1".to_string(),
        });

        let result = run_map(&map, 100.0).expect("run map waiting for variable");

        assert_eq!(result.tasks.len(), 0);
        assert_eq!(result.awaiting_node_id, "large_order");
        assert_eq!(result.awaiting_variable, "pechat_ok");
        assert_eq!(result.awaiting_expression, "pechat_ok == 1");
        assert_eq!(result.visited_node_ids, ["start", "large_order"]);
    }

    #[test]
    fn run_map_rejects_non_positive_node_qty() {
        let mut map = sample_map();
        map.nodes[2].qty_formula = "order_qty - 100".to_string();

        assert_eq!(
            run_map(&map, 100.0),
            Err(ProductionMapError::InvalidNodeQty("task".to_string()))
        );
    }

    #[tokio::test]
    async fn maps_skips_legacy_invalid_map_without_failing_list() {
        let store = std::sync::Arc::new(MemoryProductionMapStore::new());
        let mut valid = sample_map();
        valid.id = "valid-map".to_string();
        let mut invalid = sample_map();
        invalid.id = "invalid-laminatsiya".to_string();
        invalid.width_mm = Some(1070.0);
        invalid.nodes[2].title = "Laminatsiya".to_string();

        store.put_map(valid).await.expect("valid insert");
        store.put_map(invalid).await.expect("invalid legacy insert");

        let service = ProductionMapService::new(store);
        let maps = service.maps().await.expect("list");
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].map.id, "valid-map");
        assert_eq!(
            service.map("invalid-laminatsiya").await,
            Err(ProductionMapError::LaminatsiyaRubberTooLarge)
        );
    }

    #[tokio::test]
    async fn free_pick_policy_allows_ready_order_outside_sequence_head() {
        let store = std::sync::Arc::new(MemoryProductionMapStore::new());
        let service = ProductionMapService::new(store);
        let actor = QueueActionActor {
            role: "admin".to_string(),
            ref_: "admin".to_string(),
            display_name: "Admin".to_string(),
        };
        let first = apparatus_stage_map("zakaz-1", "Rezka apparat");
        let second = apparatus_stage_map("zakaz-2", "Rezka apparat");
        service.upsert_map(first).await.expect("first map");
        service.upsert_map(second).await.expect("second map");
        service
            .set_apparatus_sequence(
                "Rezka apparat",
                vec!["zakaz-1".to_string(), "zakaz-2".to_string()],
            )
            .await
            .expect("sequence");

        let strict_result = service
            .apply_apparatus_queue_action(
                "Rezka apparat",
                "zakaz-2",
                queue_state::ApparatusQueueAction::Start,
                &["Rezka apparat".to_string()],
                actor.clone(),
            )
            .await;
        assert_eq!(
            strict_result,
            Err(ProductionMapError::QueueActionNotAllowed)
        );

        service
            .set_apparatus_queue_policy("Rezka apparat", ApparatusQueuePolicy::FreePick, &actor)
            .await
            .expect("free pick policy");
        let states = service
            .apply_apparatus_queue_action(
                "Rezka apparat",
                "zakaz-2",
                queue_state::ApparatusQueueAction::Start,
                &["Rezka apparat".to_string()],
                actor,
            )
            .await
            .expect("start second");
        assert_eq!(states.get("zakaz-2"), Some(&"in_progress".to_string()));
    }

    #[tokio::test]
    async fn pechat_queue_policy_is_always_locked_strict() {
        let service =
            ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
        let actor = QueueActionActor {
            role: "admin".to_string(),
            ref_: "admin".to_string(),
            display_name: "Admin".to_string(),
        };
        let result = service
            .set_apparatus_queue_policy(
                "7 ta rangli pechat",
                ApparatusQueuePolicy::FreePick,
                &actor,
            )
            .await;
        assert_eq!(result, Err(ProductionMapError::ApparatusQueuePolicyLocked));
    }

    #[tokio::test]
    async fn raw_material_assignment_requires_exact_scan_before_start() {
        let service =
            ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
        let actor = QueueActionActor {
            role: "aparatchi".to_string(),
            ref_: "worker-1".to_string(),
            display_name: "Worker 1".to_string(),
        };
        service
            .upsert_map(apparatus_stage_map("zakaz-raw-1", "7 ta rangli pechat - A"))
            .await
            .expect("map");
        service
            .set_apparatus_material_rule(ApparatusMaterialRuleUpsert {
                apparatus: "7 ta rangli pechat - A".to_string(),
                requires_material: true,
                item_groups: vec!["Kraska".to_string()],
            })
            .await
            .expect("material rule");
        let missing_assignment = service
            .apply_apparatus_queue_action_with_material_scan(
                "7 ta rangli pechat - A",
                "zakaz-raw-1",
                queue_state::ApparatusQueueAction::Start,
                &["7 ta rangli pechat - A".to_string()],
                actor.clone(),
                "",
            )
            .await;
        assert_eq!(
            missing_assignment,
            Err(ProductionMapError::RawMaterialAssignmentNotFound)
        );
        let assigned = service
            .assign_raw_material_to_order(
                RawMaterialAssignmentInput {
                    order_id: "zakaz-raw-1".to_string(),
                    barcode: "30AA".to_string(),
                    item_code: "INK-BLACK".to_string(),
                    item_name: "Black ink".to_string(),
                    item_group: "Kraska".to_string(),
                },
                &actor,
            )
            .await
            .expect("assign material");
        assert_eq!(assigned.apparatus, "7 ta rangli pechat - A");
        let second_assigned = service
            .assign_raw_material_to_order(
                RawMaterialAssignmentInput {
                    order_id: "zakaz-raw-1".to_string(),
                    barcode: "30CC".to_string(),
                    item_code: "INK-WHITE".to_string(),
                    item_name: "White ink".to_string(),
                    item_group: "Kraska".to_string(),
                },
                &actor,
            )
            .await
            .expect("assign second material");
        assert_eq!(second_assigned.apparatus, "7 ta rangli pechat - A");

        service
            .upsert_map(apparatus_stage_map("zakaz-raw-2", "7 ta rangli pechat - A"))
            .await
            .expect("second map");
        let duplicate = service
            .assign_raw_material_to_order(
                RawMaterialAssignmentInput {
                    order_id: "zakaz-raw-2".to_string(),
                    barcode: "30AA".to_string(),
                    item_code: "INK-BLACK".to_string(),
                    item_name: "Black ink".to_string(),
                    item_group: "Kraska".to_string(),
                },
                &actor,
            )
            .await;
        assert_eq!(
            duplicate,
            Err(ProductionMapError::RawMaterialAlreadyAssigned)
        );

        let not_assigned = service
            .apply_apparatus_queue_action_with_material_scan(
                "7 ta rangli pechat - A",
                "zakaz-raw-1",
                queue_state::ApparatusQueueAction::Start,
                &["Rezka apparat".to_string()],
                actor.clone(),
                "",
            )
            .await;
        assert_eq!(not_assigned, Err(ProductionMapError::ApparatusNotAssigned));

        let missing_scan = service
            .apply_apparatus_queue_action_with_material_scan(
                "7 ta rangli pechat - A",
                "zakaz-raw-1",
                queue_state::ApparatusQueueAction::Start,
                &["7 ta rangli pechat - A".to_string()],
                actor.clone(),
                "",
            )
            .await;
        assert_eq!(
            missing_scan,
            Err(ProductionMapError::RawMaterialScanRequired)
        );

        let wrong_scan = service
            .apply_apparatus_queue_action_with_material_scan(
                "7 ta rangli pechat - A",
                "zakaz-raw-1",
                queue_state::ApparatusQueueAction::Start,
                &["7 ta rangli pechat - A".to_string()],
                actor.clone(),
                "30BB",
            )
            .await;
        assert_eq!(wrong_scan, Err(ProductionMapError::RawMaterialMismatch));

        let partial_scan = service
            .apply_apparatus_queue_action_with_material_scan(
                "7 ta rangli pechat - A",
                "zakaz-raw-1",
                queue_state::ApparatusQueueAction::Start,
                &["7 ta rangli pechat - A".to_string()],
                actor.clone(),
                "30AA",
            )
            .await;
        assert_eq!(partial_scan, Err(ProductionMapError::RawMaterialMismatch));

        let states = service
            .apply_apparatus_queue_action_with_material_scan(
                "7 ta rangli pechat - A",
                "zakaz-raw-1",
                queue_state::ApparatusQueueAction::Start,
                &["7 ta rangli pechat - A".to_string()],
                actor,
                "30AA,30CC",
            )
            .await
            .expect("start with exact material");
        assert_eq!(states.get("zakaz-raw-1"), Some(&"in_progress".to_string()));
    }

    #[tokio::test]
    async fn upsert_maps_batch_keeps_queue_state_and_sequence_cache() {
        let store = std::sync::Arc::new(MemoryProductionMapStore::new());
        let service = ProductionMapService::new(store);
        service
            .set_apparatus_sequence(
                "7 ta rangli pechat - A",
                vec!["zakaz-111".to_string(), "zakaz-222".to_string()],
            )
            .await
            .expect("sequence");
        service
            .store
            .put_apparatus_queue_states(
                "7 ta rangli pechat - A",
                BTreeMap::from([("zakaz-111".to_string(), "completed".to_string())]),
            )
            .await
            .expect("queue state");
        let mut first = sample_map();
        first.id = "zakaz-111".to_string();
        first.order_number = "111".to_string();
        first.code = "111".to_string();
        let mut second = sample_map();
        second.id = "zakaz-222".to_string();
        second.order_number = "222".to_string();
        second.code = "222".to_string();

        let saved = service
            .upsert_maps_batch(vec![first, second])
            .await
            .expect("batch upsert");

        assert_eq!(saved.len(), 2);
        assert_eq!(service.maps().await.expect("maps").len(), 2);
        assert_eq!(
            service
                .apparatus_sequences()
                .await
                .expect("sequences")
                .get("7 ta rangli pechat - A"),
            Some(&vec!["zakaz-111".to_string(), "zakaz-222".to_string()])
        );
        assert_eq!(
            service
                .apparatus_queue_states()
                .await
                .expect("states")
                .get("7 ta rangli pechat - A")
                .and_then(|states| states.get("zakaz-111")),
            Some(&"completed".to_string())
        );
    }

    fn sample_map() -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: "hotlunch-test".to_string(),
            product_code: "HOTLUNCH".to_string(),
            title: "Hotlunch test".to_string(),
            code: String::new(),
            order_number: String::new(),
            roll_count: None,
            width_mm: None,
            nodes: vec![
                ProductionMapNode {
                    id: "start".to_string(),
                    kind: ProductionMapNodeKind::Start,
                    title: "Start".to_string(),
                    formula: None,
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "formula".to_string(),
                    kind: ProductionMapNodeKind::Formula,
                    title: "CPP hisob".to_string(),
                    formula: Some(ProductionFormula {
                        target: "cpp_kg".to_string(),
                        expression: "order_qty * 1.08".to_string(),
                    }),
                    role_code: String::new(),
                    item_code: "CPP".to_string(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "task".to_string(),
                    kind: ProductionMapNodeKind::Task,
                    title: "Rezkaga yuborish".to_string(),
                    formula: None,
                    role_code: "rezkachi".to_string(),
                    item_code: String::new(),
                    qty_formula: "cpp_kg".to_string(),
                    from_location: "CPP ombor".to_string(),
                    to_location: "Rezka apparat".to_string(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "end".to_string(),
                    kind: ProductionMapNodeKind::End,
                    title: "End".to_string(),
                    formula: None,
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
            ],
            edges: vec![
                ProductionMapEdge {
                    from: "start".to_string(),
                    to: "formula".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "formula".to_string(),
                    to: "task".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "task".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
            ],
        }
    }

    fn apparatus_stage_map(id: &str, apparatus: &str) -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: id.to_string(),
            product_code: format!("{id}-product"),
            title: id.to_string(),
            code: String::new(),
            order_number: String::new(),
            roll_count: None,
            width_mm: None,
            nodes: vec![
                ProductionMapNode {
                    id: "start".to_string(),
                    kind: ProductionMapNodeKind::Start,
                    title: "Start".to_string(),
                    formula: None,
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "apparatus".to_string(),
                    kind: ProductionMapNodeKind::Apparatus,
                    title: apparatus.to_string(),
                    formula: None,
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 132.0,
                },
                ProductionMapNode {
                    id: "end".to_string(),
                    kind: ProductionMapNodeKind::End,
                    title: "End".to_string(),
                    formula: None,
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 264.0,
                },
            ],
            edges: vec![
                ProductionMapEdge {
                    from: "start".to_string(),
                    to: "apparatus".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "apparatus".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
            ],
        }
    }

    fn condition_map() -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: "branch-test".to_string(),
            product_code: "HOTLUNCH".to_string(),
            title: "Branch test".to_string(),
            code: String::new(),
            order_number: String::new(),
            roll_count: None,
            width_mm: None,
            nodes: vec![
                ProductionMapNode {
                    id: "start".to_string(),
                    kind: ProductionMapNodeKind::Start,
                    title: "Start".to_string(),
                    formula: None,
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "large_order".to_string(),
                    kind: ProductionMapNodeKind::Condition,
                    title: "Katta partiyami".to_string(),
                    formula: Some(ProductionFormula {
                        target: String::new(),
                        expression: "order_qty >= 100".to_string(),
                    }),
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "large_task".to_string(),
                    kind: ProductionMapNodeKind::Task,
                    title: "Katta partiya".to_string(),
                    formula: None,
                    role_code: "rezkachi".to_string(),
                    item_code: String::new(),
                    qty_formula: "order_qty / 6".to_string(),
                    from_location: "CPP ombor".to_string(),
                    to_location: "Rezka apparat".to_string(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "small_task".to_string(),
                    kind: ProductionMapNodeKind::Task,
                    title: "Oddiy partiya".to_string(),
                    formula: None,
                    role_code: "operator".to_string(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
                ProductionMapNode {
                    id: "end".to_string(),
                    kind: ProductionMapNodeKind::End,
                    title: "End".to_string(),
                    formula: None,
                    role_code: String::new(),
                    item_code: String::new(),
                    qty_formula: String::new(),
                    from_location: String::new(),
                    to_location: String::new(),
                    alternative_group_id: String::new(),
                    alternative_group_label: String::new(),
                    alternative_assigned_title: String::new(),
                    x: 0.0,
                    y: 0.0,
                },
            ],
            edges: vec![
                ProductionMapEdge {
                    from: "start".to_string(),
                    to: "large_order".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "large_order".to_string(),
                    to: "large_task".to_string(),
                    branch: "true".to_string(),
                },
                ProductionMapEdge {
                    from: "large_order".to_string(),
                    to: "small_task".to_string(),
                    branch: "false".to_string(),
                },
                ProductionMapEdge {
                    from: "large_task".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "small_task".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
            ],
        }
    }
}
