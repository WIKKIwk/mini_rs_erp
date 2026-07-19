use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DispatchRecord {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub record_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub supplier_ref: String,
    pub supplier_name: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub sent_qty: f64,
    pub accepted_qty: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub amount: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub currency: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub highlight: String,
    pub status: String,
    pub created_label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationComment {
    pub id: String,
    pub author_label: String,
    pub body: String,
    pub created_label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NotificationDetail {
    pub record: DispatchRecord,
    pub comments: Vec<NotificationComment>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationCommentCreateRequest {
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WerkaHomeSummary {
    pub pending_count: i64,
    pub confirmed_count: i64,
    pub returned_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierHomeSummary {
    pub pending_count: i64,
    pub submitted_count: i64,
    pub returned_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SupplierStatusBreakdownEntry {
    pub item_code: String,
    pub item_name: String,
    pub receipt_count: i64,
    pub total_sent_qty: f64,
    pub total_accepted_qty: f64,
    pub total_returned_qty: f64,
    pub uom: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaHomeData {
    pub summary: WerkaHomeSummary,
    pub pending_items: Vec<DispatchRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaStatusBreakdownEntry {
    pub supplier_ref: String,
    pub supplier_name: String,
    pub receipt_count: i64,
    pub total_sent_qty: f64,
    pub total_accepted_qty: f64,
    pub total_returned_qty: f64,
    pub uom: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchiveTotalByUom {
    pub uom: String,
    pub qty: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaArchiveSummary {
    pub record_count: usize,
    pub totals_by_uom: Vec<ArchiveTotalByUom>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaArchiveResponse {
    pub kind: String,
    pub period: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    pub summary: WerkaArchiveSummary,
    pub items: Vec<DispatchRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StockEntryBarcodeEntry {
    pub stock_entry_name: String,
    pub stock_entry_type: String,
    pub doc_status: i32,
    pub status: String,
    pub company: String,
    pub posting_date: String,
    pub posting_time: String,
    pub creation: String,
    pub modified: String,
    pub remarks: String,
    pub line_index: i32,
    pub item_code: String,
    pub item_name: String,
    pub qty: f64,
    pub uom: String,
    pub stock_uom: String,
    pub barcode: String,
    pub source_warehouse: String,
    pub target_warehouse: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StockEntryBarcodeLookup {
    pub barcode: String,
    pub count: usize,
    pub entries: Vec<StockEntryBarcodeEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierDirectoryEntry {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerDirectoryEntry {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplierItem {
    pub code: String,
    pub name: String,
    pub uom: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub warehouse: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_group: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerItemOption {
    pub customer_ref: String,
    pub customer_name: String,
    pub customer_phone: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub warehouse: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaCustomerIssueCreateRequest {
    pub customer_ref: String,
    pub item_code: String,
    pub qty: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_barcode: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_stock_entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_line_index: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WerkaCustomerIssueSource {
    pub barcode: String,
    pub stock_entry_name: String,
    pub line_index: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WerkaCustomerIssueCreateInput {
    pub customer_ref: String,
    pub item_code: String,
    pub qty: f64,
    pub source: WerkaCustomerIssueSource,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaCustomerIssueBatchCreateRequest {
    pub client_batch_id: String,
    #[serde(default)]
    pub lines: Vec<WerkaCustomerIssueCreateRequest>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaCustomerIssueRecord {
    pub entry_id: String,
    pub customer_ref: String,
    pub customer_name: String,
    pub item_code: String,
    pub item_name: String,
    pub uom: String,
    pub qty: f64,
    pub created_label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaCustomerIssueBatchLineResult {
    pub line_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<WerkaCustomerIssueRecord>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error_code: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaCustomerIssueBatchResult {
    pub client_batch_id: String,
    pub created: Vec<WerkaCustomerIssueBatchLineResult>,
    pub failed: Vec<WerkaCustomerIssueBatchLineResult>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WerkaUnannouncedCreateRequest {
    pub supplier_ref: String,
    pub item_code: String,
    pub qty: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SupplierUnannouncedResponseRequest {
    pub receipt_id: String,
    pub approve: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreateDispatchRequest {
    pub item_code: String,
    pub qty: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfirmReceiptRequest {
    pub receipt_id: String,
    pub accepted_qty: f64,
    pub returned_qty: f64,
    pub return_reason: String,
    pub return_comment: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WerkaAiSearchSuggestion {
    pub display_query: String,
    pub background_queries: Vec<String>,
    pub visible_text: String,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}
