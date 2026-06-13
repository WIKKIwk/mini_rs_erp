use crate::core::customer::models::CustomerDeliveryDetail;
use crate::core::customer::ports::CustomerDeliveryNoteDraft;
use crate::core::werka::models::DispatchRecord;

const DELIVERY_FLOW_STATE_NONE: i32 = 0;
const DELIVERY_FLOW_STATE_SUBMITTED: i32 = 1;
const CUSTOMER_STATE_PENDING: i32 = 1;
const CUSTOMER_STATE_REJECTED: i32 = 2;
const CUSTOMER_STATE_CONFIRMED: i32 = 3;
const CUSTOMER_STATE_PARTIAL: i32 = 4;

pub(super) fn detail_from_draft(draft: CustomerDeliveryNoteDraft) -> CustomerDeliveryDetail {
    let status = customer_delivery_status(&draft);
    let pending = status == "pending";
    CustomerDeliveryDetail {
        record: delivery_note_to_dispatch_record(draft),
        can_approve: pending,
        can_reject: pending,
        can_partially_accept: pending,
        can_report_claim: status == "accepted",
    }
}

pub(super) fn customer_delivery_status(item: &CustomerDeliveryNoteDraft) -> &'static str {
    if item.doc_status != 1 {
        return "draft";
    }
    if parse_accord_int(&item.accord_flow_state, DELIVERY_FLOW_STATE_NONE)
        != DELIVERY_FLOW_STATE_SUBMITTED
    {
        return "pending";
    }
    match parse_accord_int(&item.accord_customer_state, CUSTOMER_STATE_PENDING) {
        CUSTOMER_STATE_REJECTED => "rejected",
        CUSTOMER_STATE_CONFIRMED => "accepted",
        CUSTOMER_STATE_PARTIAL => "partial",
        _ => "pending",
    }
}

pub(super) fn customer_delivery_visible(item: &CustomerDeliveryNoteDraft) -> bool {
    item.doc_status == 1
        && parse_accord_int(&item.accord_flow_state, DELIVERY_FLOW_STATE_NONE)
            == DELIVERY_FLOW_STATE_SUBMITTED
}

pub(super) fn delivery_note_to_dispatch_record(item: CustomerDeliveryNoteDraft) -> DispatchRecord {
    let status = customer_delivery_status(&item);
    let (accepted_qty, returned_qty) = customer_decision_quantities(&item, status);
    let mut note = match status {
        "accepted" => "Customer tasdiqladi.".to_string(),
        "partial" => format!(
            "Customer qisman qabul qildi. Qabul: {:.2} {}. Qaytdi: {:.2} {}.",
            accepted_qty, item.uom, returned_qty, item.uom
        ),
        "rejected" => "Customer rad etdi.".to_string(),
        _ => String::new(),
    };
    if !item.accord_customer_reason.trim().is_empty() {
        note.push_str(" Sabab: ");
        note.push_str(item.accord_customer_reason.trim());
    }
    DispatchRecord {
        id: item.name,
        record_type: "delivery_note".to_string(),
        supplier_ref: item.customer,
        supplier_name: item.customer_name,
        item_code: item.item_code,
        item_name: item.item_name,
        uom: item.uom,
        sent_qty: item.qty,
        accepted_qty,
        note,
        status: status.to_string(),
        created_label: first_non_empty(&item.modified, &item.posting_date),
        ..DispatchRecord::default()
    }
}

fn customer_decision_quantities(item: &CustomerDeliveryNoteDraft, status: &str) -> (f64, f64) {
    let (mut accepted_qty, mut returned_qty) = extract_customer_decision_quantities(&item.remarks);
    if returned_qty <= 0.0 && item.returned_qty > 0.0 {
        returned_qty = item.returned_qty;
    }
    match status {
        "accepted" => {
            if accepted_qty <= 0.0 {
                accepted_qty = item.qty;
            }
            (accepted_qty, 0.0)
        }
        "partial" => {
            if accepted_qty <= 0.0 && returned_qty > 0.0 {
                accepted_qty = (item.qty - returned_qty).max(0.0);
            }
            if returned_qty <= 0.0 && accepted_qty > 0.0 {
                returned_qty = (item.qty - accepted_qty).max(0.0);
            }
            (accepted_qty, returned_qty)
        }
        "rejected" => (0.0, item.qty),
        _ => (accepted_qty, returned_qty),
    }
}

fn extract_customer_decision_quantities(remarks: &str) -> (f64, f64) {
    let mut accepted_qty = 0.0;
    let mut returned_qty = 0.0;
    for line in remarks.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("AQ:") {
            accepted_qty = value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0);
        } else if let Some(value) = trimmed.strip_prefix("AT:") {
            returned_qty = value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .unwrap_or(0.0);
        }
    }
    (accepted_qty, returned_qty)
}

fn parse_accord_int(value: &str, default: i32) -> i32 {
    value.trim().parse::<i32>().unwrap_or(default)
}

fn first_non_empty(left: &str, right: &str) -> String {
    if !left.trim().is_empty() {
        left.trim().to_string()
    } else {
        right.trim().to_string()
    }
}
