use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::admin::models::AdminWarehouse;
use crate::core::auth::models::{Principal, PrincipalRole};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarehouseAssignment {
    pub warehouse: String,
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarehouseSummary {
    pub warehouse: String,
    pub product_count: usize,
    pub reserved_count: usize,
    pub assignment_count: usize,
    pub assigned_display_names: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WarehouseStockItem {
    pub code: String,
    pub name: String,
    pub uom: String,
    pub warehouse: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_group: String,
    pub on_hand_qty: f64,
    pub package_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WarehouseAssignmentUpsert {
    pub warehouse: String,
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WarehouseAssignmentDeleteRequest {
    pub warehouse: String,
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WarehouseDeleteRequest {
    pub warehouse: String,
    #[serde(default)]
    pub delete_products: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarehouseDeleteResult {
    pub warehouse: String,
    pub deleted_product_count: usize,
    pub deleted_assignment_count: usize,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WarehouseError {
    #[error("warehouse is required")]
    MissingWarehouse,
    #[error("principal ref is required")]
    MissingPrincipalRef,
    #[error("warehouse not found")]
    NotFound,
    #[error("warehouse assignment not found")]
    AssignmentNotFound,
    #[error("warehouse contains {0} products")]
    NotEmpty(usize),
    #[error("warehouse contains {0} active reservations")]
    HasActiveReservations(usize),
    #[error("warehouse contains child warehouses")]
    HasChildren,
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

    async fn warehouse_assignments(
        &self,
        warehouse: &str,
    ) -> Result<Vec<WarehouseAssignment>, WarehouseError>;

    async fn warehouse_summaries(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WarehouseSummary>, WarehouseError>;

    async fn warehouse_stock_items(
        &self,
        warehouse: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WarehouseStockItem>, WarehouseError>;

    async fn put_warehouse_assignment(
        &self,
        assignment: WarehouseAssignment,
    ) -> Result<WarehouseAssignment, WarehouseError>;

    async fn delete_warehouse_assignment(
        &self,
        warehouse: &str,
        principal_role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<Option<WarehouseAssignment>, WarehouseError>;

    async fn delete_warehouse(&self, warehouse: &str) -> Result<(), WarehouseError>;
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

    pub async fn warehouse_assignments(
        &self,
        warehouse: &str,
    ) -> Result<Vec<WarehouseAssignment>, WarehouseError> {
        self.store.warehouse_assignments(warehouse).await
    }

    pub async fn warehouse_assignments_for_principal(
        &self,
        principal: &Principal,
    ) -> Result<Vec<WarehouseAssignment>, WarehouseError> {
        Ok(self
            .store
            .warehouse_assignments("")
            .await?
            .into_iter()
            .filter(|assignment| assignment_matches_principal(assignment, principal))
            .collect())
    }

    pub async fn assigned_warehouse_names(
        &self,
        principal: &Principal,
    ) -> Result<Vec<String>, WarehouseError> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for assignment in self.warehouse_assignments_for_principal(principal).await? {
            let warehouse = assignment.warehouse.trim();
            if warehouse.is_empty() || !seen.insert(warehouse.to_lowercase()) {
                continue;
            }
            out.push(warehouse.to_string());
        }
        Ok(out)
    }

    pub async fn warehouse_summaries(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WarehouseSummary>, WarehouseError> {
        self.store.warehouse_summaries(query, limit).await
    }

    pub async fn warehouse_stock_items(
        &self,
        warehouse: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WarehouseStockItem>, WarehouseError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        self.store
            .warehouse_stock_items(warehouse, query.trim(), limit, offset)
            .await
    }

    pub async fn assign_warehouse(
        &self,
        input: WarehouseAssignmentUpsert,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        let assignment = normalize_assignment(input)?;
        self.store.put_warehouse_assignment(assignment).await
    }

    pub async fn unassign_warehouse(
        &self,
        input: WarehouseAssignmentDeleteRequest,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        let warehouse = input.warehouse.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        let principal_ref = input.principal_ref.trim();
        if principal_ref.is_empty() {
            return Err(WarehouseError::MissingPrincipalRef);
        }
        self.store
            .delete_warehouse_assignment(warehouse, &input.principal_role, principal_ref)
            .await?
            .ok_or(WarehouseError::AssignmentNotFound)
    }

    pub async fn delete_warehouse(
        &self,
        input: WarehouseDeleteRequest,
    ) -> Result<WarehouseDeleteResult, WarehouseError> {
        let warehouse = input.warehouse.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        if !self.store.warehouses("", warehouse, 1).await?.is_empty() {
            return Err(WarehouseError::HasChildren);
        }
        let summary = self
            .store
            .warehouse_summaries(warehouse, 500)
            .await?
            .into_iter()
            .find(|summary| summary.warehouse.trim().eq_ignore_ascii_case(warehouse))
            .ok_or(WarehouseError::NotFound)?;
        if summary.reserved_count > 0 {
            return Err(WarehouseError::HasActiveReservations(
                summary.reserved_count,
            ));
        }
        if summary.product_count > 0 && !input.delete_products {
            return Err(WarehouseError::NotEmpty(summary.product_count));
        }
        self.store.delete_warehouse(warehouse).await?;
        Ok(WarehouseDeleteResult {
            warehouse: summary.warehouse,
            deleted_product_count: summary.product_count,
            deleted_assignment_count: summary.assignment_count,
        })
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

fn normalize_assignment(
    input: WarehouseAssignmentUpsert,
) -> Result<WarehouseAssignment, WarehouseError> {
    let warehouse = input.warehouse.trim().to_string();
    if warehouse.is_empty() {
        return Err(WarehouseError::MissingWarehouse);
    }
    let principal_ref = input.principal_ref.trim().to_string();
    if principal_ref.is_empty() {
        return Err(WarehouseError::MissingPrincipalRef);
    }
    Ok(WarehouseAssignment {
        warehouse,
        principal_role: input.principal_role,
        principal_ref,
        display_name: input.display_name.trim().to_string(),
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
    assignments: RwLock<Vec<WarehouseAssignment>>,
    stock_items: RwLock<Vec<WarehouseStockItem>>,
    summary_counts: RwLock<BTreeMap<String, (usize, usize)>>,
}

impl MemoryWarehouseStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub async fn set_summary_counts(
        &self,
        warehouse: &str,
        product_count: usize,
        reserved_count: usize,
    ) {
        self.summary_counts.write().await.insert(
            warehouse.trim().to_lowercase(),
            (product_count, reserved_count),
        );
    }

    #[cfg(test)]
    pub async fn set_stock_items(&self, items: Vec<WarehouseStockItem>) {
        *self.stock_items.write().await = items;
    }
}

include!("warehouses_memory_store.rs");

fn assignment_key(assignment: &WarehouseAssignment) -> String {
    format!(
        "{}::{:?}::{}",
        assignment.warehouse.trim().to_lowercase(),
        assignment.principal_role,
        assignment.principal_ref.trim().to_lowercase()
    )
}

fn assignment_matches_principal(assignment: &WarehouseAssignment, principal: &Principal) -> bool {
    assignment.principal_role == principal.role
        && assignment
            .principal_ref
            .trim()
            .eq_ignore_ascii_case(principal.ref_.trim())
}
