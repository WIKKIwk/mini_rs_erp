use serde::{Deserialize, Serialize};

use crate::core::gscale::models::MaterialReceiptPrintRequest;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RpsBatchStartRequest {
    #[serde(default)]
    pub client_batch_id: String,
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
    pub quantity_source: String,
    #[serde(default)]
    pub manual_qty_kg: f64,
    #[serde(default)]
    pub tare_enabled: bool,
    #[serde(default)]
    pub tare_kg: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RpsBatchSession {
    pub id: String,
    pub active: bool,
    pub owner_key: String,
    pub owner_role: String,
    pub owner_ref: String,
    pub driver_url: String,
    pub item_code: String,
    pub item_name: String,
    pub warehouse: String,
    pub printer: String,
    pub print_mode: String,
    pub quantity_source: String,
    pub manual_qty_kg: f64,
    pub tare_enabled: bool,
    pub tare_kg: f64,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub last_error_at: String,
    pub created_at: String,
    pub updated_at: String,
}

impl RpsBatchSession {
    pub fn inactive(owner_key: String, owner_role: String, owner_ref: String) -> Self {
        Self {
            owner_key,
            owner_role,
            owner_ref,
            print_mode: "label".to_string(),
            quantity_source: "scale".to_string(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RpsBatchResponse {
    pub ok: bool,
    pub batch: RpsBatchSession,
}

impl RpsBatchResponse {
    pub fn new(batch: RpsBatchSession) -> Self {
        Self { ok: true, batch }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RpsBatchPrintRequest {
    #[serde(default)]
    pub gross_qty: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub driver_url: String,
    #[serde(default)]
    pub print_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RpsBatchClientPrintConfirmRequest {
    #[serde(default)]
    pub epc: String,
    #[serde(flatten)]
    pub print: RpsBatchPrintRequest,
}

impl RpsBatchSession {
    pub fn material_receipt_request(
        &self,
        request: RpsBatchPrintRequest,
    ) -> MaterialReceiptPrintRequest {
        MaterialReceiptPrintRequest {
            driver_url: first_non_empty(&request.driver_url, &self.driver_url),
            item_code: self.item_code.clone(),
            item_name: self.item_name.clone(),
            warehouse: self.warehouse.clone(),
            printer: self.printer.clone(),
            print_mode: self.print_mode.clone(),
            gross_qty: request.gross_qty,
            unit: first_non_empty(&request.unit, "kg"),
            tare_enabled: self.tare_enabled,
            tare_kg: self.tare_kg,
            print_count: request.print_count,
            actor_role: String::new(),
            actor_ref: String::new(),
            actor_display_name: String::new(),
        }
    }
}

fn first_non_empty(value: &str, default: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        default.trim().to_string()
    } else {
        value.to_string()
    }
}
