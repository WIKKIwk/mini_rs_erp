use crate::core::gscale::models::ScaleDriverPrintResponse;

use super::RezkaOutputLabel;

pub(super) fn print_done(print: &ScaleDriverPrintResponse) -> bool {
    print.ok && print.status.trim().eq_ignore_ascii_case("done")
}

pub(super) fn print_error_detail(print: &ScaleDriverPrintResponse) -> String {
    for value in [&print.detail, &print.error, &print.status] {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "print failed".to_string()
}

pub(super) fn rezka_output_log(outputs: &[RezkaOutputLabel]) -> Vec<String> {
    outputs
        .iter()
        .map(|output| {
            format!(
                "item_code={} item_name={} qty={:.3} uom={} warehouse={} reason={} print_qr={} epc={}",
                output.item_code,
                output.item_name,
                output.qty,
                output.uom,
                output.warehouse,
                output.reason,
                output.print_qr,
                output.epc
            )
        })
        .collect()
}

pub(super) fn clean_store_error(message: &str) -> String {
    message
        .trim()
        .strip_prefix("store write failed: ")
        .unwrap_or_else(|| message.trim())
        .to_string()
}
