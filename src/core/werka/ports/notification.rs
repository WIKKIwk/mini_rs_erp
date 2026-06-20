use async_trait::async_trait;

use super::error::WerkaPortError;
use super::purchase::{PurchaseReceiptComment, PurchaseReceiptDraft};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeliveryNoteNotificationDraft {
    pub name: String,
    pub customer: String,
    pub customer_name: String,
    pub doc_status: i32,
    pub modified: String,
    pub posting_date: String,
    pub qty: f64,
    pub returned_qty: f64,
    pub accord_customer_reason: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub accord_flow_state: i32,
    pub accord_customer_state: i32,
    pub remarks: String,
}

#[async_trait]
pub trait NotificationDetailWriter: Send + Sync {
    async fn get_notification_purchase_receipt(
        &self,
        name: &str,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError>;
    async fn list_notification_purchase_receipt_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError>;
    async fn get_notification_delivery_note(
        &self,
        name: &str,
    ) -> Result<DeliveryNoteNotificationDraft, WerkaPortError>;
    async fn list_notification_delivery_note_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError>;
    async fn add_notification_purchase_receipt_comment(
        &self,
        _name: &str,
        _content: &str,
    ) -> Result<(), WerkaPortError> {
        Err(WerkaPortError::WriteFailed(
            "purchase receipt comment writer unavailable".to_string(),
        ))
    }
    async fn update_notification_purchase_receipt_remarks(
        &self,
        _name: &str,
        _remarks: &str,
    ) -> Result<(), WerkaPortError> {
        Err(WerkaPortError::WriteFailed(
            "purchase receipt remarks writer unavailable".to_string(),
        ))
    }
    async fn add_notification_delivery_note_comment(
        &self,
        _name: &str,
        _content: &str,
    ) -> Result<(), WerkaPortError> {
        Err(WerkaPortError::WriteFailed(
            "delivery note comment writer unavailable".to_string(),
        ))
    }
}
