use crate::core::werka::models::DispatchRecord;
use crate::core::werka::ports::WerkaPortError;
use crate::core::werka::service::{WerkaService, current_timestamp_label};

impl WerkaService {
    pub async fn confirm_receipt(
        &self,
        receipt_id: &str,
        accepted_qty: f64,
        returned_qty: f64,
        return_reason: &str,
        return_comment: &str,
    ) -> Result<Option<DispatchRecord>, WerkaPortError> {
        let Some(writer) = &self.confirm_writer else {
            return Ok(None);
        };
        let result = writer
            .confirm_and_submit_purchase_receipt(
                receipt_id.trim(),
                accepted_qty,
                returned_qty,
                return_reason,
                return_comment,
            )
            .await?;

        Ok(Some(DispatchRecord {
            id: result.name,
            supplier_name: result.supplier,
            item_code: result.item_code.clone(),
            item_name: result.item_code,
            uom: result.uom,
            sent_qty: result.sent_qty,
            accepted_qty: result.accepted_qty,
            note: result.note,
            status: dispatch_status_from_quantities(result.sent_qty, result.accepted_qty)
                .to_string(),
            created_label: current_timestamp_label(),
            ..DispatchRecord::default()
        }))
    }
}

fn dispatch_status_from_quantities(sent_qty: f64, accepted_qty: f64) -> &'static str {
    if accepted_qty <= 0.0 {
        "rejected"
    } else if sent_qty > 0.0 && accepted_qty < sent_qty {
        "partial"
    } else {
        "accepted"
    }
}
