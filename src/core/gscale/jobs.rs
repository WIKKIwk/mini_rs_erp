use super::{GscaleServiceError, MIN_BATCH_QTY_KG};
use crate::core::gscale::models::{
    MaterialReceiptPrintRequest, ProgressLabelPrintRequest, ScaleDriverPrintRequest,
};
use crate::core::quantity::positive_erp_quantity;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct NormalizedProgressLabelJob {
    pub(super) driver_url: String,
    pub(super) qr_payload: String,
    pub(super) item_code: String,
    pub(super) item_name: String,
    pub(super) executor_name: String,
    pub(super) printer: String,
    pub(super) print_mode: String,
    pub(super) gross_qty: f64,
    pub(super) progress_qty: f64,
    pub(super) unit: String,
    pub(super) progress_unit: String,
    pub(super) label_kind: String,
    pub(super) print_count: u32,
}

impl NormalizedProgressLabelJob {
    pub(super) fn from_request(
        request: ProgressLabelPrintRequest,
    ) -> Result<Self, GscaleServiceError> {
        let qr_payload = request.qr_payload.trim().to_string();
        let item_code = request.item_code.trim().to_string();
        let item_name = request.item_name.trim().to_string();
        if qr_payload.is_empty() || item_code.is_empty() || item_name.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "qr_payload_item_code_and_item_name_required".to_string(),
            ));
        }
        let gross_qty = positive_erp_quantity(request.gross_qty).ok_or_else(|| {
            GscaleServiceError::InvalidInput("progress_gross_qty_required".to_string())
        })?;
        let progress_qty = if request.progress_qty > 0.0 {
            positive_erp_quantity(request.progress_qty)
        } else {
            Some(gross_qty)
        }
        .ok_or_else(|| GscaleServiceError::InvalidInput("progress_qty_required".to_string()))?;
        Ok(Self {
            driver_url: request.driver_url.trim().to_string(),
            qr_payload,
            item_code,
            item_name,
            executor_name: request.executor_name.trim().to_string(),
            printer: request.printer.trim().to_ascii_lowercase(),
            print_mode: request.print_mode.trim().to_ascii_lowercase(),
            gross_qty,
            progress_qty,
            unit: blank_default(&request.unit, "kg"),
            progress_unit: blank_default(&request.progress_unit, "m"),
            label_kind: normalize_label_kind(&request.label_kind),
            print_count: normalize_print_count(request.print_count),
        })
    }

    pub(super) fn driver_request(&self) -> ScaleDriverPrintRequest {
        ScaleDriverPrintRequest {
            driver_url: self.driver_url.clone(),
            epc: self.qr_payload.clone(),
            item_code: self.item_code.clone(),
            item_name: self.item_name.clone(),
            warehouse: format!("Ijrochi: {}", self.executor_name.trim()),
            executor_name: self.executor_name.clone(),
            label_kind: self.label_kind.clone(),
            printer: self.printer.clone(),
            print_mode: self.print_mode.clone(),
            gross_qty: self.gross_qty,
            qty: Some(self.progress_qty),
            unit: self.unit.clone(),
            progress_unit: self.progress_unit.clone(),
            tare_enabled: false,
            tare_kg: 0.0,
            print_count: self.print_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct NormalizedMaterialReceiptJob {
    pub(super) driver_url: String,
    pub(super) item_code: String,
    pub(super) item_name: String,
    pub(super) warehouse: String,
    pub(super) printer: String,
    pub(super) print_mode: String,
    pub(super) gross_qty: f64,
    pub(super) net_qty: f64,
    pub(super) unit: String,
    pub(super) tare_enabled: bool,
    pub(super) tare_kg: f64,
    pub(super) print_count: u32,
    pub(super) actor_role: String,
    pub(super) actor_ref: String,
    pub(super) actor_display_name: String,
}

impl NormalizedMaterialReceiptJob {
    pub(super) fn from_request(
        request: MaterialReceiptPrintRequest,
    ) -> Result<Self, GscaleServiceError> {
        let item_code = request.item_code.trim().to_string();
        let warehouse = request.warehouse.trim().to_string();
        if item_code.is_empty() || warehouse.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "item_code_and_warehouse_required".to_string(),
            ));
        }
        let gross_qty = positive_erp_quantity(request.gross_qty).ok_or_else(|| {
            GscaleServiceError::InvalidInput(format!(
                "QTY juda kichik: {:.3} kg | min {MIN_BATCH_QTY_KG:.3} kg",
                request.gross_qty
            ))
        })?;
        if gross_qty < MIN_BATCH_QTY_KG {
            return Err(GscaleServiceError::InvalidInput(format!(
                "QTY juda kichik: {gross_qty:.3} kg | min {MIN_BATCH_QTY_KG:.3} kg"
            )));
        }
        let tare_enabled = request.tare_enabled || request.tare_kg > 0.0;
        let tare_kg = if tare_enabled && request.tare_kg > 0.0 {
            positive_erp_quantity(request.tare_kg)
                .ok_or_else(|| GscaleServiceError::InvalidInput("tare_kg_invalid".to_string()))?
        } else {
            0.0
        };
        let net_qty = if tare_kg > 0.0 {
            positive_erp_quantity(gross_qty - tare_kg).unwrap_or(0.0)
        } else {
            gross_qty
        };
        if net_qty < MIN_BATCH_QTY_KG {
            return Err(GscaleServiceError::InvalidInput(format!(
                "NETTO juda kichik: brutto {gross_qty:.3} kg - babina {tare_kg:.3} kg = {net_qty:.3} kg | min {MIN_BATCH_QTY_KG:.3} kg"
            )));
        }
        let item_name = blank_default(&request.item_name, &item_code);
        Ok(Self {
            driver_url: request.driver_url.trim().to_string(),
            item_code,
            item_name,
            warehouse,
            printer: request.printer.trim().to_ascii_lowercase(),
            print_mode: request.print_mode.trim().to_ascii_lowercase(),
            gross_qty,
            net_qty,
            unit: blank_default(&request.unit, "kg"),
            tare_enabled: tare_kg > 0.0,
            tare_kg,
            print_count: normalize_print_count(request.print_count),
            actor_role: request.actor_role.trim().to_string(),
            actor_ref: request.actor_ref.trim().to_string(),
            actor_display_name: request.actor_display_name.trim().to_string(),
        })
    }

    pub(super) fn driver_request(&self, epc: &str) -> ScaleDriverPrintRequest {
        ScaleDriverPrintRequest {
            driver_url: self.driver_url.clone(),
            epc: epc.trim().to_ascii_uppercase(),
            item_code: self.item_code.clone(),
            item_name: self.item_name.clone(),
            warehouse: self.warehouse.clone(),
            executor_name: String::new(),
            label_kind: String::new(),
            printer: self.printer.clone(),
            print_mode: self.print_mode.clone(),
            gross_qty: self.gross_qty,
            qty: None,
            unit: self.unit.clone(),
            progress_unit: String::new(),
            tare_enabled: self.tare_enabled,
            tare_kg: self.tare_kg,
            print_count: self.print_count,
        }
    }
}

fn normalize_print_count(value: u32) -> u32 {
    if value == 0 { 1 } else { value }
}

fn normalize_label_kind(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        "progress".to_string()
    } else {
        value
    }
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}
