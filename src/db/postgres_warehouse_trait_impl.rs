#[async_trait]
impl WarehouseStorePort for PostgresWarehouseStore {
    async fn warehouse(&self, warehouse: &str) -> Result<Option<AdminWarehouse>, WarehouseError> {
        PostgresWarehouseStore::warehouse(self, warehouse).await
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, WarehouseError> {
        PostgresWarehouseStore::warehouses(self, query, parent, limit).await
    }

    async fn put_warehouse(
        &self,
        warehouse: AdminWarehouse,
    ) -> Result<AdminWarehouse, WarehouseError> {
        PostgresWarehouseStore::put_warehouse(self, warehouse).await
    }

    async fn warehouse_assignments(
        &self,
        warehouse: &str,
    ) -> Result<Vec<WarehouseAssignment>, WarehouseError> {
        PostgresWarehouseStore::warehouse_assignments(self, warehouse).await
    }

    async fn all_warehouse_assignments(&self) -> Result<Vec<WarehouseAssignment>, WarehouseError> {
        PostgresWarehouseStore::all_warehouse_assignments(self).await
    }

    async fn warehouse_summaries(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WarehouseSummary>, WarehouseError> {
        PostgresWarehouseStore::warehouse_summaries(self, query, limit).await
    }

    async fn warehouse_stock_items(
        &self,
        warehouse: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WarehouseStockItem>, WarehouseError> {
        PostgresWarehouseStore::warehouse_stock_items(self, warehouse, query, limit, offset).await
    }

    async fn put_warehouse_assignment(
        &self,
        assignment: WarehouseAssignment,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        PostgresWarehouseStore::put_warehouse_assignment(self, assignment).await
    }

    async fn delete_warehouse_assignment(
        &self,
        identity: &WarehouseAssignmentIdentity,
        principal_role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<Option<WarehouseAssignment>, WarehouseError> {
        PostgresWarehouseStore::delete_warehouse_assignment(self, identity, principal_role, principal_ref).await
    }

    async fn delete_warehouse(
        &self,
        warehouse: &str,
        delete_products: bool,
    ) -> Result<WarehouseDeleteResult, WarehouseError> {
        PostgresWarehouseStore::delete_warehouse(self, warehouse, delete_products).await
    }
}
