use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    #[serde(default)]
    pub batch_code: String,
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
    #[serde(default)]
    pub prints: Vec<RpsBatchPrintEntry>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RpsBatchPrintEntry {
    pub epc: String,
    pub draft_name: String,
    pub status: String,
    pub qty: f64,
    pub net_qty: f64,
    pub gross_qty: f64,
    pub unit: String,
    pub printer: String,
    pub print_mode: String,
    pub print_count: u32,
    pub printed_at: String,
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

    pub fn ensure_batch_code(&mut self) {
        if self.batch_code.trim().is_empty() && !self.id.trim().is_empty() {
            self.batch_code = legacy_batch_code(&self.owner_key, &self.id);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RpsBatchResponse {
    pub ok: bool,
    pub batch: RpsBatchSession,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RpsBatchHistoryResponse {
    pub ok: bool,
    pub batches: Vec<RpsBatchSession>,
}

impl RpsBatchHistoryResponse {
    pub fn new(mut batches: Vec<RpsBatchSession>) -> Self {
        for batch in &mut batches {
            batch.ensure_batch_code();
        }
        Self { ok: true, batches }
    }
}

impl RpsBatchResponse {
    pub fn new(mut batch: RpsBatchSession) -> Self {
        batch.ensure_batch_code();
        Self { ok: true, batch }
    }
}

pub fn new_batch_code() -> String {
    let random: [u8; 11] = rand::random();
    format!("42{}", data_encoding::HEXUPPER.encode(&random))
}

pub fn legacy_batch_code(owner_key: &str, batch_id: &str) -> String {
    let digest = Sha256::digest(format!("{}\u{1f}{}", owner_key.trim(), batch_id.trim()));
    format!("42{}", data_encoding::HEXUPPER.encode(&digest[..11]))
}

pub fn is_valid_batch_code(value: &str) -> bool {
    let value = value.trim();
    value.len() == 24
        && value.starts_with("42")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
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
            label_kind: "material_product".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_batch_print_uses_large_qr_product_label() {
        let batch = RpsBatchSession {
            driver_url: "http://127.0.0.1:39117".to_string(),
            item_code: "CPP 1030/25".to_string(),
            item_name: "CPP 1030/25".to_string(),
            warehouse: "Kalidor".to_string(),
            printer: "zebra".to_string(),
            print_mode: "rfid".to_string(),
            ..RpsBatchSession::default()
        };

        let request = batch.material_receipt_request(RpsBatchPrintRequest {
            gross_qty: 23.0,
            unit: "kg".to_string(),
            driver_url: String::new(),
            print_count: 1,
        });

        assert_eq!(request.label_kind, "material_product");
        assert_eq!(request.item_code, "CPP 1030/25");
        assert_eq!(request.print_mode, "rfid");
    }

    #[test]
    fn batch_codes_have_a_separate_stable_24_hex_namespace() {
        let generated = new_batch_code();
        assert!(is_valid_batch_code(&generated));

        let legacy = legacy_batch_code("material_taminotchi:M-1", "batch-1");
        assert_eq!(legacy, legacy_batch_code("material_taminotchi:M-1", "batch-1"));
        assert!(is_valid_batch_code(&legacy));
        assert_ne!(legacy, legacy_batch_code("material_taminotchi:M-1", "batch-2"));
    }
}
