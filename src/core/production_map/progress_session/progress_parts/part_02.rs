#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaminatsiyaAstatkaReport {
    pub report_id: String,
    pub order_id: String,
    pub apparatus: String,
    pub from_at_unix: i64,
    pub to_at_unix: i64,
    pub lamination_print_leftover_rolls: f64,
    pub lamination_film_leftover_rolls: f64,
    pub total_waste: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RezkaAstatkaReport {
    pub report_id: String,
    pub order_id: String,
    pub apparatus: String,
    pub from_at_unix: i64,
    pub to_at_unix: i64,
    pub total_waste: f64,
    pub rezka_bosma_waste: f64,
    pub rezka_lamination_waste: f64,
    pub rezka_edge_waste: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderProgressBatch {
    pub batch_id: String,
    #[serde(default = "default_progress_batch_revision")]
    pub revision: u64,
    pub session_id: String,
    pub started_at_unix: i64,
    pub completed_at_unix: i64,
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
    pub wip_status: OrderProgressBatchWipStatus,
    #[serde(default)]
    pub status_detail: OrderProgressBatchStatusDetail,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_apparatus_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub next_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent_batch_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub used_by_session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub used_by_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processed_by_session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processed_by_apparatus: String,
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

const fn default_progress_batch_revision() -> u64 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressBatchCorrectionInput {
    pub batch_id: String,
    pub expected_revision: u64,
    pub produced_qty: f64,
    pub uom: String,
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
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressBatchCorrectionRecord {
    pub batch_id: String,
    pub previous_revision: u64,
    pub new_revision: u64,
    pub reason: String,
    pub actor: QueueActionActor,
    pub old_values: serde_json::Value,
    pub new_values: serde_json::Value,
    pub created_at_unix: i64,
}

impl OrderProgressBatch {
    pub fn corrected(&self, input: &ProgressBatchCorrectionInput) -> Self {
        let mut corrected = self.clone();
        corrected.revision = self.revision.saturating_add(1);
        corrected.produced_qty = input.produced_qty;
        corrected.uom = input.uom.trim().to_string();
        corrected.return_ink_kg = input.return_ink_kg;
        corrected.lamination_print_leftover_rolls = input.lamination_print_leftover_rolls;
        corrected.lamination_film_leftover_rolls = input.lamination_film_leftover_rolls;
        corrected.rezka_bosma_waste = input.rezka_bosma_waste;
        corrected.rezka_lamination_waste = input.rezka_lamination_waste;
        corrected.rezka_edge_waste = input.rezka_edge_waste;
        corrected.total_waste = input.total_waste;
        corrected.finished_goods_kg = input.finished_goods_kg;
        corrected.bobina_kg = input.bobina_kg;
        corrected.finished_goods_meter = input.finished_goods_meter;
        corrected.diameter = input.diameter;
        corrected.description = input.description.trim().to_string();
        corrected.sync_correction_payload();
        corrected
    }

    pub(crate) fn correction_values(&self) -> serde_json::Value {
        serde_json::json!({
            "produced_qty": self.produced_qty,
            "uom": self.uom,
            "return_ink_kg": self.return_ink_kg,
            "lamination_print_leftover_rolls": self.lamination_print_leftover_rolls,
            "lamination_film_leftover_rolls": self.lamination_film_leftover_rolls,
            "rezka_bosma_waste": self.rezka_bosma_waste,
            "rezka_lamination_waste": self.rezka_lamination_waste,
            "rezka_edge_waste": self.rezka_edge_waste,
            "total_waste": self.total_waste,
            "finished_goods_kg": self.finished_goods_kg,
            "bobina_kg": self.bobina_kg,
            "finished_goods_meter": self.finished_goods_meter,
            "diameter": self.diameter,
            "description": self.description,
        })
    }

    fn sync_correction_payload(&mut self) {
        if !self.payload_json.is_object() {
            self.payload_json = serde_json::json!({});
        }
        let fields = self.correction_values();
        self.payload_json
            .as_object_mut()
            .expect("progress batch payload")
            .extend(fields.as_object().expect("correction values").clone());
        self.payload_json["correction_revision"] = serde_json::json!(self.revision);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionQrOpenedBy {
    pub actor_role: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub opened_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionQrReport {
    pub scanned_batch: OrderProgressBatch,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_batch: Option<OrderProgressBatch>,
    pub is_stale: bool,
    pub stale_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<ProductionMapDefinition>,
    pub order_status: ProductionOrderStatusDetail,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub logs: Vec<ProductionOrderLogEntry>,
    pub corrections: Vec<ProgressBatchCorrectionRecord>,
    pub progress_batches: Vec<OrderProgressBatch>,
    pub run_sessions: Vec<OrderRunSession>,
    pub active_sessions: Vec<OrderRunSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<ProductionQrOpenedBy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinishedGoodsStockEntry {
    pub id: String,
    pub warehouse: String,
    pub order_id: String,
    pub item_code: String,
    pub item_name: String,
    pub qty: f64,
    pub uom: String,
    pub status: String,
    pub barcode: String,
    pub source_progress_batch_id: String,
    pub accepted_by_role: String,
    pub accepted_by_ref: String,
    pub accepted_by_display_name: String,
    pub accepted_at_unix: i64,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinishedGoodsReceipt {
    pub batch: OrderProgressBatch,
    pub stock: FinishedGoodsStockEntry,
    pub order_status: ProductionOrderStatusDetail,
}
