use time::Date;

use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, StockEntryBarcodeLookup,
    SupplierDirectoryEntry, SupplierItem, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::WerkaPortError;
use crate::core::werka::service::WerkaService;

impl WerkaService {
    pub async fn home(
        &self,
        pending_limit: usize,
    ) -> Result<Option<WerkaHomeData>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_home(pending_limit).await.map(Some)
    }

    pub async fn summary(&self) -> Result<Option<WerkaHomeSummary>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_summary().await.map(Some)
    }

    pub async fn pending(
        &self,
        limit: usize,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_pending(limit).await.map(Some)
    }

    pub async fn history(&self) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_history().await.map(Some)
    }

    pub async fn status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Option<Vec<WerkaStatusBreakdownEntry>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_status_breakdown(kind).await.map(Some)
    }

    pub async fn status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_status_details(kind, supplier_ref)
            .await
            .map(Some)
    }

    pub async fn archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<Option<WerkaArchiveResponse>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_archive(kind, period, from, to).await.map(Some)
    }

    pub async fn suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<SupplierDirectoryEntry>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_suppliers(query, limit, offset).await.map(Some)
    }

    pub async fn customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<CustomerDirectoryEntry>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_customers(query, limit, offset).await.map(Some)
    }

    pub async fn supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<SupplierItem>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_supplier_items(supplier_ref, query, limit, offset)
            .await
            .map(Some)
    }

    pub async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<SupplierItem>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_customer_items(customer_ref, query, limit, offset)
            .await
            .map(Some)
    }

    pub async fn customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<CustomerItemOption>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_customer_item_options(query, limit, offset)
            .await
            .map(Some)
    }

    pub async fn stock_entry_lookup_by_barcode(
        &self,
        barcode: &str,
        limit: usize,
    ) -> Result<Option<StockEntryBarcodeLookup>, WerkaPortError> {
        let normalized = barcode.trim().to_uppercase();
        if normalized.is_empty() {
            return Err(WerkaPortError::InvalidInput);
        }
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        let entries = lookup.stock_entries_by_barcode(&normalized, limit).await?;
        Ok(Some(StockEntryBarcodeLookup {
            barcode: normalized,
            count: entries.len(),
            entries,
        }))
    }
}
