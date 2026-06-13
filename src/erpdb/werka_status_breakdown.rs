#[cfg(test)]
use std::collections::BTreeMap;

use crate::core::werka::models::DispatchRecord;
#[cfg(test)]
use crate::core::werka::models::WerkaStatusBreakdownEntry;
#[cfg(test)]
use crate::erpdb::werka_home::{
    DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow, delivery_note_to_record,
    purchase_receipt_to_record,
};

#[cfg(test)]
pub(crate) fn build_werka_status_breakdown(
    receipts: &[PurchaseReceiptSummaryRow],
    delivery_notes: &[DeliveryNoteSummaryRow],
    kind: &str,
) -> Vec<WerkaStatusBreakdownEntry> {
    let mut grouped = BTreeMap::<String, WerkaStatusBreakdownEntry>::new();

    for record in receipts.iter().map(purchase_receipt_to_record) {
        add_if_matches(&mut grouped, record, kind);
    }

    for record in delivery_notes.iter().map(delivery_note_to_record) {
        add_if_matches(&mut grouped, record, kind);
    }

    let mut result: Vec<_> = grouped.into_values().collect();
    result.sort_by(|left, right| {
        right.receipt_count.cmp(&left.receipt_count).then_with(|| {
            left.supplier_name
                .to_lowercase()
                .cmp(&right.supplier_name.to_lowercase())
        })
    });
    result
}

#[cfg(test)]
fn add_if_matches(
    grouped: &mut BTreeMap<String, WerkaStatusBreakdownEntry>,
    record: DispatchRecord,
    kind: &str,
) {
    if !matches_werka_breakdown(&record, kind) {
        return;
    }

    let key = non_empty(record.supplier_ref.trim(), record.supplier_name.trim());
    let entry = grouped
        .entry(key)
        .or_insert_with(|| WerkaStatusBreakdownEntry {
            supplier_ref: record.supplier_ref.clone(),
            supplier_name: record.supplier_name.clone(),
            uom: record.uom.clone(),
            ..WerkaStatusBreakdownEntry::default()
        });

    entry.receipt_count += 1;
    entry.total_sent_qty += record.sent_qty;
    entry.total_accepted_qty += record.accepted_qty;
    entry.total_returned_qty += (record.sent_qty - record.accepted_qty).max(0.0);
    if entry.uom.trim().is_empty() {
        entry.uom = record.uom;
    }
}

pub(crate) fn matches_werka_breakdown(record: &DispatchRecord, kind: &str) -> bool {
    match kind.trim() {
        "pending" => record.status == "pending" || record.status == "draft",
        "confirmed" => record.status == "accepted",
        "returned" => {
            record.status == "partial"
                || record.status == "rejected"
                || record.status == "cancelled"
        }
        _ => false,
    }
}

#[cfg(test)]
fn non_empty(primary: &str, fallback: &str) -> String {
    if primary.is_empty() {
        fallback.to_string()
    } else {
        primary.to_string()
    }
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
    ) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: supplier.to_string(),
            supplier_name: format!("Supplier {supplier}"),
            doc_status,
            status: String::new(),
            total_qty,
            posting_date: "2026-01-16".to_string(),
            supplier_delivery_note: marker.to_string(),
            remarks: String::new(),
            currency: "UZS".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            amount: 12.0,
        }
    }

    fn delivery(customer: &str, customer_state: i32, qty: f64) -> DeliveryNoteSummaryRow {
        DeliveryNoteSummaryRow {
            name: format!("DN-{customer}"),
            customer: customer.to_string(),
            customer_name: format!("Customer {customer}"),
            doc_status: 1,
            modified: "2026-01-16 09:00:00".to_string(),
            qty,
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
    fn breakdown_groups_totals_and_sorts_like_go() {
        let receipts = vec![
            receipt("SUP-B", "PR-1", 1, 4.0, "TG:+998:1:4.0000"),
            receipt("SUP-A", "PR-2", 1, 3.0, "TG:+998:1:3.0000"),
            receipt("SUP-A", "PR-3", 1, 2.0, "TG:+998:1:2.0000"),
        ];
        let items = build_werka_status_breakdown(&receipts, &[], "confirmed");

        assert_eq!(items[0].supplier_ref, "SUP-A");
        assert_eq!(items[0].receipt_count, 2);
        assert_eq!(items[0].total_sent_qty, 5.0);
        assert_eq!(items[0].total_accepted_qty, 5.0);
        assert_eq!(items[1].supplier_ref, "SUP-B");
    }

    #[test]
    fn breakdown_matches_pending_confirmed_and_returned() {
        let receipts = vec![
            receipt("SUP-P", "PR-P", 0, 4.0, "TG:+998:1:4.0000"),
            receipt("SUP-R", "PR-R", 1, 2.0, "TG:+998:1:4.0000"),
        ];
        let deliveries = vec![delivery("CUST-C", 3, 5.0), delivery("CUST-R", 2, 6.0)];

        assert_eq!(
            build_werka_status_breakdown(&receipts, &deliveries, "pending")[0].supplier_ref,
            "SUP-P"
        );
        assert_eq!(
            build_werka_status_breakdown(&receipts, &deliveries, "confirmed").len(),
            1
        );
        let returned = build_werka_status_breakdown(&receipts, &deliveries, "returned");
        assert_eq!(returned.len(), 2);
        assert!(build_werka_status_breakdown(&receipts, &deliveries, "").is_empty());
    }
}
