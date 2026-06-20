use async_trait::async_trait;

use super::error::WerkaPortError;
use super::supplier::WerkaSupplierRecord;

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
