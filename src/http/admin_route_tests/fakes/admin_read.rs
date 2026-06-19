use super::super::*;

pub(crate) struct FakeAdminReadPort;

pub(crate) struct QueryCaptureReadPort {
    pub(crate) seen_query: Arc<Mutex<String>>,
}

pub(crate) struct LocalPhoneDuplicateReadPort;

#[async_trait]
impl AdminReadPort for QueryCaptureReadPort {
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
        *self.seen_query.lock().await = query.to_string();
        Ok(vec![entry("CUST-QUERY", "Customer Query", "+998904444444")])
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

#[async_trait]
impl AdminReadPort for LocalPhoneDuplicateReadPort {
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
        if query == "110000011" {
            Ok(vec![entry("CUST-LOCAL", "Customer Local", "110000011")])
        } else {
            Ok(vec![])
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

#[async_trait]
impl AdminReadPort for FakeAdminReadPort {
    async fn suppliers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(vec![
            entry("SUP-001", "Supplier One", "+998901111111"),
            entry("SUP-002", "Supplier Two", "+998902222222"),
            entry("SUP-003", "Supplier Removed", "+998903333333"),
        ])
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry(ref_, "Supplier One", "+998901111111"))
    }

    async fn customers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(vec![entry("CUST-001", "Customer One", "+998904444444")])
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry(ref_, "Customer One", "+998904444444"))
    }

    async fn items_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001")])
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(item_codes.iter().map(|code| item(code)).collect())
    }

    async fn item_groups(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        Ok(vec![
            "All Item Groups".to_string(),
            "All Item Groups".to_string(),
        ])
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        _limit: usize,
    ) -> Result<Vec<crate::core::admin::models::AdminWarehouse>, AdminPortError> {
        let warehouses = vec![
            crate::core::admin::models::AdminWarehouse {
                warehouse: "Stores - CH".to_string(),
                company: "Company".to_string(),
                is_group: false,
                parent_warehouse: String::new(),
            },
            crate::core::admin::models::AdminWarehouse {
                warehouse: "Finished Goods - CH".to_string(),
                company: "Company".to_string(),
                is_group: false,
                parent_warehouse: String::new(),
            },
            crate::core::admin::models::AdminWarehouse {
                warehouse: "Godex aparat - CH".to_string(),
                company: "Company".to_string(),
                is_group: false,
                parent_warehouse: "aparat - A".to_string(),
            },
        ];
        let query = query.trim().to_lowercase();
        let parent = parent.trim().to_lowercase();
        Ok(warehouses
            .into_iter()
            .filter(|warehouse| {
                (query.is_empty() || warehouse.warehouse.to_lowercase().contains(&query))
                    && (parent.is_empty() || warehouse.parent_warehouse.to_lowercase() == parent)
            })
            .collect())
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        Ok(vec![
            AdminItemGroup {
                name: "All Item Groups".to_string(),
                item_group_name: "All Item Groups".to_string(),
                parent_item_group: String::new(),
                is_group: true,
            },
            AdminItemGroup {
                name: "Xomashyo".to_string(),
                item_group_name: "Xomashyo".to_string(),
                parent_item_group: "All Item Groups".to_string(),
                is_group: true,
            },
            AdminItemGroup {
                name: "plyonka".to_string(),
                item_group_name: "plyonka".to_string(),
                parent_item_group: "Xomashyo".to_string(),
                is_group: true,
            },
        ])
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001"), item("ITEM-002")])
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001")])
    }
}

#[async_trait]
impl SupplierLookup for FakeAdminReadPort {
    async fn search_suppliers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError> {
        let suppliers = self
            .suppliers_page("", limit.max(1), 0)
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;
        let query = query.trim().to_lowercase();
        Ok(suppliers
            .into_iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.ref_.to_lowercase().contains(&query)
                    || entry.name.to_lowercase().contains(&query)
                    || entry.phone.to_lowercase().contains(&query)
            })
            .map(|entry| SupplierRecord {
                id: entry.ref_,
                name: entry.name,
                phone: entry.phone,
            })
            .collect())
    }
}

#[async_trait]
impl CustomerLookup for FakeAdminReadPort {
    async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        let customers = self
            .customers_page("", limit.max(1), 0)
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;
        let query = query.trim().to_lowercase();
        Ok(customers
            .into_iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.ref_.to_lowercase().contains(&query)
                    || entry.name.to_lowercase().contains(&query)
                    || entry.phone.to_lowercase().contains(&query)
            })
            .map(|entry| CustomerRecord {
                id: entry.ref_,
                name: entry.name,
                phone: entry.phone,
            })
            .collect())
    }
}

pub(crate) struct CustomerItemsFailReadPort;

#[async_trait]
impl AdminReadPort for CustomerItemsFailReadPort {
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
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}
