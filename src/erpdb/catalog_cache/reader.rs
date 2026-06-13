use std::sync::Arc;

use async_trait::async_trait;

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminWarehouse};
use crate::core::admin::ports::{AdminPortError, AdminReadPort};
use crate::core::profile::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileLookup, ProfilePortError, SupplierProfileRecord,
};
use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, StockEntryBarcodeEntry,
    SupplierDirectoryEntry, SupplierItem, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::erpdb::catalog_cache::store::{CatalogCacheError, CatalogCacheStore};
use crate::erpdb::reader::DirectDbReader;
use time::Date;

#[derive(Clone)]
pub struct CatalogCacheReader {
    store: Arc<CatalogCacheStore>,
    fallback: Option<Arc<DirectDbReader>>,
    default_warehouse: String,
}

impl CatalogCacheReader {
    pub fn new(store: Arc<CatalogCacheStore>, default_warehouse: impl Into<String>) -> Self {
        Self {
            store,
            fallback: None,
            default_warehouse: default_warehouse.into(),
        }
    }

    pub fn with_fallback(mut self, fallback: Arc<DirectDbReader>) -> Self {
        self.fallback = Some(fallback);
        self
    }
}

#[async_trait]
impl AdminReadPort for CatalogCacheReader {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        match self.store.suppliers_page(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .suppliers_page(query, limit, offset)
                    .await
            }
        }
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        match self.store.supplier_by_ref(ref_) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(AdminPortError::NotFound),
            Err(_) => self.fallback_admin()?.supplier_by_ref(ref_).await,
        }
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        match self.store.customers_page(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .customers_page(query, limit, offset)
                    .await
            }
        }
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        match self.store.customer_by_ref(ref_) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(AdminPortError::NotFound),
            Err(_) => self.fallback_admin()?.customer_by_ref(ref_).await,
        }
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .items_page(query, None, limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .items_page(query, limit, offset)
                    .await
            }
        }
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .items_page(query, Some(group), limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .items_page_by_group(group, query, limit, offset)
                    .await
            }
        }
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .items_by_codes(item_codes, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => self.fallback_admin()?.items_by_codes(item_codes).await,
        }
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        match self.store.item_groups(query, limit) {
            Ok(value) => Ok(value),
            Err(_) => self.fallback_admin()?.item_groups(query, limit).await,
        }
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        match self.store.item_group_tree() {
            Ok(value) => Ok(value),
            Err(_) => self.fallback_admin()?.item_group_tree().await,
        }
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        self.fallback_admin()?
            .warehouses(query, parent, limit)
            .await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .assigned_supplier_items(supplier_ref, limit, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .assigned_supplier_items(supplier_ref, limit)
                    .await
            }
        }
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .customer_items(customer_ref, query, limit, 0, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                <DirectDbReader as AdminReadPort>::customer_items(
                    self.fallback_admin()?,
                    customer_ref,
                    query,
                    limit,
                )
                .await
            }
        }
    }
}

#[async_trait]
impl WerkaHomeLookup for CatalogCacheReader {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        self.fallback_werka()?.werka_summary().await
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        self.fallback_werka()?.werka_home(pending_limit).await
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.fallback_werka()?.werka_pending(limit).await
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.fallback_werka()?.werka_history().await
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        self.fallback_werka()?.werka_status_breakdown(kind).await
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.fallback_werka()?
            .werka_status_details(kind, supplier_ref)
            .await
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        self.fallback_werka()?
            .werka_archive(kind, period, from, to)
            .await
    }

    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        match self.store.werka_suppliers(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_suppliers(query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        match self.store.werka_customers(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_customers(query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        match self
            .store
            .supplier_items(supplier_ref, query, limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_supplier_items(supplier_ref, query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        match self.store.werka_customer_items(
            customer_ref,
            query,
            limit,
            offset,
            &self.default_warehouse,
        ) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_customer_items(customer_ref, query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        match self
            .store
            .customer_item_options(query, limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_customer_item_options(query, limit, offset)
                    .await
            }
        }
    }

    async fn stock_entries_by_barcode(
        &self,
        barcode: &str,
        limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, WerkaPortError> {
        <DirectDbReader as WerkaHomeLookup>::stock_entries_by_barcode(
            self.fallback_werka()?,
            barcode,
            limit,
        )
        .await
    }
}

#[async_trait]
impl ProfileLookup for CatalogCacheReader {
    async fn get_supplier_profile(
        &self,
        id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError> {
        match self.store.supplier_profile(id) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(ProfilePortError::LookupFailed),
            Err(_) => self.fallback_profile()?.get_supplier_profile(id).await,
        }
    }

    async fn get_customer_profile(
        &self,
        id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError> {
        match self.store.customer_profile(id) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(ProfilePortError::LookupFailed),
            Err(_) => self.fallback_profile()?.get_customer_profile(id).await,
        }
    }

    async fn download_file(&self, file_url: &str) -> Result<DownloadedFile, ProfilePortError> {
        self.fallback_profile()?.download_file(file_url).await
    }

    async fn upload_supplier_image(
        &self,
        supplier_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<String, ProfilePortError> {
        self.fallback_profile()?
            .upload_supplier_image(supplier_id, filename, content_type, content)
            .await
    }
}

impl CatalogCacheReader {
    fn fallback_admin(&self) -> Result<&DirectDbReader, AdminPortError> {
        self.fallback.as_deref().ok_or(AdminPortError::LookupFailed)
    }

    fn fallback_werka(&self) -> Result<&DirectDbReader, WerkaPortError> {
        self.fallback
            .as_deref()
            .ok_or(WerkaPortError::DirectDbLookupUnavailable)
    }

    fn fallback_profile(&self) -> Result<&DirectDbReader, ProfilePortError> {
        self.fallback
            .as_deref()
            .ok_or(ProfilePortError::LookupFailed)
    }
}

impl From<CatalogCacheError> for AdminPortError {
    fn from(_value: CatalogCacheError) -> Self {
        AdminPortError::LookupFailed
    }
}

impl From<CatalogCacheError> for WerkaPortError {
    fn from(_value: CatalogCacheError) -> Self {
        WerkaPortError::DirectDbLookupUnavailable
    }
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
