use async_trait::async_trait;

use crate::core::werka::ports::DeliveryNoteStateUpdate;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomerDeliveryNoteDraft {
    pub name: String,
    pub customer: String,
    pub customer_name: String,
    pub posting_date: String,
    pub modified: String,
    pub status: String,
    pub doc_status: i32,
    pub remarks: String,
    pub accord_flow_state: String,
    pub accord_customer_state: String,
    pub accord_customer_reason: String,
    pub accord_delivery_actor: String,
    pub accord_ui_status: String,
    pub accord_source_key: String,
    pub item_code: String,
    pub item_name: String,
    pub qty: f64,
    pub returned_qty: f64,
    pub uom: String,
}

#[async_trait]
pub trait CustomerDeliveryPort: Send + Sync {
    async fn list_customer_delivery_notes_page(
        &self,
        customer: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDeliveryNoteDraft>, CustomerPortError>;

    async fn get_delivery_note(
        &self,
        name: &str,
    ) -> Result<CustomerDeliveryNoteDraft, CustomerPortError>;

    async fn create_and_submit_delivery_note_return(
        &self,
        source_name: &str,
    ) -> Result<(), CustomerPortError>;

    async fn create_and_submit_partial_delivery_note_return(
        &self,
        source_name: &str,
        returned_qty: f64,
    ) -> Result<(), CustomerPortError>;

    async fn update_delivery_note_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), CustomerPortError>;

    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), CustomerPortError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CustomerPortError {
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CustomerServiceError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("invalid input")]
    InvalidInput,
    #[error("{0}")]
    Failed(String),
}

impl From<CustomerPortError> for CustomerServiceError {
    fn from(error: CustomerPortError) -> Self {
        Self::Failed(error.to_string())
    }
}
