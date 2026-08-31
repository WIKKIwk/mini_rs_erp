/// Parse a live apparatus reference at the progress boundary.
///
/// Progress records may retain display snapshots in their existing string
/// fields, but identity matching is only valid for canonical IDs. In
/// particular, this intentionally does not resolve a title or warehouse
/// instance to an ID.
pub fn canonical_apparatus_id(value: &str) -> Option<ApparatusId> {
    ApparatusId::new(value.trim().to_string()).ok()
}

pub fn canonical_apparatus_key(value: &str) -> String {
    canonical_apparatus_id(value)
        .map(|id| id.as_str().to_string())
        .unwrap_or_default()
}

pub fn apparatus_ids_match(left: &str, right: &str) -> bool {
    match (canonical_apparatus_id(left), canonical_apparatus_id(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// Compare a topology stage identity. Apparatus stages use canonical
/// `ApparatusId`; non-apparatus task stages use their stable graph identity.
/// This is an exact identity comparison and never resolves display titles.
pub fn stage_ids_match(left: &str, right: &str) -> bool {
    if apparatus_ids_match(left, right) {
        return true;
    }
    let left = left.trim();
    let right = right.trim();
    is_stable_task_stage_id(left) && is_stable_task_stage_id(right) && left == right
}

/// Validated Qolip lineage carried by a progress/session payload.
///
/// The payload remains backward-compatible with the existing JSON fields, but
/// every producer/consumer crosses this typed boundary so a malformed or
/// title-derived value cannot become lineage state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct QolipLineage {
    pub(crate) qolip_code: String,
    #[serde(default)]
    pub(crate) qolip_codes: Vec<String>,
}

impl QolipLineage {
    pub(crate) fn from_codes(codes: &[String]) -> Option<Self> {
        let mut normalized = Vec::new();
        for code in codes {
            let code = code.trim();
            if code.is_empty()
                || normalized
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(code))
            {
                continue;
            }
            normalized.push(code.to_string());
        }
        normalized.first().cloned().map(|qolip_code| Self {
            qolip_code,
            qolip_codes: normalized,
        })
    }

    pub(crate) fn from_payload(payload: &serde_json::Value) -> Option<Self> {
        let primary = payload
            .get("qolip_code")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .map(str::to_string);
        let mut codes = Vec::new();
        if let Some(values) = payload.get("qolip_codes") {
            for value in values.as_array()? {
                let code = value.as_str()?.trim();
                if code.is_empty() {
                    return None;
                }
                codes.push(code.to_string());
            }
        }
        if let Some(primary) = primary {
            codes.insert(0, primary);
        }
        Self::from_codes(&codes)
    }

    pub(crate) fn write_to_payload(&self, payload: &mut serde_json::Value) {
        if !payload.is_object() {
            *payload = serde_json::json!({});
        }
        payload["qolip_code"] = serde_json::json!(self.qolip_code);
        payload["qolip_codes"] = serde_json::json!(self.qolip_codes);
    }
}

pub(crate) fn qolip_lineage_from_batch(batch: &OrderProgressBatch) -> Option<QolipLineage> {
    QolipLineage::from_payload(&batch.payload_json)
}

fn is_stable_task_stage_id(value: &str) -> bool {
    let Some(task_id) = value.strip_prefix("task:") else {
        return false;
    };
    !task_id.is_empty()
        && canonical_apparatus_id(value).is_none()
        && task_id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | ':' | '.')
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunStatus {
    Active,
    Paused,
    Frozen,
    RollDetached,
    Completed,
}

impl OrderRunStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "frozen" => Some(Self::Frozen),
            "roll_detached" => Some(Self::RollDetached),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Frozen => "frozen",
            Self::RollDetached => "roll_detached",
            Self::Completed => "completed",
        }
    }

    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Paused | Self::Frozen | Self::RollDetached
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchStatus {
    Paused,
    RollDetached,
    Completed,
    Resumed,
}

impl OrderProgressBatchStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "paused" => Some(Self::Paused),
            "roll_detached" => Some(Self::RollDetached),
            "completed" => Some(Self::Completed),
            "resumed" => Some(Self::Resumed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::RollDetached => "roll_detached",
            Self::Completed => "completed",
            Self::Resumed => "resumed",
        }
    }

    pub const fn is_resumable(self) -> bool {
        matches!(self, Self::Paused | Self::RollDetached)
    }
}

#[cfg(test)]
mod order_progress_batch_status_tests {
    use super::OrderProgressBatchStatus;

    #[test]
    fn resumable_status_classification_is_canonical() {
        assert!(OrderProgressBatchStatus::Paused.is_resumable());
        assert!(OrderProgressBatchStatus::RollDetached.is_resumable());
        assert!(!OrderProgressBatchStatus::Completed.is_resumable());
        assert!(!OrderProgressBatchStatus::Resumed.is_resumable());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchWipStatus {
    Waiting,
    InUse,
    Processed,
}

impl OrderProgressBatchWipStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "waiting" => Some(Self::Waiting),
            "in_use" => Some(Self::InUse),
            "processed" => Some(Self::Processed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::InUse => "in_use",
            Self::Processed => "processed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunInputSourceKind {
    ProgressBatch,
    OpeningWip,
}

impl OrderRunInputSourceKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "progress_batch" => Some(Self::ProgressBatch),
            "opening_wip" => Some(Self::OpeningWip),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProgressBatch => "progress_batch",
            Self::OpeningWip => "opening_wip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunInputStatus {
    InUse,
    Processed,
}

impl OrderRunInputStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "in_use" => Some(Self::InUse),
            "processed" => Some(Self::Processed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InUse => "in_use",
            Self::Processed => "processed",
        }
    }
}

/// One upstream WIP consumed by a production run session.
///
/// `sequence_no` preserves splice order. Quantity contribution is
/// intentionally absent because the worker flow does not measure it at the
/// merge boundary; inventing an allocation would corrupt accounting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderRunInputLink {
    pub input_batch_id: String,
    pub input_qr_payload: String,
    pub source_apparatus: String,
    pub source_kind: OrderRunInputSourceKind,
    pub stage_node_id: String,
    pub sequence_no: u32,
    pub status: OrderRunInputStatus,
    pub linked_at_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at_unix: Option<i64>,
}

impl OrderRunInputLink {
    pub(crate) fn is_valid(&self) -> bool {
        !self.input_batch_id.trim().is_empty()
            && self.sequence_no > 0
            && match self.status {
                OrderRunInputStatus::InUse => self.processed_at_unix.is_none(),
                OrderRunInputStatus::Processed => self.processed_at_unix.is_some(),
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RezkaPartialRollStatus {
    Active,
}

impl RezkaPartialRollStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
        }
    }
}

/// The unfinished physical output roll currently mounted in one Rezka frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RezkaActivePartialRoll {
    pub slot_index: u32,
    pub generation: u32,
    pub contained_kadr_count: u32,
    pub status: RezkaPartialRollStatus,
    #[serde(default)]
    pub source_input_batch_ids: Vec<String>,
    pub started_at_unix: i64,
    pub updated_at_unix: i64,
}

impl RezkaActivePartialRoll {
    pub(crate) fn is_valid(&self) -> bool {
        if self.slot_index == 0 || self.generation == 0 || self.contained_kadr_count == 0 {
            return false;
        }
        let mut unique = std::collections::BTreeSet::new();
        self.source_input_batch_ids
            .iter()
            .all(|batch_id| !batch_id.trim().is_empty() && unique.insert(batch_id.trim()))
    }
}

/// Auditable many-to-one lineage for one completed output batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressBatchInputLink {
    pub input_batch_id: String,
    pub input_qr_payload: String,
    pub source_apparatus: String,
    pub source_kind: OrderRunInputSourceKind,
    pub sequence_no: u32,
}

impl ProgressBatchInputLink {
    pub(crate) fn is_valid(&self) -> bool {
        !self.input_batch_id.trim().is_empty() && self.sequence_no > 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WipProgressBatchQuery {
    pub apparatus: String,
    pub next_apparatus: String,
    pub current_location: String,
    pub status: Option<OrderProgressBatchWipStatus>,
    pub include_processed: bool,
    pub order_id: String,
    pub limit: usize,
}

impl WipProgressBatchQuery {
    pub fn new(
        apparatus: &str,
        next_apparatus: &str,
        current_location: &str,
        status: Option<OrderProgressBatchWipStatus>,
        include_processed: bool,
        order_id: &str,
        limit: usize,
    ) -> Self {
        Self {
            apparatus: apparatus.trim().to_string(),
            next_apparatus: next_apparatus.trim().to_string(),
            current_location: current_location.trim().to_string(),
            status,
            include_processed,
            order_id: order_id.trim().to_string(),
            limit,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderProgressBatchStatusDetail {
    pub work_status: String,
    pub wip_status: String,
    pub flow_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stock_status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionOrderStatusDetail {
    #[serde(default)]
    pub lifecycle_status: ProductionOrderLifecycleStatus,
    pub order_status: String,
    pub work_status: String,
    pub flow_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stock_status: String,
    #[serde(default)]
    pub total_wip_count: usize,
    #[serde(default)]
    pub waiting_wip_count: usize,
    #[serde(default)]
    pub in_use_wip_count: usize,
    #[serde(default)]
    pub processed_wip_count: usize,
    #[serde(default)]
    pub waiting_next_stage_count: usize,
    #[serde(default)]
    pub consumed_by_next_stage_count: usize,
    #[serde(default, alias = "finished_pending_acceptance_count")]
    pub free_wip_count: usize,
    #[serde(default)]
    pub accepted_wip_count: usize,
    #[serde(default)]
    pub active_session_count: usize,
    #[serde(default)]
    pub paused_session_count: usize,
    #[serde(default)]
    pub roll_detached_session_count: usize,
    #[serde(default)]
    pub completed_queue_count: usize,
    #[serde(default)]
    pub completed_with_issue_count: usize,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ink_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_print_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_film_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_bosma_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_lamination_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_edge_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub payload_json: serde_json::Value,
}
