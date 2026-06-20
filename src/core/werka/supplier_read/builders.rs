use std::collections::HashMap;

use crate::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierStatusBreakdownEntry,
};
use crate::core::werka::ports::{PurchaseReceiptComment, PurchaseReceiptDraft};
use crate::core::werka::unannounced::purchase_receipt_to_dispatch_record;

use super::comments::is_supplier_acknowledgment_comment;

pub(super) fn build_supplier_summary_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
) -> SupplierHomeSummary {
    let mut summary = SupplierHomeSummary::default();
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        match record.status.as_str() {
            "pending" | "draft" => summary.pending_count += 1,
            "accepted" => summary.submitted_count += 1,
            "partial" | "rejected" | "cancelled" => summary.returned_count += 1,
            _ => {}
        }
    }
    summary
}

pub(super) fn build_supplier_history_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    comments_by_receipt: &HashMap<String, Vec<PurchaseReceiptComment>>,
    supplier_display_name: &str,
) -> Vec<DispatchRecord> {
    receipts
        .into_iter()
        .map(|receipt| {
            let mut record =
                purchase_receipt_to_dispatch_record(receipt.clone(), supplier_display_name);
            for comment in comments_by_receipt
                .get(receipt.name.trim())
                .into_iter()
                .flatten()
            {
                if !is_supplier_acknowledgment_comment(&comment.content) {
                    continue;
                }
                if !record.note.contains("Supplier tasdiqladi:") {
                    if !record.note.trim().is_empty() {
                        record.note.push('\n');
                    }
                    record.note.push_str(
                        "Supplier tasdiqladi: Tasdiqlayman, shu holat bo‘lganini ko‘rdim.",
                    );
                }
                break;
            }
            record
        })
        .collect()
}

pub(super) fn build_supplier_status_breakdown_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
    kind: &str,
) -> Vec<SupplierStatusBreakdownEntry> {
    let mut grouped = HashMap::<String, SupplierStatusBreakdownEntry>::new();
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        if !record_matches_supplier_breakdown(&record, kind) {
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

pub(super) fn build_supplier_status_details_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
    kind: &str,
    item_code: &str,
) -> Vec<DispatchRecord> {
    let needle = item_code.trim();
    let mut result = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        if !record_matches_supplier_breakdown(&record, kind) {
            continue;
        }
        if !needle.is_empty() && !record.item_code.trim().eq_ignore_ascii_case(needle) {
            continue;
        }
        result.push(record);
    }
    result
}

fn record_matches_supplier_breakdown(record: &DispatchRecord, kind: &str) -> bool {
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
