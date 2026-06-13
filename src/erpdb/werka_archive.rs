use std::collections::BTreeMap;

use sqlx::{MySqlPool, query_as};
use time::Date;

use crate::core::werka::models::{
    ArchiveTotalByUom, DispatchRecord, WerkaArchiveResponse, WerkaArchiveSummary,
};
use crate::erpdb::werka_home::{
    DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow, classify_werka_receipt,
    delivery_note_to_record, delivery_visible, purchase_receipt_to_record,
};

pub(crate) async fn read_werka_archive(
    pool: &MySqlPool,
    kind: &str,
    period: &str,
    from: Option<Date>,
    to: Option<Date>,
) -> Result<WerkaArchiveResponse, sqlx::Error> {
    let normalized_kind = normalize_archive_kind(kind);
    let from_date = format_archive_date(from);
    let to_date = format_archive_date(to);
    let (from_date_time, to_exclusive_date_time) = format_archive_date_time_range(from, to);

    let receipts = async {
        if normalized_kind != "received" && normalized_kind != "returned" {
            return Ok(Vec::new());
        }
        query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_FILTERED_SQL)
            .bind("")
            .bind("")
            .bind(&from_date)
            .bind(&from_date)
            .bind(&to_date)
            .bind(&to_date)
            .fetch_all(pool)
            .await
    };
    let delivery_notes = async {
        if normalized_kind != "sent" && normalized_kind != "returned" {
            return Ok(Vec::new());
        }
        query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_FILTERED_SQL)
            .bind("")
            .bind("")
            .bind(&from_date_time)
            .bind(&from_date_time)
            .bind(&to_exclusive_date_time)
            .bind(&to_exclusive_date_time)
            .fetch_all(pool)
            .await
    };
    let (receipts, delivery_notes) = tokio::try_join!(receipts, delivery_notes)?;

    Ok(build_werka_archive(
        &receipts,
        &delivery_notes,
        kind,
        period,
        from,
        to,
    ))
}

pub(crate) fn build_werka_archive(
    receipts: &[PurchaseReceiptSummaryRow],
    delivery_notes: &[DeliveryNoteSummaryRow],
    kind: &str,
    period: &str,
    from: Option<Date>,
    to: Option<Date>,
) -> WerkaArchiveResponse {
    let normalized_kind = normalize_archive_kind(kind);
    let normalized_period = normalize_archive_period(period);
    let mut records = Vec::with_capacity(receipts.len() + delivery_notes.len());

    if normalized_kind == "received" || normalized_kind == "returned" {
        for row in receipts {
            let (status, include) = classify_werka_receipt(row);
            if !include {
                continue;
            }
            let record = purchase_receipt_to_record(row);
            if archive_includes_record(&normalized_kind, &record, &status) {
                records.push(record);
            }
        }
    }

    if normalized_kind == "sent" || normalized_kind == "returned" {
        for row in delivery_notes {
            if !delivery_visible(row) {
                continue;
            }
            let record = delivery_note_to_record(row);
            if archive_includes_record(&normalized_kind, &record, &record.status) {
                records.push(record);
            }
        }
    }

    records.sort_by(|left, right| right.created_label.cmp(&left.created_label));

    WerkaArchiveResponse {
        kind: normalized_kind.clone(),
        period: normalized_period,
        from: format_archive_date(from),
        to: format_archive_date(to),
        summary: build_archive_summary(&normalized_kind, &records),
        items: records,
    }
}

pub(crate) fn normalize_archive_kind(kind: &str) -> String {
    match kind.trim().to_lowercase().as_str() {
        "received" | "returned" => kind.trim().to_lowercase(),
        _ => "sent".to_string(),
    }
}

pub(crate) fn normalize_archive_period(period: &str) -> String {
    match period.trim().to_lowercase().as_str() {
        "daily" | "monthly" | "custom" => period.trim().to_lowercase(),
        _ => "yearly".to_string(),
    }
}

pub(crate) fn format_archive_date(value: Option<Date>) -> String {
    value.map(|date| date.to_string()).unwrap_or_default()
}

pub(crate) fn format_archive_date_time_range(
    from: Option<Date>,
    to: Option<Date>,
) -> (String, String) {
    let from_value = from
        .map(|date| format!("{date} 00:00:00"))
        .unwrap_or_default();
    let to_value = to
        .and_then(|date| date.next_day())
        .map(|date| format!("{date} 00:00:00"))
        .unwrap_or_default();
    (from_value, to_value)
}

fn archive_includes_record(kind: &str, record: &DispatchRecord, status: &str) -> bool {
    match kind {
        "received" => {
            record.record_type == "purchase_receipt"
                && (status == "accepted" || status == "partial")
        }
        "returned" => status == "partial" || status == "rejected" || status == "cancelled",
        _ => record.record_type == "delivery_note",
    }
}

fn build_archive_summary(kind: &str, records: &[DispatchRecord]) -> WerkaArchiveSummary {
    let mut totals = BTreeMap::<String, f64>::new();
    for record in records {
        let uom = if record.uom.trim().is_empty() {
            "Nos".to_string()
        } else {
            record.uom.trim().to_string()
        };
        *totals.entry(uom).or_default() += archive_metric_qty(kind, record);
    }

    let mut totals_by_uom: Vec<_> = totals
        .into_iter()
        .map(|(uom, qty)| ArchiveTotalByUom { uom, qty })
        .collect();
    totals_by_uom.sort_by(|left, right| left.uom.to_lowercase().cmp(&right.uom.to_lowercase()));

    WerkaArchiveSummary {
        record_count: records.len(),
        totals_by_uom,
    }
}

fn archive_metric_qty(kind: &str, record: &DispatchRecord) -> f64 {
    match kind {
        "received" => {
            if record.accepted_qty > 0.0 {
                record.accepted_qty
            } else {
                record.sent_qty
            }
        }
        "returned" => (record.sent_qty - record.accepted_qty).max(0.0),
        _ => record.sent_qty,
    }
}

const PURCHASE_RECEIPT_ROWS_FILTERED_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
      AND (? = '' OR pr.supplier = ?)
      AND (? = '' OR pr.posting_date >= ?)
      AND (? = '' OR pr.posting_date <= ?)
    ORDER BY pr.name DESC
"#;

const DELIVERY_NOTE_ROWS_FILTERED_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS qty,
        CAST(COALESCE(dni.returned_qty, 0) AS DOUBLE) AS returned_qty,
        COALESCE(dn.accord_customer_reason, '') AS customer_reason,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE (? = '' OR dn.customer = ?)
      AND (? = '' OR dn.modified >= ?)
      AND (? = '' OR dn.modified < ?)
    ORDER BY dn.name DESC
"#;

#[cfg(test)]
mod tests {
    use time::Month;

    use super::*;

    fn receipt(
        name: &str,
        doc_status: i32,
        total_qty: f64,
        marker: &str,
        posting_date: &str,
    ) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
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

    fn delivery(name: &str, state: i32, modified: &str) -> DeliveryNoteSummaryRow {
        DeliveryNoteSummaryRow {
            name: name.to_string(),
            customer: "CUST-001".to_string(),
            customer_name: "Customer".to_string(),
            doc_status: 1,
            modified: modified.to_string(),
            qty: 5.0,
            returned_qty: 1.0,
            customer_reason: String::new(),
            item_code: "ITEM-002".to_string(),
            item_name: "Item 2".to_string(),
            uom: "Pcs".to_string(),
            accord_flow_state: 1,
            accord_customer_state: state,
        }
    }

    #[test]
    fn archive_defaults_to_sent_and_yearly() {
        let data = build_werka_archive(
            &[],
            &[delivery("DN-1", 3, "2026-01-16")],
            "",
            "",
            None,
            None,
        );

        assert_eq!(data.kind, "sent");
        assert_eq!(data.period, "yearly");
        assert_eq!(data.summary.record_count, 1);
        assert_eq!(data.summary.totals_by_uom[0].qty, 5.0);
    }

    #[test]
    fn archive_received_uses_accepted_qty_and_filters_receipts() {
        let receipts = vec![
            receipt("PR-accepted", 1, 4.0, "TG:+998:1:4.0000", "2026-01-16"),
            receipt("PR-returned", 1, 2.0, "TG:+998:1:5.0000", "2026-01-17"),
            receipt("PR-pending", 0, 1.0, "TG:+998:1:1.0000", "2026-01-18"),
        ];

        let data = build_werka_archive(&receipts, &[], "received", "daily", None, None);

        assert_eq!(data.items.len(), 2);
        assert_eq!(data.items[0].id, "PR-returned");
        assert_eq!(data.summary.totals_by_uom[0].qty, 6.0);
    }

    #[test]
    fn archive_returned_combines_receipts_and_deliveries() {
        let receipts = vec![receipt("PR-1", 1, 2.0, "TG:+998:1:5.0000", "2026-01-16")];
        let deliveries = vec![delivery("DN-1", 4, "2026-01-17 10:00:00")];

        let data = build_werka_archive(&receipts, &deliveries, "returned", "custom", None, None);

        assert_eq!(data.summary.record_count, 2);
        assert_eq!(data.items[0].id, "DN-1");
        assert_eq!(data.summary.totals_by_uom.len(), 2);
    }

    #[test]
    fn archive_formats_dates_like_go() {
        let from = Date::from_calendar_date(2026, Month::January, 16).expect("date");
        let to = Date::from_calendar_date(2026, Month::January, 20).expect("date");
        let (from_dt, to_dt) = format_archive_date_time_range(Some(from), Some(to));

        assert_eq!(format_archive_date(Some(from)), "2026-01-16");
        assert_eq!(from_dt, "2026-01-16 00:00:00");
        assert_eq!(to_dt, "2026-01-21 00:00:00");
    }
}
