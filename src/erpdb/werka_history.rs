use crate::core::werka::models::DispatchRecord;
use crate::erpdb::werka_home::{
    DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow, delivery_note_to_record, delivery_status,
    purchase_receipt_to_record,
};

const CUSTOMER_DELIVERY_RESULT_EVENT_PREFIX: &str = "customer_delivery_result:";
const SUPPLIER_ACK_EVENT_PREFIX: &str = "supplier_ack:";

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub(crate) struct SupplierAckRow {
    pub comment_id: String,
    pub created_label: String,
    pub supplier_ref: String,
    pub supplier_name: String,
    pub sent_qty: f64,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
}

pub(crate) fn build_werka_history(
    receipts: &[PurchaseReceiptSummaryRow],
    acks: &[SupplierAckRow],
    delivery_notes: &[DeliveryNoteSummaryRow],
    recent_limit: usize,
) -> Vec<DispatchRecord> {
    let mut result = Vec::with_capacity(receipts.len() + acks.len() + delivery_notes.len());

    for row in receipts {
        let record = purchase_receipt_to_record(row);
        if record.event_type == "werka_unannounced_pending" {
            continue;
        }
        result.push(record);
    }

    result.extend(acks.iter().map(supplier_ack_to_record));

    for row in delivery_notes {
        if let Some(record) = customer_result_to_record(row) {
            result.push(record);
        }
    }

    result.sort_by(|left, right| right.created_label.cmp(&left.created_label));
    if result.len() > recent_limit {
        result.truncate(recent_limit);
    }
    result
}

fn supplier_ack_to_record(row: &SupplierAckRow) -> DispatchRecord {
    DispatchRecord {
        id: format!("{}{}", SUPPLIER_ACK_EVENT_PREFIX, row.comment_id.trim()),
        supplier_ref: row.supplier_ref.trim().to_string(),
        supplier_name: row.supplier_name.trim().to_string(),
        item_code: row.item_code.trim().to_string(),
        item_name: row.item_name.trim().to_string(),
        uom: row.uom.trim().to_string(),
        sent_qty: row.sent_qty,
        accepted_qty: row.sent_qty,
        event_type: "supplier_ack".to_string(),
        highlight: "Supplier mahsulotni qaytarganingizni tasdiqladi".to_string(),
        status: "accepted".to_string(),
        created_label: row.created_label.trim().to_string(),
        ..DispatchRecord::default()
    }
}

fn customer_result_to_record(row: &DeliveryNoteSummaryRow) -> Option<DispatchRecord> {
    let status = delivery_status(row);
    if status != "accepted" && status != "partial" && status != "rejected" {
        return None;
    }

    let mut record = delivery_note_to_record(row);
    record.id = format!(
        "{}{}",
        CUSTOMER_DELIVERY_RESULT_EVENT_PREFIX,
        record.id.trim()
    );
    match status.as_str() {
        "accepted" => {
            record.event_type = "customer_delivery_confirmed".to_string();
            record.highlight = "Customer mahsulotni qabul qildi".to_string();
        }
        "partial" => {
            record.event_type = "customer_delivery_partial".to_string();
            record.highlight = "Customer mahsulotning bir qismini qaytardi".to_string();
        }
        "rejected" => {
            record.event_type = "customer_delivery_rejected".to_string();
            record.highlight = "Customer mahsulotni rad etdi".to_string();
        }
        _ => {}
    }
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(name: &str, date: &str) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            doc_status: 1,
            status: String::new(),
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

    fn delivery(name: &str, customer_state: i32, modified: &str) -> DeliveryNoteSummaryRow {
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
            accord_flow_state: 1,
            accord_customer_state: customer_state,
        }
    }

    #[test]
    fn history_combines_and_sorts_recent_events_like_go() {
        let receipts = vec![receipt("PR-OLD", "2026-01-15")];
        let acks = vec![SupplierAckRow {
            comment_id: "COMM-001".to_string(),
            created_label: "2026-01-17 08:00:00".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            sent_qty: 4.0,
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
        }];
        let deliveries = vec![delivery("DN-MID", 3, "2026-01-16 09:00:00")];

        let items = build_werka_history(&receipts, &acks, &deliveries, 120);

        let ids: Vec<_> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "supplier_ack:COMM-001",
                "customer_delivery_result:DN-MID",
                "PR-OLD"
            ]
        );
        assert_eq!(items[0].event_type, "supplier_ack");
        assert_eq!(items[1].event_type, "customer_delivery_confirmed");
    }

    #[test]
    fn history_skips_non_result_customer_delivery_notes_and_limits() {
        let deliveries = vec![
            delivery("DN-PENDING", 0, "2026-01-19 09:00:00"),
            delivery("DN-REJECTED", 2, "2026-01-18 09:00:00"),
            delivery("DN-PARTIAL", 4, "2026-01-17 09:00:00"),
        ];

        let items = build_werka_history(&[], &[], &deliveries, 1);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "customer_delivery_result:DN-REJECTED");
        assert_eq!(items[0].event_type, "customer_delivery_rejected");
    }
}
