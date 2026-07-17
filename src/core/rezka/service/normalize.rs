use std::collections::HashSet;

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct NormalizedRezkaSplit {
    pub(super) source: RezkaSourceEntry,
    pub(super) reason: String,
    driver_url: String,
    printer: String,
    print_mode: String,
    pub(super) outputs: Vec<RezkaOutputLabel>,
    pub(super) printable_outputs: Vec<RezkaOutputLabel>,
    pub(super) all_printable_epcs_supplied: bool,
}

impl NormalizedRezkaSplit {
    pub(super) fn from_request(
        source: RezkaSourceEntry,
        request: RezkaSplitRequest,
        epc: Option<&dyn EpcSource>,
    ) -> Result<Self, RezkaServiceError> {
        if source.barcode.trim().is_empty()
            || source.item_code.trim().is_empty()
            || source.warehouse.trim().is_empty()
            || source.qty <= 0.0
        {
            return Err(RezkaServiceError::InvalidInput(
                "source_is_invalid".to_string(),
            ));
        }
        if !request
            .source_barcode
            .trim()
            .eq_ignore_ascii_case(&source.barcode)
        {
            return Err(RezkaServiceError::InvalidInput(
                "source_barcode_mismatch".to_string(),
            ));
        }
        if !request.source_stock_entry.trim().is_empty()
            && request.source_stock_entry.trim() != source.stock_entry_name
        {
            return Err(RezkaServiceError::InvalidInput(
                "source_stock_entry_mismatch".to_string(),
            ));
        }
        if request.source_line_index > 0 && request.source_line_index != source.line_index {
            return Err(RezkaServiceError::InvalidInput(
                "source_line_index_mismatch".to_string(),
            ));
        }
        if request.outputs.len() < 2 {
            return Err(RezkaServiceError::InvalidInput(
                "at_least_two_outputs_required".to_string(),
            ));
        }

        let mut total = 0.0;
        let mut outputs = Vec::with_capacity(request.outputs.len());
        let mut printable_outputs = Vec::new();
        let mut all_printable_epcs_supplied = true;
        let mut client_epcs = HashSet::new();
        for output in request.outputs {
            let target_warehouse = output.target_warehouse.trim().to_string();
            let print_qr =
                output.print_qr && !is_rezka_scrap_output(&target_warehouse, &output.reason);
            let item_code = if print_qr {
                output.item_code.trim().to_string()
            } else {
                blank_default(&output.item_code, &source.item_code)
            };
            if item_code.is_empty()
                || target_warehouse.is_empty()
                || output.qty <= 0.0
                || (print_qr && output.item_code.trim().is_empty())
            {
                return Err(RezkaServiceError::InvalidInput(
                    "output_item_warehouse_qty_required".to_string(),
                ));
            }
            let next_epc = if print_qr {
                let requested_epc = output.epc.trim().to_ascii_uppercase();
                if requested_epc.is_empty() {
                    all_printable_epcs_supplied = false;
                    let epc = epc.ok_or(RezkaServiceError::EpcGenerationFailed)?;
                    let next_epc = epc.next_epc().trim().to_ascii_uppercase();
                    if next_epc.is_empty() {
                        return Err(RezkaServiceError::EpcGenerationFailed);
                    }
                    next_epc
                } else {
                    validate_client_epc(&requested_epc)?;
                    if !client_epcs.insert(requested_epc.clone()) {
                        return Err(RezkaServiceError::InvalidInput(
                            "client_print_epc_duplicate".to_string(),
                        ));
                    }
                    requested_epc
                }
            } else {
                String::new()
            };
            let output_label = RezkaOutputLabel {
                epc: next_epc,
                item_name: if print_qr {
                    blank_default(&output.item_name, &item_code)
                } else {
                    blank_default(&output.item_name, &source.item_name)
                },
                item_code,
                qty: output.qty,
                uom: blank_default(&output.uom, &source.uom),
                warehouse: target_warehouse,
                reason: output.reason.trim().to_string(),
                print_qr,
            };
            if output_label.print_qr {
                printable_outputs.push(output_label.clone());
            }
            total += output.qty;
            outputs.push(output_label);
        }
        if (total - source.qty).abs() > QTY_TOLERANCE {
            return Err(RezkaServiceError::InvalidInput(format!(
                "output_total_must_equal_source_qty:{total:.3}!={:.3}",
                source.qty
            )));
        }

        Ok(Self {
            source,
            reason: request.reason.trim().to_string(),
            driver_url: request.driver_url.trim().trim_end_matches('/').to_string(),
            printer: blank_default(&request.printer.to_ascii_lowercase(), "zebra"),
            print_mode: blank_default(&request.print_mode.to_ascii_lowercase(), "rfid"),
            outputs,
            printable_outputs,
            all_printable_epcs_supplied,
        })
    }

    pub(super) fn driver_request(&self, output: &RezkaOutputLabel) -> ScaleDriverPrintRequest {
        ScaleDriverPrintRequest {
            driver_url: self.driver_url.clone(),
            epc: output.epc.clone(),
            item_code: output.item_code.clone(),
            item_name: output.item_name.clone(),
            warehouse: output.warehouse.clone(),
            executor_name: String::new(),
            label_kind: String::new(),
            printer: self.printer.clone(),
            print_mode: self.print_mode.clone(),
            gross_qty: output.qty,
            qty: None,
            unit: output.uom.clone(),
            progress_unit: String::new(),
            tare_enabled: false,
            tare_kg: 0.0,
            print_count: 1,
        }
    }
}

fn validate_client_epc(value: &str) -> Result<(), RezkaServiceError> {
    if value.len() != 24
        || !value.starts_with("30")
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RezkaServiceError::InvalidInput(
            "client_print_epc_invalid".to_string(),
        ));
    }
    Ok(())
}

fn is_rezka_scrap_output(warehouse: &str, reason: &str) -> bool {
    let warehouse = warehouse.trim().to_ascii_lowercase();
    let reason = reason.trim().to_ascii_lowercase();
    warehouse == "brak - ombori - a" || reason.contains("brak") || reason.contains("atxot")
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}
