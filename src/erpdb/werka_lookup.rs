use async_trait::async_trait;
use time::Date;

use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, StockEntryBarcodeEntry,
    SupplierDirectoryEntry, SupplierItem, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::erpdb::reader::DirectDbReader;

#[async_trait]
impl WerkaHomeLookup for DirectDbReader {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        self.summary().await.map_err(database_error)
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        self.home(pending_limit).await.map_err(database_error)
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.pending(limit).await.map_err(database_error)
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.history().await.map_err(database_error)
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        self.status_breakdown(kind).await.map_err(database_error)
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.status_details(kind, supplier_ref)
            .await
            .map_err(database_error)
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        self.archive(kind, period, from, to)
            .await
            .map_err(database_error)
    }

    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        self.suppliers(query, limit, offset)
            .await
            .map_err(database_error)
    }

    async fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        self.customers(query, limit, offset)
            .await
            .map_err(database_error)
    }

    async fn werka_supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        self.supplier_items(supplier_ref, query, limit, offset)
            .await
            .map_err(database_error)
    }

    async fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        self.customer_items(customer_ref, query, limit, offset)
            .await
            .map_err(database_error)
    }

    async fn werka_customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        self.customer_item_options(query, limit, offset)
            .await
            .map_err(database_error)
    }

    async fn stock_entries_by_barcode(
        &self,
        barcode: &str,
        limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, WerkaPortError> {
        let entries = self
            .stock_entries_by_barcode(barcode, limit)
            .await
            .map_err(database_error)?;
        if entries.is_empty() {
            Err(WerkaPortError::NotFound)
        } else {
            Ok(entries)
        }
    }
}

pub(crate) fn database_error(error: sqlx::Error) -> WerkaPortError {
    WerkaPortError::Database(error.to_string())
}
