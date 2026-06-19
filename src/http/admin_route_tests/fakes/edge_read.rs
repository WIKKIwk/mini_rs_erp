use super::super::*;
use super::admin_read::FakeAdminReadPort;

pub(crate) struct AssignedItemsErrorReadPort {
    permission: bool,
}

impl AssignedItemsErrorReadPort {
    pub(crate) fn permission() -> Self {
        Self { permission: true }
    }

    pub(crate) fn lookup_failed() -> Self {
        Self { permission: false }
    }
}

#[async_trait]
impl AdminReadPort for AssignedItemsErrorReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        if self.permission {
            Err(AdminPortError::PermissionDenied)
        } else {
            Err(AdminPortError::LookupFailed)
        }
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

pub(crate) struct MissingItemsReadPort;

#[async_trait]
impl AdminReadPort for MissingItemsReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

pub(crate) struct ActivityLookup;

#[async_trait]
impl WerkaHomeLookup for ActivityLookup {
    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok((0..35)
            .map(|index| DispatchRecord {
                id: format!("REC-{index:03}"),
                supplier_name: "Supplier".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Rice".to_string(),
                uom: "Kg".to_string(),
                status: "confirmed".to_string(),
                created_label: "2026-02-08 12:00".to_string(),
                ..DispatchRecord::default()
            })
            .collect())
    }
}
