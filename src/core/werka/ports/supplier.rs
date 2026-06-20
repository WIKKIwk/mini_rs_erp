use async_trait::async_trait;

use crate::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierItem, SupplierStatusBreakdownEntry,
};

use super::error::WerkaPortError;

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

#[async_trait]
pub trait WerkaSupplierAdminStateLookup: Send + Sync {
    async fn werka_supplier_admin_state(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierAdminState, WerkaPortError>;
}
