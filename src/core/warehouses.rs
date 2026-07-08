use std::collections::BTreeSet;
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WarehouseAssignmentUpsert {
    pub warehouse: String,
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WarehouseError {
    #[error("warehouse is required")]
    MissingWarehouse,
    #[error("principal ref is required")]
    MissingPrincipalRef,
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

    async fn put_warehouse_assignment(
        &self,
        assignment: WarehouseAssignment,
    ) -> Result<WarehouseAssignment, WarehouseError>;
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

    pub async fn assign_warehouse(
        &self,
        input: WarehouseAssignmentUpsert,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        let assignment = normalize_assignment(input)?;
        self.store.put_warehouse_assignment(assignment).await
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

    async fn warehouse_summaries(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WarehouseSummary>, WarehouseError> {
        let query = query.trim().to_lowercase();
        let warehouses = self.warehouses.read().await.clone();
        let assignments = self.assignments.read().await.clone();
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
                WarehouseSummary {
                    warehouse: warehouse.warehouse,
                    product_count: 0,
                    reserved_count: 0,
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
