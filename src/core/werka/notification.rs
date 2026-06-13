use crate::core::auth::models::PrincipalRole;
use crate::core::werka::models::{DispatchRecord, NotificationDetail};
use crate::core::werka::ports::{
    DeliveryNoteNotificationDraft, NotificationDetailWriter, WerkaPortError,
};
use crate::core::werka::service::WerkaService;
use crate::core::werka::unannounced::{
    extract_werka_unannounced_state, parse_notification_comment_record,
    purchase_receipt_to_dispatch_record,
};

const CUSTOMER_DELIVERY_RESULT_PREFIX: &str = "customer_delivery_result:";
const SUPPLIER_ACK_PREFIX: &str = "supplier_ack:";
const DELIVERY_FLOW_STATE_SUBMITTED: i32 = 1;
const CUSTOMER_STATE_REJECTED: i32 = 2;
const CUSTOMER_STATE_CONFIRMED: i32 = 3;
const CUSTOMER_STATE_PARTIAL: i32 = 4;

impl WerkaService {
    pub async fn notification_detail(
        &self,
        role: PrincipalRole,
        principal_ref: &str,
        principal_display_name: &str,
        receipt_id: &str,
    ) -> Result<Option<NotificationDetail>, WerkaPortError> {
        let target = resolve_notification_target(receipt_id)?;
        if let Some(lookup) = &self.notification_detail_lookup
            && let Ok(detail) = lookup.notification_detail_by_receipt_id(receipt_id).await
        {
            authorize_notification_detail(&role, principal_ref, &target, &detail)?;
            return Ok(Some(with_supplier_display_name(
                detail,
                &role,
                principal_display_name,
            )));
        }
        let Some(writer) = &self.notification_detail_writer else {
            return Ok(None);
        };
        let detail = match target.target_type {
            NotificationTargetType::DeliveryNote => {
                delivery_note_detail(writer.as_ref(), &target, receipt_id).await?
            }
            NotificationTargetType::PurchaseReceipt => {
                purchase_receipt_detail(writer.as_ref(), &target, receipt_id).await?
            }
        };
        authorize_notification_detail(&role, principal_ref, &target, &detail)?;
        Ok(Some(with_supplier_display_name(
            detail,
            &role,
            principal_display_name,
        )))
    }
}

async fn purchase_receipt_detail(
    writer: &dyn NotificationDetailWriter,
    target: &NotificationTarget,
    receipt_id: &str,
) -> Result<NotificationDetail, WerkaPortError> {
    let draft = writer
        .get_notification_purchase_receipt(&target.name)
        .await?;
    let mut record = purchase_receipt_to_dispatch_record(draft.clone(), &draft.supplier_name);
    if draft.doc_status == 0 && extract_werka_unannounced_state(&draft.remarks) == "pending" {
        record.event_type = "werka_unannounced_pending".to_string();
        record.highlight = "Werka siz qayd etmagan mahsulotni qabul qildi".to_string();
    }
    if target.event_type == "supplier_ack" {
        record.id = receipt_id.trim().to_string();
        record.event_type = "supplier_ack".to_string();
        record.highlight = "Supplier mahsulotni qaytarganingizni tasdiqladi".to_string();
    }
    let comments = writer
        .list_notification_purchase_receipt_comments(&draft.name, 100)
        .await?
        .into_iter()
        .filter_map(parse_notification_comment_record)
        .collect();
    Ok(NotificationDetail { record, comments })
}

async fn delivery_note_detail(
    writer: &dyn NotificationDetailWriter,
    target: &NotificationTarget,
    receipt_id: &str,
) -> Result<NotificationDetail, WerkaPortError> {
    let draft = writer.get_notification_delivery_note(&target.name).await?;
    let mut record = build_customer_delivery_result_event(draft.clone())
        .unwrap_or_else(|| delivery_note_to_dispatch_record(draft.clone()));
    if receipt_id.trim().is_empty() {
        record.id = draft.name.clone();
    } else {
        record.id = receipt_id.trim().to_string();
    }
    let comments = writer
        .list_notification_delivery_note_comments(&draft.name, 100)
        .await?
        .into_iter()
        .filter_map(parse_notification_comment_record)
        .collect();
    Ok(NotificationDetail { record, comments })
}

pub(crate) fn authorize_notification_detail(
    role: &PrincipalRole,
    principal_ref: &str,
    target: &NotificationTarget,
    detail: &NotificationDetail,
) -> Result<(), WerkaPortError> {
    match target.target_type {
        NotificationTargetType::DeliveryNote => {
            if *role == PrincipalRole::Customer
                && detail.record.supplier_ref.trim() != principal_ref.trim()
            {
                return Err(WerkaPortError::WriteFailed("unauthorized".to_string()));
            }
        }
        NotificationTargetType::PurchaseReceipt => {
            if *role == PrincipalRole::Customer
                || (*role == PrincipalRole::Supplier
                    && detail.record.supplier_ref.trim() != principal_ref.trim())
            {
                return Err(WerkaPortError::WriteFailed("unauthorized".to_string()));
            }
        }
    }
    Ok(())
}

pub(crate) fn with_supplier_display_name(
    mut detail: NotificationDetail,
    role: &PrincipalRole,
    principal_display_name: &str,
) -> NotificationDetail {
    if *role == PrincipalRole::Supplier && !principal_display_name.trim().is_empty() {
        detail.record.supplier_name = principal_display_name.trim().to_string();
    }
    detail
}

fn delivery_note_to_dispatch_record(item: DeliveryNoteNotificationDraft) -> DispatchRecord {
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

fn build_customer_delivery_result_event(
    item: DeliveryNoteNotificationDraft,
) -> Option<DispatchRecord> {
    let state = customer_delivery_status(&item);
    if state != "accepted" && state != "partial" && state != "rejected" {
        return None;
    }
    let mut record = delivery_note_to_dispatch_record(item);
    record.id = format!("{}{}", CUSTOMER_DELIVERY_RESULT_PREFIX, record.id.trim());
    match state {
        "accepted" => {
            record.event_type = "customer_delivery_confirmed".to_string();
            record.highlight = "Customer mahsulotni qabul qildi".to_string();
        }
        "partial" => {
            record.event_type = "customer_delivery_partial".to_string();
            record.highlight = "Customer mahsulotning bir qismini qaytardi".to_string();
        }
        _ => {
            record.event_type = "customer_delivery_rejected".to_string();
            record.highlight = "Customer mahsulotni rad etdi".to_string();
        }
    }
    Some(record)
}

fn customer_delivery_status(item: &DeliveryNoteNotificationDraft) -> &'static str {
    if item.doc_status != 1 {
        return "draft";
    }
    if item.accord_flow_state != DELIVERY_FLOW_STATE_SUBMITTED {
        return "pending";
    }
    match item.accord_customer_state {
        CUSTOMER_STATE_REJECTED => "rejected",
        CUSTOMER_STATE_CONFIRMED => "accepted",
        CUSTOMER_STATE_PARTIAL => "partial",
        _ => "pending",
    }
}

fn customer_decision_quantities(item: &DeliveryNoteNotificationDraft, status: &str) -> (f64, f64) {
    let mut returned_qty = if item.returned_qty > 0.0 {
        item.returned_qty
    } else {
        0.0
    };
    match status {
        "accepted" => (item.qty, 0.0),
        "partial" => {
            let accepted_qty = if returned_qty > 0.0 {
                (item.qty - returned_qty).max(0.0)
            } else {
                item.qty
            };
            if returned_qty <= 0.0 && accepted_qty > 0.0 {
                returned_qty = (item.qty - accepted_qty).max(0.0);
            }
            (accepted_qty, returned_qty)
        }
        "rejected" => (0.0, item.qty),
        _ => (0.0, returned_qty),
    }
}

pub(crate) fn resolve_notification_target(
    receipt_id: &str,
) -> Result<NotificationTarget, WerkaPortError> {
    let mut trimmed = receipt_id.trim();
    let mut event_type = "";
    if let Some(rest) = trimmed.strip_prefix(SUPPLIER_ACK_PREFIX) {
        event_type = "supplier_ack";
        trimmed = rest
            .split_once(':')
            .map(|(name, _)| name)
            .unwrap_or(rest)
            .trim();
    }
    if let Some(rest) = trimmed.strip_prefix(CUSTOMER_DELIVERY_RESULT_PREFIX) {
        let name = rest
            .split_once(':')
            .map(|(name, _)| name)
            .unwrap_or(rest)
            .trim();
        if name.is_empty() {
            return Err(WerkaPortError::WriteFailed(
                "delivery note id is required".to_string(),
            ));
        }
        return Ok(NotificationTarget {
            name: name.to_string(),
            target_type: NotificationTargetType::DeliveryNote,
            event_type: event_type.to_string(),
        });
    }
    if trimmed.is_empty() {
        return Err(WerkaPortError::WriteFailed(
            "receipt id is required".to_string(),
        ));
    }
    Ok(NotificationTarget {
        name: trimmed.to_string(),
        target_type: NotificationTargetType::PurchaseReceipt,
        event_type: event_type.to_string(),
    })
}

fn first_non_empty(first: &str, second: &str) -> String {
    if first.trim().is_empty() {
        second.trim().to_string()
    } else {
        first.trim().to_string()
    }
}

pub(crate) struct NotificationTarget {
    pub(crate) name: String,
    pub(crate) target_type: NotificationTargetType,
    pub(crate) event_type: String,
}

pub(crate) enum NotificationTargetType {
    PurchaseReceipt,
    DeliveryNote,
}
