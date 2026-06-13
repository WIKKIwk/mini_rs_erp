use crate::core::werka::models::WerkaHomeSummary;
use crate::erpdb::werka_home::{classify_werka_receipt_fields, delivery_status_from_state};

const DELIVERY_FLOW_STATE_SUBMITTED: i32 = 1;

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub(crate) struct PurchaseReceiptStatusRow {
    pub doc_status: i32,
    pub status: String,
    pub total_qty: f64,
    pub supplier_delivery_note: String,
    pub remarks: String,
}

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub(crate) struct DeliveryNoteStatusRow {
    pub doc_status: i32,
    pub accord_flow_state: i32,
    pub accord_customer_state: i32,
}

pub(crate) fn build_werka_summary(
    receipts: &[PurchaseReceiptStatusRow],
    delivery_notes: &[DeliveryNoteStatusRow],
) -> WerkaHomeSummary {
    let mut summary = WerkaHomeSummary::default();

    for row in receipts {
        let (status, include) = classify_werka_receipt_fields(
            row.doc_status,
            &row.status,
            row.total_qty,
            &row.supplier_delivery_note,
            &row.remarks,
        );
        if include {
            count_status(&mut summary, &status);
        }
    }

    for row in delivery_notes {
        if row.doc_status != 1 || row.accord_flow_state != DELIVERY_FLOW_STATE_SUBMITTED {
            continue;
        }
        let status = delivery_status_from_state(
            row.doc_status,
            row.accord_flow_state,
            row.accord_customer_state,
        );
        count_status(&mut summary, status);
    }

    summary
}

fn count_status(summary: &mut WerkaHomeSummary, status: &str) {
    match status {
        "pending" | "draft" => summary.pending_count += 1,
        "accepted" => summary.confirmed_count += 1,
        "partial" | "rejected" | "cancelled" => summary.returned_count += 1,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(
        doc_status: i32,
        status: &str,
        total_qty: f64,
        marker: &str,
    ) -> PurchaseReceiptStatusRow {
        PurchaseReceiptStatusRow {
            doc_status,
            status: status.to_string(),
            total_qty,
            supplier_delivery_note: marker.to_string(),
            remarks: String::new(),
        }
    }

    fn delivery(customer_state: i32) -> DeliveryNoteStatusRow {
        DeliveryNoteStatusRow {
            doc_status: 1,
            accord_flow_state: DELIVERY_FLOW_STATE_SUBMITTED,
            accord_customer_state: customer_state,
        }
    }

    #[test]
    fn summary_counts_status_rows_like_go() {
        let receipts = vec![
            receipt(0, "", 4.0, "TG:+998:1:4.0000"),
            receipt(1, "", 4.0, "TG:+998:1:4.0000"),
            receipt(1, "", 4.0, "TG:+998:1:8.0000"),
            receipt(2, "Cancelled", 4.0, "TG:+998:1:4.0000"),
        ];
        let deliveries = vec![delivery(0), delivery(3), delivery(4)];

        let summary = build_werka_summary(&receipts, &deliveries);

        assert_eq!(summary.pending_count, 2);
        assert_eq!(summary.confirmed_count, 2);
        assert_eq!(summary.returned_count, 3);
    }

    #[test]
    fn summary_ignores_hidden_unannounced_receipts_like_go() {
        let mut hidden_pending = receipt(0, "", 4.0, "TG:+998:1:4.0000");
        hidden_pending.remarks = "Accord Werka Aytilmagan: pending".to_string();
        let mut hidden_approved = receipt(1, "", 4.0, "TG:+998:1:4.0000");
        hidden_approved.remarks = "Accord Werka Aytilmagan: approved".to_string();

        let summary = build_werka_summary(&[hidden_pending, hidden_approved], &[]);

        assert_eq!(summary, WerkaHomeSummary::default());
    }
}
