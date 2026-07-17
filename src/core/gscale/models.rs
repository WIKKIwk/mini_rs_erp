use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct MaterialReceiptPrintRequest {
    #[serde(default)]
    pub driver_url: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub warehouse: String,
    #[serde(default)]
    pub printer: String,
    #[serde(default)]
    pub print_mode: String,
    #[serde(default)]
    pub gross_qty: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub tare_enabled: bool,
    #[serde(default)]
    pub tare_kg: f64,
    #[serde(default)]
    pub print_count: u32,
    #[serde(default)]
    pub actor_role: String,
    #[serde(default)]
    pub actor_ref: String,
    #[serde(default)]
    pub actor_display_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MaterialReceiptPrintResponse {
    pub ok: bool,
    pub status: String,
    pub draft_name: String,
    pub epc: String,
    pub item_code: String,
    pub item_name: String,
    pub warehouse: String,
    pub qty: f64,
    pub net_qty: f64,
    pub gross_qty: f64,
    pub unit: String,
    pub printer: String,
    pub print_mode: String,
    pub printer_status: String,
    pub print_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProgressLabelPrintRequest {
    #[serde(default)]
    pub driver_url: String,
    #[serde(default)]
    pub qr_payload: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub executor_name: String,
    #[serde(default)]
    pub printer: String,
    #[serde(default)]
    pub print_mode: String,
    #[serde(default)]
    pub gross_qty: f64,
    #[serde(default)]
    pub progress_qty: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub progress_unit: String,
    #[serde(default)]
    pub label_kind: String,
    #[serde(default)]
    pub print_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProgressLabelPrintResponse {
    pub ok: bool,
    pub status: String,
    pub qr_payload: String,
    pub item_code: String,
    pub item_name: String,
    pub executor_name: String,
    pub qty: f64,
    pub gross_qty: f64,
    pub unit: String,
    pub progress_unit: String,
    pub label_kind: String,
    pub printer: String,
    pub print_mode: String,
    pub printer_status: String,
    pub print_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaterialReceiptDraft {
    pub name: String,
    pub item_code: String,
    pub warehouse: String,
    pub qty: f64,
    pub uom: String,
    pub barcode: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RawMaterialStockEntry {
    pub id: String,
    pub warehouse: String,
    pub item_code: String,
    pub item_name: String,
    pub barcode: String,
    pub qty: f64,
    pub uom: String,
    pub status: String,
    pub reserved_order_id: String,
    pub source_receipt_id: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreateMaterialReceiptDraftInput {
    pub item_code: String,
    pub item_name: String,
    pub warehouse: String,
    pub qty: f64,
    pub barcode: String,
    pub actor_role: String,
    pub actor_ref: String,
    pub actor_display_name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScaleDriverPrintRequest {
    pub driver_url: String,
    pub epc: String,
    pub item_code: String,
    pub item_name: String,
    pub warehouse: String,
    pub executor_name: String,
    pub label_kind: String,
    pub printer: String,
    pub print_mode: String,
    pub gross_qty: f64,
    pub qty: Option<f64>,
    pub unit: String,
    pub progress_unit: String,
    pub tare_enabled: bool,
    pub tare_kg: f64,
    pub print_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ScaleDriverPrintResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub epc: String,
    #[serde(default)]
    pub printer: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub qty: f64,
    #[serde(default)]
    pub net_qty: f64,
    #[serde(default)]
    pub gross_qty: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub printer_status: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub detail: String,
}
