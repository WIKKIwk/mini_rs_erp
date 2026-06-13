use crate::core::werka::models::NotificationDetail;
use crate::core::werka::ports::WerkaPortError;
use crate::core::werka::service::WerkaService;
use crate::core::werka::unannounced::{
    extract_werka_unannounced_state, format_notification_comment,
    purchase_receipt_notification_detail, upsert_werka_unannounced_in_remarks,
};

impl WerkaService {
    pub async fn respond_supplier_unannounced(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
        receipt_id: &str,
        approve: bool,
        reason: &str,
    ) -> Result<Option<NotificationDetail>, WerkaPortError> {
        let Some(writer) = &self.supplier_unannounced_writer else {
            return Ok(None);
        };

        let draft = writer.get_purchase_receipt(receipt_id.trim()).await?;
        if draft.supplier.trim() != supplier_ref.trim()
            || extract_werka_unannounced_state(&draft.remarks) != "pending"
        {
            return Err(WerkaPortError::WriteFailed(
                "supplier unannounced response failed".to_string(),
            ));
        }
        if approve {
            let remarks = upsert_werka_unannounced_in_remarks(&draft.remarks, "approved", "");
            writer
                .update_purchase_receipt_remarks(&draft.name, &remarks)
                .await?;
            let result = writer
                .confirm_and_submit_purchase_receipt(&draft.name, draft.qty, 0.0, "", "")
                .await?;
            let _ = writer
                .add_purchase_receipt_comment(
                    &draft.name,
                    &format_notification_comment(
                        "Supplier",
                        supplier_display_name,
                        "Aytilmagan mol tasdiqlandi.",
                    ),
                )
                .await;
            let mut detail = purchase_receipt_notification_detail(
                writer.as_ref(),
                supplier_ref,
                supplier_display_name,
                receipt_id,
            )
            .await?;
            detail.record.accepted_qty = result.accepted_qty;
            detail.record.status = "accepted".to_string();
            detail.record.event_type.clear();
            detail.record.highlight.clear();
            detail.record.note.clear();
            return Ok(Some(detail));
        }

        let remarks = upsert_werka_unannounced_in_remarks(&draft.remarks, "rejected", reason);
        writer
            .update_purchase_receipt_remarks(&draft.name, &remarks)
            .await?;
        let _ = writer
            .add_purchase_receipt_comment(
                &draft.name,
                &format_notification_comment(
                    "Supplier",
                    supplier_display_name,
                    &rejected_message(reason),
                ),
            )
            .await;
        purchase_receipt_notification_detail(
            writer.as_ref(),
            supplier_ref,
            supplier_display_name,
            receipt_id,
        )
        .await
        .map(Some)
    }
}

fn rejected_message(reason: &str) -> String {
    if reason.trim().is_empty() {
        "Aytilmagan mol rad etildi.".to_string()
    } else {
        format!("Aytilmagan mol rad etildi. Sabab: {}", reason.trim())
    }
}
