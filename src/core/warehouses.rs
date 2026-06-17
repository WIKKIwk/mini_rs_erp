use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::admin::models::AdminWarehouse;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarehouseUpsert {
    #[serde(default, alias = "name")]
    pub warehouse: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub is_group: bool,
    #[serde(default)]
    pub parent_warehouse: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WarehouseError {
    #[error("warehouse is required")]
    MissingWarehouse,
    #[error("warehouse store failed")]
    StoreFailed,
}

#[async_trait]
pub trait WarehouseStorePort: Send + Sync {
    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, WarehouseError>;

    async fn put_warehouse(
        &self,
        warehouse: AdminWarehouse,
    ) -> Result<AdminWarehouse, WarehouseError>;
}

#[derive(Clone)]
pub struct WarehouseService {
    store: Arc<dyn WarehouseStorePort>,
}

impl WarehouseService {
    pub fn new(store: Arc<dyn WarehouseStorePort>) -> Self {
        Self { store }
    }

    pub async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, WarehouseError> {
        self.store.warehouses(query, parent, limit).await
    }

    pub async fn upsert_warehouse(
        &self,
        input: WarehouseUpsert,
    ) -> Result<AdminWarehouse, WarehouseError> {
        let warehouse = normalize_warehouse(input)?;
        self.store.put_warehouse(warehouse).await
    }
}

fn normalize_warehouse(input: WarehouseUpsert) -> Result<AdminWarehouse, WarehouseError> {
    let warehouse = input.warehouse.trim().to_string();
    if warehouse.is_empty() {
        return Err(WarehouseError::MissingWarehouse);
    }
    Ok(AdminWarehouse {
        warehouse,
        company: input.company.trim().to_string(),
        is_group: input.is_group,
        parent_warehouse: input.parent_warehouse.trim().to_string(),
    })
}

pub fn merge_admin_warehouses(
    mut first: Vec<AdminWarehouse>,
    second: Vec<AdminWarehouse>,
    limit: usize,
) -> Vec<AdminWarehouse> {
    let mut seen = first
        .iter()
        .map(|item| item.warehouse.to_lowercase())
        .collect::<BTreeSet<_>>();
    for warehouse in second {
        if seen.insert(warehouse.warehouse.to_lowercase()) {
            first.push(warehouse);
        }
        if first.len() >= limit {
            break;
        }
    }
    first.sort_by(|left, right| {
        left.warehouse
            .to_lowercase()
            .cmp(&right.warehouse.to_lowercase())
    });
    first.truncate(limit);
    first
}

#[derive(Default)]
pub struct MemoryWarehouseStore {
    warehouses: RwLock<Vec<AdminWarehouse>>,
}

impl MemoryWarehouseStore {
    pub fn new() -> Self {
        Self::default()
    }
}

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
}
