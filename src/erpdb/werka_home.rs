use crate::core::werka::models::DispatchRecord;
#[cfg(test)]
use crate::core::werka::models::{WerkaHomeData, WerkaHomeSummary};

const DELIVERY_FLOW_STATE_SUBMITTED: i32 = 1;
const CUSTOMER_STATE_REJECTED: i32 = 2;
const CUSTOMER_STATE_CONFIRMED: i32 = 3;
const CUSTOMER_STATE_PARTIAL: i32 = 4;
const TELEGRAM_RECEIPT_MARKER_PREFIX: &str = "TG:";
const WERKA_UNANNOUNCED_PREFIX: &str = "Accord Werka Aytilmagan:";

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub(crate) struct PurchaseReceiptSummaryRow {
    pub name: String,
    pub supplier: String,
    pub supplier_name: String,
    pub doc_status: i32,
    pub status: String,
    pub total_qty: f64,
    pub posting_date: String,
    pub supplier_delivery_note: String,
    pub remarks: String,
    pub currency: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub(crate) struct DeliveryNoteSummaryRow {
    pub name: String,
    pub customer: String,
    pub customer_name: String,
    pub doc_status: i32,
    pub modified: String,
    pub qty: f64,
    pub returned_qty: f64,
    #[allow(dead_code)]
    pub customer_reason: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub accord_flow_state: i32,
    pub accord_customer_state: i32,
}

#[cfg(test)]
pub(crate) fn build_werka_home(
    receipts: &[PurchaseReceiptSummaryRow],
    delivery_notes: &[DeliveryNoteSummaryRow],
    pending_limit: usize,
) -> WerkaHomeData {
    let mut data = WerkaHomeData {
        summary: WerkaHomeSummary::default(),
        pending_items: Vec::with_capacity(pending_limit),
    };

    for row in receipts {
        let (status, include) = classify_werka_receipt(row);
        if !include {
            continue;
        }
        match status.as_str() {
            "pending" | "draft" => {
                data.summary.pending_count += 1;
                if pending_limit == 0 || data.pending_items.len() < pending_limit {
                    data.pending_items.push(purchase_receipt_to_record(row));
                }
            }
            "accepted" => data.summary.confirmed_count += 1,
            "partial" | "rejected" | "cancelled" => data.summary.returned_count += 1,
            _ => {}
        }
    }

    for row in delivery_notes {
        if !delivery_visible(row) {
            continue;
        }
        let status = delivery_status(row);
        match status.as_str() {
            "pending" => {
                data.summary.pending_count += 1;
                if pending_limit == 0 || data.pending_items.len() < pending_limit {
                    data.pending_items.push(delivery_note_to_record(row));
                }
            }
            "accepted" => data.summary.confirmed_count += 1,
            "partial" | "rejected" | "cancelled" => data.summary.returned_count += 1,
            _ => {}
        }
    }

    data.pending_items
        .sort_by(|left, right| right.created_label.cmp(&left.created_label));
    if pending_limit > 0 && data.pending_items.len() > pending_limit {
        data.pending_items.truncate(pending_limit);
    }
    data
}

pub(crate) fn classify_werka_receipt(row: &PurchaseReceiptSummaryRow) -> (String, bool) {
    classify_werka_receipt_fields(
        row.doc_status,
        &row.status,
        row.total_qty,
        &row.supplier_delivery_note,
        &row.remarks,
    )
}

pub(crate) fn classify_werka_receipt_fields(
    doc_status: i32,
    raw_status: &str,
    total_qty: f64,
    supplier_delivery_note: &str,
    remarks: &str,
) -> (String, bool) {
    let mut sent_qty = total_qty;
    if let Some(marker_qty) = parse_telegram_receipt_marker_qty(supplier_delivery_note)
        && marker_qty > sent_qty
    {
        sent_qty = marker_qty;
    }

    let mut status = "pending";
    if doc_status == 2 || raw_status.trim().eq_ignore_ascii_case("Cancelled") {
        status = "cancelled";
    } else if doc_status == 1 {
        status = purchase_receipt_status_from_quantities(sent_qty, total_qty);
    } else if raw_status.trim().eq_ignore_ascii_case("Draft") {
        status = "draft";
    }

    let unannounced_state = extract_werka_unannounced_state(remarks);
    if doc_status == 0 && unannounced_state == "pending" {
        return (status.to_string(), false);
    }
    if status == "accepted" && unannounced_state == "approved" {
        return (status.to_string(), false);
    }
    (status.to_string(), true)
}

pub(crate) fn purchase_receipt_to_record(row: &PurchaseReceiptSummaryRow) -> DispatchRecord {
    let mut sent_qty = row.total_qty;
    if let Some(marker_qty) = parse_telegram_receipt_marker_qty(&row.supplier_delivery_note)
        && marker_qty > sent_qty
    {
        sent_qty = marker_qty;
    }
    let (status, _) = classify_werka_receipt(row);
    let accepted_qty = match status.as_str() {
        "accepted" | "partial" => row.total_qty,
        _ => 0.0,
    };

    DispatchRecord {
        id: row.name.trim().to_string(),
        record_type: "purchase_receipt".to_string(),
        supplier_ref: row.supplier.trim().to_string(),
        supplier_name: row.supplier_name.trim().to_string(),
        item_code: row.item_code.trim().to_string(),
        item_name: row.item_name.trim().to_string(),
        uom: row.uom.trim().to_string(),
        sent_qty,
        accepted_qty,
        amount: row.amount,
        currency: row.currency.trim().to_string(),
        status,
        created_label: row.posting_date.trim().to_string(),
        ..DispatchRecord::default()
    }
}

pub(crate) fn delivery_note_to_record(row: &DeliveryNoteSummaryRow) -> DispatchRecord {
    let status = delivery_status(row);
    let (mut accepted_qty, returned_qty) = delivery_note_decision_quantities(row, &status);
    if status == "accepted" && accepted_qty <= 0.0 {
        accepted_qty = row.qty;
    }
    if status == "partial" && accepted_qty <= 0.0 && returned_qty > 0.0 {
        accepted_qty = (row.qty - returned_qty).max(0.0);
    }

    DispatchRecord {
        id: row.name.trim().to_string(),
        record_type: "delivery_note".to_string(),
        supplier_ref: row.customer.trim().to_string(),
        supplier_name: row.customer_name.trim().to_string(),
        item_code: row.item_code.trim().to_string(),
        item_name: row.item_name.trim().to_string(),
        uom: row.uom.trim().to_string(),
        sent_qty: row.qty,
        accepted_qty,
        status,
        created_label: row.modified.trim().to_string(),
        ..DispatchRecord::default()
    }
}

pub(crate) fn delivery_visible(row: &DeliveryNoteSummaryRow) -> bool {
    row.doc_status == 1 && row.accord_flow_state == DELIVERY_FLOW_STATE_SUBMITTED
}

pub(crate) fn delivery_status(row: &DeliveryNoteSummaryRow) -> String {
    delivery_status_from_state(
        row.doc_status,
        row.accord_flow_state,
        row.accord_customer_state,
    )
    .to_string()
}

pub(crate) fn delivery_status_from_state(
    doc_status: i32,
    flow_state: i32,
    customer_state: i32,
) -> &'static str {
    if doc_status != 1 {
        return "draft";
    }
    if flow_state != DELIVERY_FLOW_STATE_SUBMITTED {
        return "pending";
    }
    match customer_state {
        CUSTOMER_STATE_REJECTED => "rejected",
        CUSTOMER_STATE_CONFIRMED => "accepted",
        CUSTOMER_STATE_PARTIAL => "partial",
        _ => "pending",
    }
}

fn delivery_note_decision_quantities(row: &DeliveryNoteSummaryRow, status: &str) -> (f64, f64) {
    match status {
        "accepted" => (row.qty, 0.0),
        "partial" => {
            let returned_qty = if row.returned_qty <= 0.0 {
                row.qty.max(0.0)
            } else {
                row.returned_qty
            };
            ((row.qty - returned_qty).max(0.0), returned_qty)
        }
        "rejected" | "cancelled" => (0.0, row.qty),
        _ => (0.0, 0.0),
    }
}

fn purchase_receipt_status_from_quantities(sent_qty: f64, accepted_qty: f64) -> &'static str {
    if accepted_qty <= 0.0 {
        "rejected"
    } else if sent_qty > 0.0 && accepted_qty < sent_qty {
        "partial"
    } else {
        "accepted"
    }
}

fn parse_telegram_receipt_marker_qty(marker: &str) -> Option<f64> {
    let trimmed = marker.trim();
    if !trimmed.starts_with(TELEGRAM_RECEIPT_MARKER_PREFIX) {
        return None;
    }
    trimmed
        .split(':')
        .next_back()
        .and_then(|value| value.trim().parse::<f64>().ok())
}

fn extract_werka_unannounced_state(remarks: &str) -> String {
    remarks
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .find_map(|line| {
            line.strip_prefix(WERKA_UNANNOUNCED_PREFIX)
                .map(|value| value.trim().to_lowercase())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(
        name: &str,
        doc_status: i32,
        status: &str,
        marker: &str,
        date: &str,
    ) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            doc_status,
            status: status.to_string(),
            total_qty: 4.0,
            posting_date: date.to_string(),
            supplier_delivery_note: marker.to_string(),
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
            returned_qty: 2.0,
            customer_reason: String::new(),
            item_code: "ITEM-002".to_string(),
            item_name: "Item 2".to_string(),
            uom: "Pcs".to_string(),
            accord_flow_state: DELIVERY_FLOW_STATE_SUBMITTED,
            accord_customer_state: customer_state,
        }
    }

    #[test]
    fn summary_counts_receipts_and_delivery_notes_like_go() {
        let receipts = vec![
            receipt(
                "PR-PENDING",
                0,
                "",
                "TG:+998:20260116080000:10.0000",
                "2026-01-16",
            ),
            receipt(
                "PR-ACCEPTED",
                1,
                "",
                "TG:+998:20260116080100:4.0000",
                "2026-01-16",
            ),
            receipt(
                "PR-PARTIAL",
                1,
                "",
                "TG:+998:20260116080200:9.0000",
                "2026-01-16",
            ),
        ];
        let deliveries = vec![
            delivery("DN-PENDING", 0, "2026-01-16 09:00:00"),
            delivery(
                "DN-ACCEPTED",
                CUSTOMER_STATE_CONFIRMED,
                "2026-01-16 09:01:00",
            ),
            delivery(
                "DN-REJECTED",
                CUSTOMER_STATE_REJECTED,
                "2026-01-16 09:02:00",
            ),
        ];

        let data = build_werka_home(&receipts, &deliveries, 20);

        assert_eq!(data.summary.pending_count, 2);
        assert_eq!(data.summary.confirmed_count, 2);
        assert_eq!(data.summary.returned_count, 2);
        assert_eq!(data.pending_items.len(), 2);
        assert_eq!(data.pending_items[0].id, "DN-PENDING");
        assert_eq!(data.pending_items[1].id, "PR-PENDING");
        assert_eq!(data.pending_items[1].sent_qty, 10.0);
    }

    #[test]
    fn pending_limit_matches_go_preappend_behavior() {
        let receipts = vec![receipt(
            "PR-OLD",
            0,
            "",
            "TG:+998:20260116080000:4.0000",
            "2026-01-15",
        )];
        let deliveries = vec![delivery("DN-NEW", 0, "2026-01-16 09:00:00")];

        let data = build_werka_home(&receipts, &deliveries, 1);

        assert_eq!(data.summary.pending_count, 2);
        assert_eq!(data.pending_items.len(), 1);
        assert_eq!(data.pending_items[0].id, "PR-OLD");
    }

    #[test]
    fn hides_unannounced_pending_and_approved_receipts_like_go() {
        let mut hidden_pending =
            receipt("PR-HIDDEN-PENDING", 0, "", "TG:+998:1:1.0000", "2026-01-16");
        hidden_pending.remarks = "Accord Werka Aytilmagan: pending".to_string();
        let mut hidden_approved = receipt(
            "PR-HIDDEN-APPROVED",
            1,
            "",
            "TG:+998:1:4.0000",
            "2026-01-16",
        );
        hidden_approved.remarks = "Accord Werka Aytilmagan: approved".to_string();

        let data = build_werka_home(&[hidden_pending, hidden_approved], &[], 20);

        assert_eq!(data.summary.pending_count, 0);
        assert_eq!(data.summary.confirmed_count, 0);
        assert!(data.pending_items.is_empty());
    }
}
