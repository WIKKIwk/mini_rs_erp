use crate::core::werka::models::DispatchRecord;
use crate::erpdb::werka_home::{
    DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow, classify_werka_receipt,
    delivery_note_to_record, delivery_status, delivery_visible, purchase_receipt_to_record,
};

pub(crate) fn build_werka_pending(
    receipts: &[PurchaseReceiptSummaryRow],
    delivery_notes: &[DeliveryNoteSummaryRow],
    limit: usize,
) -> Vec<DispatchRecord> {
    let mut result = Vec::with_capacity(64);

    for row in receipts {
        let (status, include) = classify_werka_receipt(row);
        if include && (status == "pending" || status == "draft") {
            result.push(purchase_receipt_to_record(row));
        }
    }

    for row in delivery_notes {
        if delivery_visible(row) && delivery_status(row) == "pending" {
            result.push(delivery_note_to_record(row));
        }
    }

    result.sort_by(|left, right| right.created_label.cmp(&left.created_label));
    if limit > 0 && result.len() > limit {
        result.truncate(limit);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(name: &str, doc_status: i32, status: &str, date: &str) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            doc_status,
            status: status.to_string(),
            total_qty: 4.0,
            posting_date: date.to_string(),
            supplier_delivery_note: "TG:+998:20260116080000:4.0000".to_string(),
            remarks: String::new(),
            currency: "UZS".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            amount: 12.0,
        }
    }

    fn delivery(
        name: &str,
        customer_state: i32,
        modified: &str,
        flow_state: i32,
    ) -> DeliveryNoteSummaryRow {
        DeliveryNoteSummaryRow {
            name: name.to_string(),
            customer: "CUST-001".to_string(),
            customer_name: "Customer".to_string(),
            doc_status: 1,
            modified: modified.to_string(),
            qty: 8.0,
            returned_qty: 0.0,
            customer_reason: String::new(),
            item_code: "ITEM-002".to_string(),
            item_name: "Item 2".to_string(),
            uom: "Pcs".to_string(),
            accord_flow_state: flow_state,
            accord_customer_state: customer_state,
        }
    }

    #[test]
    fn pending_filters_and_sorts_like_go() {
        let receipts = vec![
            receipt("PR-PENDING", 0, "", "2026-01-15"),
            receipt("PR-DRAFT", 0, "Draft", "2026-01-17"),
            receipt("PR-ACCEPTED", 1, "", "2026-01-18"),
        ];
        let deliveries = vec![
            delivery("DN-PENDING", 0, "2026-01-16 09:00:00", 1),
            delivery("DN-HIDDEN", 0, "2026-01-19 09:00:00", 0),
            delivery("DN-ACCEPTED", 3, "2026-01-20 09:00:00", 1),
        ];

        let items = build_werka_pending(&receipts, &deliveries, 0);

        let ids: Vec<_> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, vec!["PR-DRAFT", "DN-PENDING", "PR-PENDING"]);
    }

    #[test]
    fn pending_applies_final_limit_like_go() {
        let receipts = vec![
            receipt("PR-OLD", 0, "", "2026-01-15"),
            receipt("PR-NEW", 0, "", "2026-01-17"),
        ];

        let items = build_werka_pending(&receipts, &[], 1);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "PR-NEW");
    }
}
