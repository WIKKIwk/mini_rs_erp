use crate::core::werka::models::DispatchRecord;
use crate::erpdb::werka_home::{
    DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow, delivery_note_to_record,
    purchase_receipt_to_record,
};
use crate::erpdb::werka_status_breakdown::matches_werka_breakdown;

pub(crate) fn build_werka_status_details(
    receipts: &[PurchaseReceiptSummaryRow],
    delivery_notes: &[DeliveryNoteSummaryRow],
    kind: &str,
    supplier_ref: &str,
) -> Vec<DispatchRecord> {
    let needle = supplier_ref.trim();
    let mut result = Vec::with_capacity(receipts.len() + delivery_notes.len());

    for record in receipts.iter().map(purchase_receipt_to_record) {
        add_if_matches(&mut result, record, kind, needle);
    }

    for record in delivery_notes.iter().map(delivery_note_to_record) {
        add_if_matches(&mut result, record, kind, needle);
    }

    result.sort_by(|left, right| right.created_label.cmp(&left.created_label));
    result
}

fn add_if_matches(
    result: &mut Vec<DispatchRecord>,
    record: DispatchRecord,
    kind: &str,
    supplier_ref: &str,
) {
    if !supplier_ref.is_empty()
        && !record
            .supplier_ref
            .trim()
            .eq_ignore_ascii_case(supplier_ref)
    {
        return;
    }
    if !matches_werka_breakdown(&record, kind) {
        return;
    }

    result.push(record);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(
        supplier: &str,
        name: &str,
        doc_status: i32,
        total_qty: f64,
        marker: &str,
        posting_date: &str,
    ) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: supplier.to_string(),
            supplier_name: format!("Supplier {supplier}"),
            doc_status,
            status: String::new(),
            total_qty,
            posting_date: posting_date.to_string(),
            supplier_delivery_note: marker.to_string(),
            remarks: String::new(),
            currency: "UZS".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            amount: 12.0,
        }
    }

    fn delivery(customer: &str, customer_state: i32, modified: &str) -> DeliveryNoteSummaryRow {
        DeliveryNoteSummaryRow {
            name: format!("DN-{customer}"),
            customer: customer.to_string(),
            customer_name: format!("Customer {customer}"),
            doc_status: 1,
            modified: modified.to_string(),
            qty: 5.0,
            returned_qty: 1.0,
            customer_reason: String::new(),
            item_code: "ITEM-002".to_string(),
            item_name: "Item 2".to_string(),
            uom: "Pcs".to_string(),
            accord_flow_state: 1,
            accord_customer_state: customer_state,
        }
    }

    #[test]
    fn details_filters_by_supplier_and_kind_like_go() {
        let receipts = vec![
            receipt("SUP-001", "PR-1", 0, 4.0, "TG:+998:1:4.0000", "2026-01-15"),
            receipt("SUP-002", "PR-2", 0, 3.0, "TG:+998:1:3.0000", "2026-01-16"),
            receipt("SUP-001", "PR-3", 1, 2.0, "TG:+998:1:2.0000", "2026-01-17"),
        ];

        let items = build_werka_status_details(&receipts, &[], "pending", "sup-001");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "PR-1");
    }

    #[test]
    fn details_includes_delivery_notes_and_sorts_newest_first() {
        let receipts = vec![receipt(
            "SUP-001",
            "PR-OLD",
            1,
            4.0,
            "TG:+998:1:6.0000",
            "2026-01-15",
        )];
        let delivery_notes = vec![delivery("SUP-001", 4, "2026-01-18 10:00:00")];

        let items = build_werka_status_details(&receipts, &delivery_notes, "returned", "");

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "DN-SUP-001");
        assert_eq!(items[1].id, "PR-OLD");
    }

    #[test]
    fn details_returns_empty_for_unknown_kind() {
        let receipts = vec![receipt(
            "SUP-001",
            "PR-1",
            0,
            4.0,
            "TG:+998:1:4.0000",
            "2026-01-15",
        )];

        assert!(build_werka_status_details(&receipts, &[], "", "").is_empty());
    }
}
