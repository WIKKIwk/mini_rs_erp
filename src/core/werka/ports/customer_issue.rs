use async_trait::async_trait;

use crate::core::werka::models::WerkaCustomerIssueRecord;

use super::error::WerkaPortError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogItem {
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

#[async_trait]
pub trait WerkaCustomerIssueWriter: Send + Sync {
    async fn get_items_by_codes(
        &self,
        codes: &[String],
    ) -> Result<Vec<CatalogItem>, WerkaPortError>;
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

#[allow(dead_code)]
fn _customer_issue_record_contract(_record: WerkaCustomerIssueRecord) {}
