use async_trait::async_trait;
use time::Date;

use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, NotificationDetail,
    StockEntryBarcodeEntry, SupplierDirectoryEntry, SupplierHomeSummary, SupplierItem,
    SupplierStatusBreakdownEntry, WerkaAiSearchSuggestion, WerkaArchiveResponse,
    WerkaCustomerIssueRecord, WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
};

#[async_trait]
pub trait WerkaHomeLookup: Send + Sync {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary::default())
    }
    async fn werka_home(&self, _pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        Ok(WerkaHomeData::default())
    }
    async fn werka_pending(&self, _limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_status_breakdown(
        &self,
        _kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_status_details(
        &self,
        _kind: &str,
        _supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_archive(
        &self,
        _kind: &str,
        _period: &str,
        _from: Option<Date>,
        _to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        Ok(WerkaArchiveResponse::default())
    }
    async fn werka_suppliers(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_customers(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_supplier_items(
        &self,
        _supplier_ref: &str,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_customer_item_options(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn stock_entries_by_barcode(
        &self,
        _barcode: &str,
        _limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, WerkaPortError> {
        Err(WerkaPortError::DirectDbLookupUnavailable)
    }
}
#[async_trait]
pub trait SupplierReadLookup: Send + Sync {
    async fn supplier_summary(
        &self,
        supplier_ref: &str,
    ) -> Result<SupplierHomeSummary, WerkaPortError>;
    async fn supplier_history(
        &self,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError>;
    async fn supplier_status_breakdown(
        &self,
        supplier_ref: &str,
        kind: &str,
    ) -> Result<Vec<SupplierStatusBreakdownEntry>, WerkaPortError>;
    async fn supplier_status_details(
        &self,
        supplier_ref: &str,
        kind: &str,
        item_code: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError>;
}
#[async_trait]
pub trait SupplierPurchaseReceiptLookup: Send + Sync {
    async fn list_supplier_purchase_receipts_page(
        &self,
        supplier_ref: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<PurchaseReceiptDraft>, WerkaPortError>;
    async fn list_supplier_purchase_receipt_comments_batch(
        &self,
        names: &[String],
        limit: usize,
    ) -> Result<std::collections::HashMap<String, Vec<PurchaseReceiptComment>>, WerkaPortError>;
}

#[async_trait]
pub trait SupplierItemLookup: Send + Sync {
    async fn list_assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError>;
    async fn get_supplier_items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, WerkaPortError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErpItem {
    pub code: String,
    pub name: String,
    pub uom: String,
    pub item_group: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreateDeliveryNoteInput {
    pub customer: String,
    pub company: String,
    pub warehouse: String,
    pub item_code: String,
    pub qty: f64,
    pub uom: String,
    pub source_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryNoteStateUpdate {
    pub flow_state: String,
    pub customer_state: String,
    pub customer_reason: String,
    pub delivery_actor: String,
    pub ui_status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryNoteDraft {
    pub name: String,
    pub remarks: String,
    pub accord_source_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WerkaSupplierRecord {
    pub id: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WerkaSupplierAdminState {
    pub blocked: bool,
    pub removed: bool,
    pub assigned_item_codes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreatePurchaseReceiptInput {
    pub supplier: String,
    pub supplier_phone: String,
    pub item_code: String,
    pub qty: f64,
    pub uom: String,
    pub warehouse: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PurchaseReceiptDraft {
    pub name: String,
    pub doc_status: i32,
    pub status: String,
    pub supplier: String,
    pub supplier_name: String,
    pub posting_date: String,
    pub supplier_delivery_note: String,
    pub item_code: String,
    pub item_name: String,
    pub qty: f64,
    pub uom: String,
    pub warehouse: String,
    pub amount: f64,
    pub currency: String,
    pub remarks: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PurchaseReceiptSubmissionResult {
    pub name: String,
    pub supplier: String,
    pub item_code: String,
    pub uom: String,
    pub sent_qty: f64,
    pub accepted_qty: f64,
    pub supplier_delivery_note: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurchaseReceiptComment {
    pub id: String,
    pub content: String,
    pub created_at: String,
}

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
pub trait WerkaCustomerIssueWriter: Send + Sync {
    async fn get_items_by_codes(&self, codes: &[String]) -> Result<Vec<ErpItem>, WerkaPortError>;
    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError>;
    async fn resolve_company(&self) -> Result<String, WerkaPortError>;
    async fn customer_issue_source_exists_by_scan(
        &self,
        customer_ref: &str,
        marker: &str,
    ) -> Result<bool, WerkaPortError>;
    async fn create_draft_delivery_note(
        &self,
        input: CreateDeliveryNoteInput,
    ) -> Result<String, WerkaPortError>;
    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), WerkaPortError>;
    async fn submit_delivery_note(&self, name: &str) -> Result<(), WerkaPortError>;
    async fn delete_delivery_note(&self, name: &str) -> Result<(), WerkaPortError>;
}

#[async_trait]
pub trait WerkaUnannouncedWriter: Send + Sync {
    async fn find_supplier_for_werka(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierRecord, WerkaPortError>;
    async fn validate_supplier_item_allowed(
        &self,
        supplier_ref: &str,
        item_code: &str,
    ) -> Result<(), WerkaPortError>;
    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError>;
    async fn create_draft_purchase_receipt(
        &self,
        input: CreatePurchaseReceiptInput,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError>;
    async fn update_purchase_receipt_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), WerkaPortError>;
    async fn add_purchase_receipt_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError>;
}

#[async_trait]
pub trait WerkaSupplierAdminStateLookup: Send + Sync {
    async fn werka_supplier_admin_state(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierAdminState, WerkaPortError>;
}

#[async_trait]
pub trait SupplierUnannouncedWriter: Send + Sync {
    async fn get_purchase_receipt(
        &self,
        name: &str,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError>;
    async fn update_purchase_receipt_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), WerkaPortError>;
    async fn confirm_and_submit_purchase_receipt(
        &self,
        name: &str,
        accepted_qty: f64,
        returned_qty: f64,
        return_reason: &str,
        return_comment: &str,
    ) -> Result<PurchaseReceiptSubmissionResult, WerkaPortError>;
    async fn add_purchase_receipt_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError>;
    async fn list_purchase_receipt_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError>;
}

#[async_trait]
pub trait WerkaConfirmWriter: Send + Sync {
    async fn confirm_and_submit_purchase_receipt(
        &self,
        name: &str,
        accepted_qty: f64,
        returned_qty: f64,
        return_reason: &str,
        return_comment: &str,
    ) -> Result<PurchaseReceiptSubmissionResult, WerkaPortError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WerkaAiSearchImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WerkaAiSearchError {
    pub code: &'static str,
    pub message: String,
}

impl WerkaAiSearchError {
    pub fn not_configured() -> Self {
        Self {
            code: "not_configured",
            message: "werka ai search is not configured".to_string(),
        }
    }

    pub fn invalid_image(message: &str) -> Self {
        Self {
            code: "invalid_image",
            message: message.to_string(),
        }
    }

    pub fn no_result() -> Self {
        Self {
            code: "no_result",
            message: "no ai search suggestion".to_string(),
        }
    }

    pub fn upstream(message: impl Into<String>) -> Self {
        Self {
            code: "upstream_failed",
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait WerkaAiSearch: Send + Sync {
    async fn infer_suggestion(
        &self,
        image: WerkaAiSearchImage,
    ) -> Result<WerkaAiSearchSuggestion, WerkaAiSearchError>;
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

#[async_trait]
pub trait NotificationDetailLookup: Send + Sync {
    async fn notification_detail_by_receipt_id(
        &self,
        receipt_id: &str,
    ) -> Result<NotificationDetail, WerkaPortError>;
}

#[async_trait]
pub trait CustomerIssueSourceLookup: Send + Sync {
    async fn customer_issue_source_exists(&self, marker: &str) -> Result<bool, WerkaPortError>;
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum WerkaPortError {
    #[error("lookup failed")]
    LookupFailed,
    #[error("database lookup failed: {0}")]
    Database(String),
    #[error("invalid input")]
    InvalidInput,
    #[error("not found")]
    NotFound,
    #[error("direct db lookup unavailable")]
    DirectDbLookupUnavailable,
    #[error("insufficient stock")]
    InsufficientStock,
    #[error("duplicate customer issue source")]
    DuplicateCustomerIssueSource,
    #[error("write failed: {0}")]
    WriteFailed(String),
}

#[allow(dead_code)]
fn _customer_issue_record_contract(_record: WerkaCustomerIssueRecord) {}
