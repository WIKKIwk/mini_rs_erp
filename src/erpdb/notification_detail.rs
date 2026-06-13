use async_trait::async_trait;
use sqlx::query_as;

use crate::core::werka::models::{DispatchRecord, NotificationDetail};
use crate::core::werka::ports::{NotificationDetailLookup, PurchaseReceiptComment, WerkaPortError};
use crate::core::werka::unannounced::parse_notification_comment_record;
use crate::erpdb::reader::DirectDbReader;
use crate::erpdb::werka_home::{
    DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow, delivery_note_to_record, delivery_status,
    purchase_receipt_to_record,
};

const CUSTOMER_DELIVERY_RESULT_PREFIX: &str = "customer_delivery_result:";
const SUPPLIER_ACK_PREFIX: &str = "supplier_ack:";

#[async_trait]
impl NotificationDetailLookup for DirectDbReader {
    async fn notification_detail_by_receipt_id(
        &self,
        receipt_id: &str,
    ) -> Result<NotificationDetail, WerkaPortError> {
        let target = resolve_notification_target(receipt_id)?;
        match target.target_type {
            NotificationTargetType::DeliveryNote => {
                self.delivery_note_notification_detail(&target.name, receipt_id)
                    .await
            }
            NotificationTargetType::PurchaseReceipt => {
                self.purchase_receipt_notification_detail(&target, receipt_id)
                    .await
            }
        }
    }
}

impl DirectDbReader {
    async fn purchase_receipt_notification_detail(
        &self,
        target: &NotificationTarget,
        receipt_id: &str,
    ) -> Result<NotificationDetail, WerkaPortError> {
        let row = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_BY_NAME_SQL)
            .bind(target.name.trim())
            .fetch_one(&self.pool)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))?;
        let mut record = purchase_receipt_to_record(&row);
        if target.event_type == "supplier_ack" {
            record.id = receipt_id.trim().to_string();
            record.event_type = "supplier_ack".to_string();
            record.highlight = "Supplier mahsulotni qaytarganingizni tasdiqladi".to_string();
        }
        let comments = self
            .notification_comments("Purchase Receipt", &target.name, 100)
            .await?;
        Ok(NotificationDetail { record, comments })
    }

    async fn delivery_note_notification_detail(
        &self,
        name: &str,
        receipt_id: &str,
    ) -> Result<NotificationDetail, WerkaPortError> {
        let row = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_BY_NAME_SQL)
            .bind(name.trim())
            .fetch_one(&self.pool)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))?;
        let mut record =
            build_customer_result_dispatch(&row).unwrap_or_else(|| delivery_note_to_record(&row));
        if !receipt_id.trim().is_empty() {
            record.id = receipt_id.trim().to_string();
        }
        let comments = self
            .notification_comments("Delivery Note", name, 100)
            .await?;
        Ok(NotificationDetail { record, comments })
    }

    async fn notification_comments(
        &self,
        doctype: &str,
        name: &str,
        limit: usize,
    ) -> Result<Vec<crate::core::werka::models::NotificationComment>, WerkaPortError> {
        let limit = limit.min(200);
        let rows = query_as::<_, CommentRow>(COMMENT_ROWS_SQL)
            .bind(doctype.trim())
            .bind(name.trim())
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                parse_notification_comment_record(PurchaseReceiptComment {
                    id: row.name,
                    content: row.content,
                    created_at: row.creation,
                })
            })
            .collect())
    }
}

fn build_customer_result_dispatch(row: &DeliveryNoteSummaryRow) -> Option<DispatchRecord> {
    let status = delivery_status(row);
    if status != "accepted" && status != "partial" && status != "rejected" {
        return None;
    }
    let mut record = delivery_note_to_record(row);
    record.id = format!("{}{}", CUSTOMER_DELIVERY_RESULT_PREFIX, record.id.trim());
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

fn resolve_notification_target(receipt_id: &str) -> Result<NotificationTarget, WerkaPortError> {
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

#[derive(Debug, sqlx::FromRow)]
struct CommentRow {
    name: String,
    creation: String,
    content: String,
}

struct NotificationTarget {
    name: String,
    target_type: NotificationTargetType,
    event_type: String,
}

enum NotificationTargetType {
    PurchaseReceipt,
    DeliveryNote,
}

const PURCHASE_RECEIPT_BY_NAME_SQL: &str = r#"
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
    WHERE pr.name = ?
    LIMIT 1
"#;

const DELIVERY_NOTE_BY_NAME_SQL: &str = r#"
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
    WHERE dn.name = ?
    LIMIT 1
"#;

const COMMENT_ROWS_SQL: &str = r#"
    SELECT
        c.name AS name,
        COALESCE(CAST(c.creation AS CHAR), '') AS creation,
        COALESCE(c.content, '') AS content
    FROM `tabComment` c
    WHERE c.reference_doctype = ?
      AND c.reference_name = ?
    ORDER BY c.creation ASC, c.name ASC
    LIMIT ?
"#;
