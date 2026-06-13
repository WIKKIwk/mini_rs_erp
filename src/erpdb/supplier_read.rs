use async_trait::async_trait;
use sqlx::query_as;

use std::collections::HashMap;

use crate::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierStatusBreakdownEntry,
};
use crate::core::werka::ports::{SupplierReadLookup, WerkaPortError};
use crate::erpdb::reader::DirectDbReader;
use crate::erpdb::werka_home::{
    PurchaseReceiptSummaryRow, classify_werka_receipt, purchase_receipt_to_record,
};
use crate::erpdb::werka_lookup::database_error;

#[async_trait]
impl SupplierReadLookup for DirectDbReader {
    async fn supplier_summary(
        &self,
        supplier_ref: &str,
    ) -> Result<SupplierHomeSummary, WerkaPortError> {
        let rows = query_as::<_, PurchaseReceiptSummaryRow>(SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL)
            .bind(supplier_ref.trim())
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(build_supplier_summary(&rows))
    }

    async fn supplier_history(
        &self,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        let rows = query_as::<_, PurchaseReceiptSummaryRow>(SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL)
            .bind(supplier_ref.trim())
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(build_supplier_history(&rows))
    }

    async fn supplier_status_breakdown(
        &self,
        supplier_ref: &str,
        kind: &str,
    ) -> Result<Vec<SupplierStatusBreakdownEntry>, WerkaPortError> {
        let rows = query_as::<_, PurchaseReceiptSummaryRow>(SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL)
            .bind(supplier_ref.trim())
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(build_supplier_status_breakdown(&rows, kind))
    }

    async fn supplier_status_details(
        &self,
        supplier_ref: &str,
        kind: &str,
        item_code: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        let rows = query_as::<_, PurchaseReceiptSummaryRow>(SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL)
            .bind(supplier_ref.trim())
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(build_supplier_status_details(&rows, kind, item_code))
    }
}

fn build_supplier_summary(rows: &[PurchaseReceiptSummaryRow]) -> SupplierHomeSummary {
    let mut summary = SupplierHomeSummary::default();
    for row in rows {
        let (status, _) = classify_werka_receipt(row);
        match status.as_str() {
            "pending" | "draft" => summary.pending_count += 1,
            "accepted" => summary.submitted_count += 1,
            "partial" | "rejected" | "cancelled" => summary.returned_count += 1,
            _ => {}
        }
    }
    summary
}

fn build_supplier_history(rows: &[PurchaseReceiptSummaryRow]) -> Vec<DispatchRecord> {
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let (_, include) = classify_werka_receipt(row);
        if include {
            result.push(purchase_receipt_to_record(row));
        }
    }
    result.sort_by(|left, right| right.created_label.cmp(&left.created_label));
    result
}

fn build_supplier_status_breakdown(
    rows: &[PurchaseReceiptSummaryRow],
    kind: &str,
) -> Vec<SupplierStatusBreakdownEntry> {
    let mut grouped = HashMap::<String, SupplierStatusBreakdownEntry>::new();
    for record in visible_supplier_records(rows) {
        if !record_matches_supplier_kind(&record, kind) {
            continue;
        }
        let key = if record.item_code.trim().is_empty() {
            record.item_name.trim().to_string()
        } else {
            record.item_code.trim().to_string()
        };
        let entry = grouped
            .entry(key)
            .or_insert_with(|| SupplierStatusBreakdownEntry {
                item_code: record.item_code.clone(),
                item_name: record.item_name.clone(),
                uom: record.uom.clone(),
                ..SupplierStatusBreakdownEntry::default()
            });
        entry.receipt_count += 1;
        entry.total_sent_qty += record.sent_qty;
        entry.total_accepted_qty += record.accepted_qty;
        entry.total_returned_qty += (record.sent_qty - record.accepted_qty).max(0.0);
        if entry.uom.trim().is_empty() {
            entry.uom = record.uom;
        }
    }

    let mut result = grouped.into_values().collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right.receipt_count.cmp(&left.receipt_count).then_with(|| {
            left.item_name
                .to_lowercase()
                .cmp(&right.item_name.to_lowercase())
        })
    });
    result
}

fn build_supplier_status_details(
    rows: &[PurchaseReceiptSummaryRow],
    kind: &str,
    item_code: &str,
) -> Vec<DispatchRecord> {
    let needle = item_code.trim();
    visible_supplier_records(rows)
        .into_iter()
        .filter(|record| record_matches_supplier_kind(record, kind))
        .filter(|record| needle.is_empty() || record.item_code.trim().eq_ignore_ascii_case(needle))
        .collect()
}

fn visible_supplier_records(rows: &[PurchaseReceiptSummaryRow]) -> Vec<DispatchRecord> {
    rows.iter()
        .filter_map(|row| {
            let (_, include) = classify_werka_receipt(row);
            include.then(|| purchase_receipt_to_record(row))
        })
        .collect()
}

fn record_matches_supplier_kind(record: &DispatchRecord, kind: &str) -> bool {
    match kind.trim() {
        "pending" => record.status == "pending" || record.status == "draft",
        "submitted" => record.status == "accepted",
        "returned" => {
            record.status == "partial"
                || record.status == "rejected"
                || record.status == "cancelled"
        }
        _ => false,
    }
}

const SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
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
      AND pr.supplier = ?
    ORDER BY pr.name DESC
"#;

#[cfg(test)]
mod tests {
    use super::{
        build_supplier_history, build_supplier_status_breakdown, build_supplier_status_details,
        build_supplier_summary,
    };
    use crate::erpdb::werka_home::PurchaseReceiptSummaryRow;

    #[test]
    fn supplier_summary_counts_statuses_like_go_reader() {
        let rows = vec![
            receipt("PR-PENDING", 0, "To Bill", 5.0, ""),
            receipt("PR-DRAFT", 0, "Draft", 5.0, ""),
            receipt("PR-OK", 1, "Completed", 5.0, ""),
            receipt(
                "PR-PARTIAL",
                1,
                "Completed",
                3.0,
                "TG:+998:20260126090003:5.0000",
            ),
            receipt("PR-CANCELLED", 2, "Cancelled", 5.0, ""),
        ];

        let summary = build_supplier_summary(&rows);

        assert_eq!(summary.pending_count, 2);
        assert_eq!(summary.submitted_count, 1);
        assert_eq!(summary.returned_count, 2);
    }

    #[test]
    fn supplier_history_filters_hidden_unannounced_and_sorts_like_go_reader() {
        let mut pending = receipt("PR-PENDING", 0, "To Bill", 5.0, "");
        pending.posting_date = "2026-01-26".to_string();
        let mut hidden = receipt("PR-HIDDEN", 0, "To Bill", 5.0, "");
        hidden.posting_date = "2026-01-27".to_string();
        hidden.remarks = "Accord Werka Aytilmagan: pending".to_string();
        let mut accepted = receipt("PR-ACCEPTED", 1, "Completed", 5.0, "");
        accepted.posting_date = "2026-01-28".to_string();

        let items = build_supplier_history(&[pending, hidden, accepted]);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "PR-ACCEPTED");
        assert_eq!(items[1].id, "PR-PENDING");
    }

    #[test]
    fn supplier_status_breakdown_groups_items_like_erp_fallback() {
        let rows = vec![
            receipt("PR-OK-1", 1, "Completed", 5.0, ""),
            receipt("PR-OK-2", 1, "Completed", 5.0, ""),
            receipt("PR-PENDING", 0, "To Bill", 2.0, ""),
        ];

        let items = build_supplier_status_breakdown(&rows, "submitted");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_code, "ITEM-001");
        assert_eq!(items[0].receipt_count, 2);
        assert_eq!(items[0].total_sent_qty, 10.0);
        assert_eq!(items[0].total_accepted_qty, 10.0);
        assert_eq!(items[0].total_returned_qty, 0.0);
    }

    #[test]
    fn supplier_status_details_filters_item_and_kind_like_erp_fallback() {
        let mut other_item = receipt("PR-OTHER", 1, "Completed", 5.0, "");
        other_item.item_code = "ITEM-002".to_string();
        let rows = vec![
            receipt("PR-OK", 1, "Completed", 5.0, ""),
            receipt("PR-PENDING", 0, "To Bill", 5.0, ""),
            other_item,
        ];

        let items = build_supplier_status_details(&rows, "submitted", "item-001");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "PR-OK");
    }

    fn receipt(
        name: &str,
        doc_status: i32,
        status: &str,
        qty: f64,
        marker: &str,
    ) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            doc_status,
            status: status.to_string(),
            total_qty: qty,
            posting_date: "2026-01-26".to_string(),
            supplier_delivery_note: if marker.is_empty() {
                "TG:+998:20260126090000:5.0000".to_string()
            } else {
                marker.to_string()
            },
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Nos".to_string(),
            ..PurchaseReceiptSummaryRow::default()
        }
    }
}
