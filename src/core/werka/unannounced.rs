use std::sync::Arc;

use crate::core::werka::models::{DispatchRecord, NotificationComment, NotificationDetail};
use crate::core::werka::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierUnannouncedWriter, WerkaHomeLookup,
    WerkaPortError, WerkaSupplierAdminState, WerkaSupplierAdminStateLookup, WerkaUnannouncedWriter,
};

const WERKA_UNANNOUNCED_PREFIX: &str = "Accord Werka Aytilmagan:";
const WERKA_UNANNOUNCED_REASON_PREFIX: &str = "Accord Werka Aytilmagan Sabab:";

pub(crate) fn upsert_werka_unannounced_in_remarks(
    existing_note: &str,
    state: &str,
    reason: &str,
) -> String {
    let mut filtered = Vec::new();
    for line in existing_note.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(WERKA_UNANNOUNCED_PREFIX)
            || trimmed.starts_with(WERKA_UNANNOUNCED_REASON_PREFIX)
        {
            continue;
        }
        filtered.push(trimmed.to_string());
    }
    if !state.trim().is_empty() {
        filtered.push(format!("{WERKA_UNANNOUNCED_PREFIX} {}", state.trim()));
    }
    if !reason.trim().is_empty() {
        filtered.push(format!(
            "{WERKA_UNANNOUNCED_REASON_PREFIX} {}",
            reason.trim()
        ));
    }
    filtered.join("\n")
}

pub(crate) fn purchase_receipt_to_dispatch_record(
    draft: PurchaseReceiptDraft,
    fallback_supplier_name: &str,
) -> DispatchRecord {
    let unannounced_state = extract_werka_unannounced_state(&draft.remarks);
    let sent_qty = parse_telegram_receipt_marker_qty(&draft.supplier_delivery_note)
        .filter(|marker_qty| *marker_qty > draft.qty)
        .unwrap_or(draft.qty);
    let (accepted_from_note, returned_from_note) =
        extract_accord_decision_quantities(&draft.remarks);
    let status = if (draft.doc_status == 0
        && (unannounced_state == "rejected"
            || (accepted_from_note <= 0.0
                && returned_from_note >= sent_qty
                && returned_from_note > 0.0)))
        || draft.doc_status == 2
        || draft.status.trim().eq_ignore_ascii_case("Cancelled")
    {
        "cancelled"
    } else if draft.doc_status == 1 {
        dispatch_status_from_quantities(sent_qty, draft.qty)
    } else if draft.status.trim().eq_ignore_ascii_case("Draft") {
        "draft"
    } else {
        "pending"
    };
    let mut accepted_qty = if draft.doc_status == 1 {
        draft.qty
    } else {
        0.0
    };
    if status == "pending" {
        accepted_qty = 0.0;
    }
    let mut note = extract_accord_decision_note(&draft.remarks);
    if draft.doc_status == 0 && unannounced_state == "pending" {
        note = "Werka siz qayd etmagan mahsulotni qabul qildi. Tasdiqlash kutilmoqda.".to_string();
    }
    if note.is_empty() && unannounced_state == "rejected" {
        note = "Supplier aytilmagan molni rad etdi.".to_string();
        let reason = extract_werka_unannounced_reason(&draft.remarks);
        if !reason.is_empty() {
            note.push_str("\nSabab: ");
            note.push_str(&reason);
        }
    }
    let event_type = if draft.doc_status == 0 && unannounced_state == "pending" {
        "werka_unannounced_pending"
    } else if status == "accepted" && unannounced_state == "approved" {
        if note.is_empty() {
            note = "Aytilmagan mol tasdiqlandi.".to_string();
        }
        "werka_unannounced_approved"
    } else {
        ""
    };
    let supplier_name = if draft.supplier_name.trim().is_empty() {
        fallback_supplier_name.trim().to_string()
    } else {
        draft.supplier_name.trim().to_string()
    };

    DispatchRecord {
        id: draft.name,
        record_type: "purchase_receipt".to_string(),
        supplier_ref: draft.supplier,
        supplier_name,
        item_code: draft.item_code,
        item_name: draft.item_name,
        uom: draft.uom,
        sent_qty,
        accepted_qty,
        amount: draft.amount,
        currency: draft.currency,
        note,
        event_type: event_type.to_string(),
        status: status.to_string(),
        created_label: draft.posting_date,
        ..DispatchRecord::default()
    }
}

pub(crate) fn format_notification_comment(
    label: &str,
    display_name: &str,
    message: &str,
) -> String {
    let name = display_name.trim();
    if name.is_empty() {
        format!("{}\n{}", label.trim(), message.trim())
    } else {
        format!("{} • {}\n{}", label.trim(), name, message.trim())
    }
}

pub(crate) fn assigned_codes_allow_item(assigned_item_codes: &[String], item_code: &str) -> bool {
    assigned_item_codes
        .iter()
        .any(|code| code.trim().eq_ignore_ascii_case(item_code.trim()))
}

pub(crate) fn item_supplier_permission_denied(error: &WerkaPortError) -> bool {
    match error {
        WerkaPortError::WriteFailed(message) | WerkaPortError::Database(message) => {
            let lower = message.to_lowercase();
            lower.contains("permissionerror") || lower.contains("status 403:")
        }
        _ => false,
    }
}

pub(crate) async fn supplier_admin_state(
    lookup: Option<&Arc<dyn WerkaSupplierAdminStateLookup>>,
    supplier_ref: &str,
) -> Result<WerkaSupplierAdminState, WerkaPortError> {
    let Some(lookup) = lookup else {
        return Ok(WerkaSupplierAdminState::default());
    };
    lookup.werka_supplier_admin_state(supplier_ref).await
}

pub(crate) async fn validate_unannounced_supplier_item(
    lookup: Option<&Arc<dyn WerkaHomeLookup>>,
    writer: &dyn WerkaUnannouncedWriter,
    supplier_ref: &str,
    item_code: &str,
    state: &WerkaSupplierAdminState,
) -> Result<(), WerkaPortError> {
    if let Some(lookup) = lookup
        && let Ok(items) = lookup.werka_supplier_items(supplier_ref, "", 200, 0).await
        && items
            .iter()
            .any(|item| item.code.trim().eq_ignore_ascii_case(item_code.trim()))
    {
        return Ok(());
    }

    match writer
        .validate_supplier_item_allowed(supplier_ref, item_code)
        .await
    {
        Ok(()) => Ok(()),
        Err(error)
            if item_supplier_permission_denied(&error)
                && assigned_codes_allow_item(&state.assigned_item_codes, item_code) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub(crate) async fn purchase_receipt_notification_detail(
    writer: &dyn SupplierUnannouncedWriter,
    supplier_ref: &str,
    supplier_display_name: &str,
    receipt_id: &str,
) -> Result<NotificationDetail, WerkaPortError> {
    let draft = writer.get_purchase_receipt(receipt_id.trim()).await?;
    if draft.supplier.trim() != supplier_ref.trim() {
        return Err(WerkaPortError::WriteFailed("unauthorized".to_string()));
    }
    let mut record = purchase_receipt_to_dispatch_record(draft.clone(), &draft.supplier_name);
    if draft.doc_status == 0 && extract_werka_unannounced_state(&draft.remarks) == "pending" {
        record.event_type = "werka_unannounced_pending".to_string();
        record.highlight = "Werka siz qayd etmagan mahsulotni qabul qildi".to_string();
    }
    if !supplier_display_name.trim().is_empty() {
        record.supplier_name = supplier_display_name.trim().to_string();
    }
    let comments = writer
        .list_purchase_receipt_comments(&draft.name, 100)
        .await?
        .into_iter()
        .filter_map(parse_notification_comment_record)
        .collect();
    Ok(NotificationDetail { record, comments })
}

pub(crate) fn parse_notification_comment_record(
    comment: PurchaseReceiptComment,
) -> Option<NotificationComment> {
    let (author_label, body) = parse_notification_comment(&comment.content)?;
    Some(NotificationComment {
        id: comment.id,
        author_label,
        body,
        created_label: comment.created_at,
    })
}

pub(crate) fn extract_werka_unannounced_state(remarks: &str) -> String {
    for line in remarks.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(WERKA_UNANNOUNCED_PREFIX) {
            return value.trim().to_lowercase();
        }
    }
    String::new()
}

pub(crate) fn extract_werka_unannounced_reason(remarks: &str) -> String {
    for line in remarks.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(WERKA_UNANNOUNCED_REASON_PREFIX) {
            return value.trim().to_string();
        }
    }
    String::new()
}

fn extract_accord_decision_note(remarks: &str) -> String {
    remarks
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            line.strip_prefix("Accord Qabul:")
                .map(|value| format!("Qabul: {}", value.trim()))
                .or_else(|| {
                    line.strip_prefix("Accord Qaytarildi:")
                        .map(|value| format!("Qaytarildi: {}", value.trim()))
                })
                .or_else(|| {
                    line.strip_prefix("Accord Sabab:")
                        .map(|value| format!("Sabab: {}", value.trim()))
                })
                .or_else(|| {
                    line.strip_prefix("Accord Izoh:")
                        .map(|value| format!("Izoh: {}", value.trim()))
                })
                .or_else(|| {
                    line.strip_prefix("Accord Supplier Tasdiq:")
                        .map(|value| format!("Supplier tasdiqladi: {}", value.trim()))
                })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_accord_decision_quantities(remarks: &str) -> (f64, f64) {
    let mut accepted_qty = 0.0;
    let mut returned_qty = 0.0;
    for line in remarks.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Accord Qabul:") {
            accepted_qty = first_float(value);
        } else if let Some(value) = trimmed.strip_prefix("Accord Qaytarildi:") {
            returned_qty = first_float(value);
        }
    }
    (accepted_qty, returned_qty)
}

fn first_float(value: &str) -> f64 {
    value
        .split_whitespace()
        .next()
        .and_then(|field| field.parse().ok())
        .unwrap_or(0.0)
}

fn parse_notification_comment(content: &str) -> Option<(String, String)> {
    let trimmed = sanitize_notification_comment(content);
    if trimmed.is_empty() {
        return None;
    }
    let lines = trimmed.lines().collect::<Vec<_>>();
    if lines.len() >= 2 {
        let head = lines[0].trim();
        let body = lines[1..].join("\n").trim().to_string();
        if !body.is_empty()
            && (head.starts_with("Supplier")
                || head.starts_with("Werka")
                || head.starts_with("Customer")
                || head.starts_with("Admin"))
        {
            return Some((head.to_string(), body));
        }
    }
    Some(("Tizim".to_string(), trimmed))
}

fn sanitize_notification_comment(content: &str) -> String {
    content
        .trim()
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_telegram_receipt_marker_qty(marker: &str) -> Option<f64> {
    let trimmed = marker.trim();
    if !trimmed.starts_with("TG:") {
        return None;
    }
    trimmed.split(':').next_back()?.trim().parse().ok()
}

fn dispatch_status_from_quantities(sent_qty: f64, accepted_qty: f64) -> &'static str {
    if accepted_qty <= 0.0 {
        "rejected"
    } else if sent_qty > 0.0 && accepted_qty < sent_qty {
        "partial"
    } else {
        "accepted"
    }
}
