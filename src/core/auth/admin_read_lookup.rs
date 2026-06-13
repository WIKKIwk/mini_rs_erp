use std::sync::Arc;

use async_trait::async_trait;

use crate::core::admin::ports::AdminReadPort;
use crate::core::auth::ports::{
    AuthPortError, CustomerLookup, CustomerRecord, SupplierLookup, SupplierRecord,
};

/// Uses the same admin directory read path as customer/supplier admin screens.
pub struct AdminReadAuthLookup<R> {
    inner: Arc<R>,
}

impl<R> AdminReadAuthLookup<R> {
    pub fn new(inner: Arc<R>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<R> CustomerLookup for AdminReadAuthLookup<R>
where
    R: AdminReadPort + Send + Sync + 'static,
{
    async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        let entries = self
            .inner
            .customers_page(query, limit, 0)
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;
        Ok(entries
            .into_iter()
            .map(|entry| CustomerRecord {
                id: entry.ref_,
                name: entry.name,
                phone: entry.phone,
            })
            .collect())
    }
}

#[async_trait]
impl<R> SupplierLookup for AdminReadAuthLookup<R>
where
    R: AdminReadPort + Send + Sync + 'static,
{
    async fn search_suppliers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError> {
        let entries = self
            .inner
            .suppliers_page(query, limit, 0)
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;
        Ok(entries
            .into_iter()
            .map(|entry| SupplierRecord {
                id: entry.ref_,
                name: entry.name,
                phone: entry.phone,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminWarehouse};
    use crate::core::admin::ports::{AdminPortError, AdminReadPort};
    use crate::core::werka::models::SupplierItem;

    struct FakeAdminRead {
        customers: Vec<AdminDirectoryEntry>,
    }

    #[async_trait]
    impl AdminReadPort for FakeAdminRead {
        async fn suppliers_page(
            &self,
            _query: &str,
            _limit: usize,
            _offset: usize,
        ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn supplier_by_ref(
            &self,
            _ref_: &str,
        ) -> Result<AdminDirectoryEntry, AdminPortError> {
            Err(AdminPortError::NotFound)
        }

        async fn customers_page(
            &self,
            query: &str,
            limit: usize,
            _offset: usize,
        ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
            let query = query.trim();
            Ok(self
                .customers
                .iter()
                .filter(|entry| {
                    query.is_empty() || entry.ref_.contains(query) || entry.phone.contains(query)
                })
                .take(limit.max(1))
                .cloned()
                .collect())
        }

        async fn customer_by_ref(
            &self,
            _ref_: &str,
        ) -> Result<AdminDirectoryEntry, AdminPortError> {
            Err(AdminPortError::NotFound)
        }

        async fn items_page(
            &self,
            _query: &str,
            _limit: usize,
            _offset: usize,
        ) -> Result<Vec<SupplierItem>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn items_page_by_group(
            &self,
            _group: &str,
            _query: &str,
            _limit: usize,
            _offset: usize,
        ) -> Result<Vec<SupplierItem>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn items_by_codes(
            &self,
            _item_codes: &[String],
        ) -> Result<Vec<SupplierItem>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn item_groups(
            &self,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<String>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn warehouses(
            &self,
            _query: &str,
            _parent: &str,
            _limit: usize,
        ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn assigned_supplier_items(
            &self,
            _ref_: &str,
            _limit: usize,
        ) -> Result<Vec<SupplierItem>, AdminPortError> {
            Ok(Vec::new())
        }

        async fn customer_items(
            &self,
            _ref_: &str,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<SupplierItem>, AdminPortError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn admin_read_lookup_maps_customers_for_auth_login() {
        let lookup = AdminReadAuthLookup::new(Arc::new(FakeAdminRead {
            customers: vec![AdminDirectoryEntry {
                ref_: "aparatchi - 4".to_string(),
                name: "aparatchi".to_string(),
                phone: "110000011".to_string(),
            }],
        }));

        let customers = lookup
            .search_customers("110000011", 20)
            .await
            .expect("customers");
        assert_eq!(customers.len(), 1);
        assert_eq!(customers[0].id, "aparatchi - 4");
        assert_eq!(customers[0].phone, "110000011");
    }
}
