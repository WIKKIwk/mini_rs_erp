#[async_trait]
impl WarehouseStorePort for MemoryWarehouseStore {
    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, WarehouseError> {
        let query = query.trim().to_lowercase();
        let parent = parent.trim().to_lowercase();
        Ok(self
            .warehouses
            .read()
            .await
            .iter()
            .filter(|warehouse| {
                (query.is_empty() || warehouse.warehouse.to_lowercase().contains(&query))
                    && (parent.is_empty() || warehouse.parent_warehouse.to_lowercase() == parent)
            })
            .take(limit.max(1))
            .cloned()
            .collect())
    }

    async fn put_warehouse(
        &self,
        warehouse: AdminWarehouse,
    ) -> Result<AdminWarehouse, WarehouseError> {
        let mut warehouses = self.warehouses.write().await;
        let key = warehouse.warehouse.to_lowercase();
        if let Some(index) = warehouses
            .iter()
            .position(|item| item.warehouse.to_lowercase() == key)
        {
            warehouses[index] = warehouse.clone();
        } else {
            warehouses.push(warehouse.clone());
        }
        warehouses.sort_by(|left, right| {
            left.warehouse
                .to_lowercase()
                .cmp(&right.warehouse.to_lowercase())
        });
        Ok(warehouse)
    }

    async fn warehouse_assignments(
        &self,
        warehouse: &str,
    ) -> Result<Vec<WarehouseAssignment>, WarehouseError> {
        let warehouse = warehouse.trim().to_lowercase();
        Ok(self
            .assignments
            .read()
            .await
            .iter()
            .filter(|item| warehouse.is_empty() || item.warehouse.to_lowercase() == warehouse)
            .cloned()
            .collect())
    }

    async fn warehouse_stock_items(
        &self,
        warehouse: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WarehouseStockItem>, WarehouseError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let warehouse = warehouse.trim().to_lowercase();
        let query = query.trim().to_lowercase();
        let mut items = self
            .stock_items
            .read()
            .await
            .iter()
            .filter(|item| {
                item.on_hand_qty > 0.0
                    && item.warehouse.trim().to_lowercase() == warehouse
                    && (query.is_empty()
                        || item.code.to_lowercase().contains(&query)
                        || item.name.to_lowercase().contains(&query)
                        || item.item_group.to_lowercase().contains(&query))
            })
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.code.to_lowercase().cmp(&right.code.to_lowercase()))
        });
        Ok(items.into_iter().skip(offset).take(limit).collect())
    }

    async fn put_warehouse_assignment(
        &self,
        assignment: WarehouseAssignment,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        let mut assignments = self.assignments.write().await;
        let key = assignment_key(&assignment);
        if let Some(index) = assignments
            .iter()
            .position(|item| assignment_key(item) == key)
        {
            assignments[index] = assignment.clone();
        } else {
            assignments.push(assignment.clone());
        }
        assignments.sort_by(|left, right| {
            left.warehouse
                .to_lowercase()
                .cmp(&right.warehouse.to_lowercase())
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        Ok(assignment)
    }

    async fn delete_warehouse_assignment(
        &self,
        warehouse: &str,
        principal_role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<Option<WarehouseAssignment>, WarehouseError> {
        let mut assignments = self.assignments.write().await;
        let index = assignments.iter().position(|assignment| {
            assignment
                .warehouse
                .trim()
                .eq_ignore_ascii_case(warehouse.trim())
                && assignment.principal_role == *principal_role
                && assignment
                    .principal_ref
                    .trim()
                    .eq_ignore_ascii_case(principal_ref.trim())
        });
        Ok(index.map(|index| assignments.remove(index)))
    }

    async fn delete_warehouse(&self, warehouse: &str) -> Result<(), WarehouseError> {
        let key = warehouse.trim().to_lowercase();
        self.warehouses
            .write()
            .await
            .retain(|item| item.warehouse.trim().to_lowercase() != key);
        self.assignments
            .write()
            .await
            .retain(|item| item.warehouse.trim().to_lowercase() != key);
        self.stock_items
            .write()
            .await
            .retain(|item| item.warehouse.trim().to_lowercase() != key);
        self.summary_counts.write().await.remove(&key);
        Ok(())
    }

    async fn warehouse_summaries(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WarehouseSummary>, WarehouseError> {
        let query = query.trim().to_lowercase();
        let warehouses = self.warehouses.read().await.clone();
        let assignments = self.assignments.read().await.clone();
        let stock_items = self.stock_items.read().await.clone();
        let summary_counts = self.summary_counts.read().await.clone();
        let mut summaries = warehouses
            .into_iter()
            .filter(|warehouse| {
                warehouse.parent_warehouse.trim().is_empty()
                    && (query.is_empty() || warehouse.warehouse.to_lowercase().contains(&query))
            })
            .map(|warehouse| {
                let assigned = assignments
                    .iter()
                    .filter(|item| item.warehouse.eq_ignore_ascii_case(&warehouse.warehouse))
                    .collect::<Vec<_>>();
                let stock_product_count = stock_items
                    .iter()
                    .filter(|item| {
                        item.on_hand_qty > 0.0
                            && item
                                .warehouse
                                .trim()
                                .eq_ignore_ascii_case(&warehouse.warehouse)
                    })
                    .count();
                let (product_count, reserved_count) = summary_counts
                    .get(&warehouse.warehouse.trim().to_lowercase())
                    .copied()
                    .unwrap_or((stock_product_count, 0));
                WarehouseSummary {
                    warehouse: warehouse.warehouse,
                    product_count,
                    reserved_count,
                    assignment_count: assigned.len(),
                    assigned_display_names: assigned
                        .into_iter()
                        .map(|item| {
                            if item.display_name.trim().is_empty() {
                                item.principal_ref.clone()
                            } else {
                                item.display_name.clone()
                            }
                        })
                        .collect(),
                }
            })
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| {
            left.warehouse
                .to_lowercase()
                .cmp(&right.warehouse.to_lowercase())
        });
        summaries.truncate(limit.max(1));
        Ok(summaries)
    }
}

