use crate::core::werka::models::{DispatchRecord, WerkaHomeSummary, WerkaStatusBreakdownEntry};

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct WerkaSummaryPushdownRow {
    pending_count: i64,
    confirmed_count: i64,
    returned_count: i64,
}

impl From<WerkaSummaryPushdownRow> for WerkaHomeSummary {
    fn from(row: WerkaSummaryPushdownRow) -> Self {
        Self {
            pending_count: row.pending_count,
            confirmed_count: row.confirmed_count,
            returned_count: row.returned_count,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct WerkaDispatchRecordPushdownRow {
    id: String,
    record_type: String,
    supplier_ref: String,
    supplier_name: String,
    item_code: String,
    item_name: String,
    uom: String,
    sent_qty: f64,
    accepted_qty: f64,
    amount: f64,
    currency: String,
    status: String,
    created_label: String,
}

impl From<WerkaDispatchRecordPushdownRow> for DispatchRecord {
    fn from(row: WerkaDispatchRecordPushdownRow) -> Self {
        Self {
            id: row.id,
            record_type: row.record_type,
            supplier_ref: row.supplier_ref,
            supplier_name: row.supplier_name,
            item_code: row.item_code,
            item_name: row.item_name,
            uom: row.uom,
            sent_qty: row.sent_qty,
            accepted_qty: row.accepted_qty,
            amount: row.amount,
            currency: row.currency,
            status: row.status,
            created_label: row.created_label,
            ..Self::default()
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct WerkaStatusBreakdownPushdownRow {
    supplier_ref: String,
    supplier_name: String,
    receipt_count: i64,
    total_sent_qty: f64,
    total_accepted_qty: f64,
    total_returned_qty: f64,
    uom: String,
}

impl From<WerkaStatusBreakdownPushdownRow> for WerkaStatusBreakdownEntry {
    fn from(row: WerkaStatusBreakdownPushdownRow) -> Self {
        Self {
            supplier_ref: row.supplier_ref,
            supplier_name: row.supplier_name,
            receipt_count: row.receipt_count,
            total_sent_qty: row.total_sent_qty,
            total_accepted_qty: row.total_accepted_qty,
            total_returned_qty: row.total_returned_qty,
            uom: row.uom,
        }
    }
}
