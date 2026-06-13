use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RezkaSourceEntry {
    pub barcode: String,
    pub stock_entry_name: String,
    pub line_index: i32,
    pub item_code: String,
    pub item_name: String,
    pub qty: f64,
    pub uom: String,
    pub warehouse: String,
    pub company: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RezkaSplitRequest {
    #[serde(default)]
    pub source_barcode: String,
    #[serde(default)]
    pub source_stock_entry: String,
    #[serde(default)]
    pub source_line_index: i32,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub driver_url: String,
    #[serde(default)]
    pub printer: String,
    #[serde(default)]
    pub print_mode: String,
    #[serde(default)]
    pub outputs: Vec<RezkaSplitOutputRequest>,
}

fn default_print_qr() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RezkaSplitOutputRequest {
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub item_name: String,
    #[serde(default)]
    pub qty: f64,
    #[serde(default)]
    pub uom: String,
    #[serde(default)]
    pub target_warehouse: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default = "default_print_qr")]
    pub print_qr: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RezkaOutputLabel {
    pub epc: String,
    pub item_code: String,
    pub item_name: String,
    pub qty: f64,
    pub uom: String,
    pub warehouse: String,
    pub reason: String,
    pub print_qr: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RezkaSplitResponse {
    pub ok: bool,
    pub status: String,
    pub stock_entry_name: String,
    pub source_barcode: String,
    pub outputs: Vec<RezkaOutputLabel>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateRezkaRepackDraftInput {
    pub source: RezkaSourceEntry,
    pub reason: String,
    pub outputs: Vec<RezkaOutputLabel>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RezkaRepackDraft {
    pub name: String,
}
