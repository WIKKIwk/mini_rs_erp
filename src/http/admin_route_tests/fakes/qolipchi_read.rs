use super::super::*;
use super::admin_read::FakeAdminReadPort;

pub(crate) struct QolipchiCustomerLookupReadPort;

#[async_trait]
impl AdminReadPort for QolipchiCustomerLookupReadPort {
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
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        if query == "CUST-001" {
            Ok(vec![entry("CUST-001", "Jumaniyoz", "+998110000011")])
        } else {
            Ok(Vec::new())
        }
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
