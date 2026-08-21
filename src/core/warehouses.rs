use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use crate::core::admin::models::AdminWarehouse;
use crate::core::apparatus_standard::ApparatusId;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::production_map::CanonicalApparatusResolver;

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
    #[serde(default = "default_assignment_kind")]
    pub assignment_kind: String,
    pub warehouse: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warehouse_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apparatus_id: Option<String>,
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
    #[serde(default = "default_assignment_kind")]
    pub assignment_kind: String,
    pub warehouse: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warehouse_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apparatus_id: Option<String>,
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WarehouseAssignmentDeleteRequest {
    #[serde(default = "default_assignment_kind")]
    pub assignment_kind: String,
    pub warehouse: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warehouse_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apparatus_id: Option<String>,
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarehouseAssignmentIdentity {
    WarehouseName(String),
    ApparatusId(ApparatusId),
}

fn default_assignment_kind() -> String {
    "warehouse".to_string()
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
    #[error("apparatus is invalid")]
    InvalidApparatus,
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
    async fn warehouse(&self, warehouse: &str) -> Result<Option<AdminWarehouse>, WarehouseError>;

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

    async fn all_warehouse_assignments(&self) -> Result<Vec<WarehouseAssignment>, WarehouseError>;

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
        identity: &WarehouseAssignmentIdentity,
        principal_role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<Option<WarehouseAssignment>, WarehouseError>;

    async fn delete_warehouse(
        &self,
        warehouse: &str,
        delete_products: bool,
    ) -> Result<WarehouseDeleteResult, WarehouseError>;
}

#[derive(Clone)]
pub struct WarehouseService {
    store: Arc<dyn WarehouseStorePort>,
    canonical_apparatus_resolver: Arc<dyn CanonicalApparatusResolver>,
}

impl WarehouseService {
    pub fn new(
        store: Arc<dyn WarehouseStorePort>,
        canonical_apparatus_resolver: Arc<dyn CanonicalApparatusResolver>,
    ) -> Self {
        Self {
            store,
            canonical_apparatus_resolver,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(store: Arc<dyn WarehouseStorePort>) -> Self {
        Self::new(
            store,
            Arc::new(crate::core::production_map::TestCanonicalApparatusResolver::standard()),
        )
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
            .all_warehouse_assignments()
            .await?
            .into_iter()
            .filter(|assignment| assignment_matches_principal(assignment, principal))
            .collect())
    }

    pub async fn assigned_warehouse_keys(
        &self,
        principal: &Principal,
    ) -> Result<Vec<String>, WarehouseError> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for assignment in self.warehouse_assignments_for_principal(principal).await? {
            let key = assignment_identity_key(&assignment);
            if key.is_empty() || !seen.insert(key.to_lowercase()) {
                continue;
            }
            out.push(key);
        }
        Ok(out)
    }

    pub async fn assigned_warehouse_names(
        &self,
        principal: &Principal,
    ) -> Result<Vec<String>, WarehouseError> {
        self.assigned_warehouse_keys(principal).await
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
        if assignment.assignment_kind == "apparatus" {
            let apparatus_id = assignment
                .apparatus_id
                .as_deref()
                .ok_or(WarehouseError::InvalidApparatus)
                .and_then(canonical_apparatus_id)?;
            let Some(canonical) = self
                .canonical_apparatus_resolver
                .resolve(&apparatus_id)
                .await
                .map_err(|_| WarehouseError::StoreFailed)?
            else {
                return Err(WarehouseError::InvalidApparatus);
            };
            if canonical.runtime.apparatus_id != apparatus_id
                || !canonical.has_coherent_source()
                || !canonical.is_active()
            {
                return Err(WarehouseError::InvalidApparatus);
            }
        }
        self.store.put_warehouse_assignment(assignment).await
    }

    pub async fn unassign_warehouse(
        &self,
        input: WarehouseAssignmentDeleteRequest,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        let identity = normalize_assignment_delete_key(&input)?;
        let principal_ref = input.principal_ref.trim();
        if principal_ref.is_empty() {
            return Err(WarehouseError::MissingPrincipalRef);
        }
        self.store
            .delete_warehouse_assignment(&identity, &input.principal_role, principal_ref)
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
        self.store
            .delete_warehouse(warehouse, input.delete_products)
            .await
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
    let assignment_kind = normalize_assignment_kind(&input.assignment_kind)?;
    let warehouse = input.warehouse.trim().to_string();
    let warehouse_name = input
        .warehouse_name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let apparatus_id = input
        .apparatus_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(canonical_apparatus_id)
        .transpose()?;
    let (warehouse_name, apparatus_id) = match assignment_kind.as_str() {
        "warehouse" => {
            let name =
                warehouse_name.or_else(|| (!warehouse.is_empty()).then(|| warehouse.clone()));
            if name.is_none() {
                return Err(WarehouseError::MissingWarehouse);
            }
            (name, None)
        }
        "apparatus" => {
            if apparatus_id.is_none() {
                return Err(WarehouseError::MissingWarehouse);
            }
            (None, apparatus_id)
        }
        _ => unreachable!(),
    };
    let principal_ref = input.principal_ref.trim().to_string();
    if principal_ref.is_empty() {
        return Err(WarehouseError::MissingPrincipalRef);
    }
    Ok(WarehouseAssignment {
        assignment_kind,
        warehouse,
        warehouse_name,
        apparatus_id: apparatus_id.map(|id| id.as_str().to_string()),
        principal_role: input.principal_role,
        principal_ref,
        display_name: input.display_name.trim().to_string(),
    })
}

fn normalize_assignment_kind(value: &str) -> Result<String, WarehouseError> {
    match value.trim().to_lowercase().as_str() {
        "warehouse" | "apparatus" => Ok(value.trim().to_lowercase()),
        _ => Err(WarehouseError::StoreFailed),
    }
}

fn canonical_apparatus_id(value: &str) -> Result<ApparatusId, WarehouseError> {
    ApparatusId::new(value.to_string()).map_err(|_| WarehouseError::StoreFailed)
}

fn assignment_identity_key(assignment: &WarehouseAssignment) -> String {
    if assignment.assignment_kind.eq_ignore_ascii_case("apparatus") {
        assignment
            .apparatus_id
            .as_deref()
            .and_then(|value| canonical_apparatus_id(value).ok())
            .map(|id| id.as_str().to_string())
            .unwrap_or_default()
    } else {
        assignment
            .warehouse_name
            .as_deref()
            .unwrap_or(&assignment.warehouse)
            .trim()
            .to_string()
    }
}

fn assignment_matches_identity(
    assignment: &WarehouseAssignment,
    identity: &WarehouseAssignmentIdentity,
) -> bool {
    match identity {
        WarehouseAssignmentIdentity::WarehouseName(warehouse) => {
            assignment.assignment_kind.eq_ignore_ascii_case("warehouse")
                && assignment_identity_key(assignment).eq_ignore_ascii_case(warehouse)
        }
        WarehouseAssignmentIdentity::ApparatusId(apparatus_id) => {
            assignment.assignment_kind.eq_ignore_ascii_case("apparatus")
                && assignment
                    .apparatus_id
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(apparatus_id.as_str()))
        }
    }
}

fn normalize_assignment_delete_key(
    input: &WarehouseAssignmentDeleteRequest,
) -> Result<WarehouseAssignmentIdentity, WarehouseError> {
    match normalize_assignment_kind(&input.assignment_kind)?.as_str() {
        "apparatus" => input
            .apparatus_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(canonical_apparatus_id)
            .transpose()?
            .map(WarehouseAssignmentIdentity::ApparatusId)
            .ok_or(WarehouseError::MissingWarehouse),
        _ => {
            let warehouse = input
                .warehouse_name
                .as_deref()
                .unwrap_or(&input.warehouse)
                .trim();
            if warehouse.is_empty() {
                Err(WarehouseError::MissingWarehouse)
            } else {
                Ok(WarehouseAssignmentIdentity::WarehouseName(
                    warehouse.to_string(),
                ))
            }
        }
    }
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
    mutation_lock: Mutex<()>,
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delete_warehouse_allows_an_empty_child_warehouse() {
        let store = Arc::new(MemoryWarehouseStore::new());
        let service = WarehouseService::new_for_test(store);
        service
            .upsert_warehouse(WarehouseUpsert {
                warehouse: "Qolip ombori".to_string(),
                ..WarehouseUpsert::default()
            })
            .await
            .expect("parent warehouse should be created");
        service
            .upsert_warehouse(WarehouseUpsert {
                warehouse: "A blok".to_string(),
                parent_warehouse: "Qolip ombori".to_string(),
                ..WarehouseUpsert::default()
            })
            .await
            .expect("child warehouse should be created");

        let deleted = service
            .delete_warehouse(WarehouseDeleteRequest {
                warehouse: "A blok".to_string(),
                delete_products: false,
            })
            .await
            .expect("empty child warehouse should be deleted");

        assert_eq!(deleted.warehouse, "A blok");
        assert!(
            service
                .warehouses("A blok", "", 10)
                .await
                .expect("warehouses should load")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn typed_assignments_keep_memory_reads_and_deletes_disjoint() {
        let store = Arc::new(MemoryWarehouseStore::new());
        let service = WarehouseService::new_for_test(store.clone());
        let principal = Principal {
            role: PrincipalRole::Admin,
            display_name: "Typed principal".to_string(),
            legal_name: String::new(),
            ref_: "typed-principal".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        };

        store
            .put_warehouse_assignment(WarehouseAssignment {
                assignment_kind: "warehouse".to_string(),
                warehouse: "Shared warehouse".to_string(),
                warehouse_name: Some("Shared warehouse".to_string()),
                apparatus_id: None,
                principal_role: principal.role.clone(),
                principal_ref: principal.ref_.clone(),
                display_name: "Warehouse assignment".to_string(),
            })
            .await
            .expect("warehouse assignment");
        store
            .put_warehouse_assignment(WarehouseAssignment {
                assignment_kind: "apparatus".to_string(),
                warehouse: "Apparatus snapshot".to_string(),
                warehouse_name: None,
                apparatus_id: Some("apparatus:test:one".to_string()),
                principal_role: principal.role.clone(),
                principal_ref: principal.ref_.clone(),
                display_name: "Apparatus assignment".to_string(),
            })
            .await
            .expect("apparatus assignment");

        assert_eq!(service.warehouse_assignments("").await.unwrap().len(), 1);
        assert_eq!(
            service
                .warehouse_assignments_for_principal(&principal)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            service
                .assigned_warehouse_keys(&principal)
                .await
                .unwrap()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "Shared warehouse".to_string(),
                "apparatus:test:one".to_string(),
            ])
        );
        assert_eq!(
            service
                .assigned_warehouse_names(&principal)
                .await
                .unwrap()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "Shared warehouse".to_string(),
                "apparatus:test:one".to_string(),
            ])
        );

        let removed_apparatus = service
            .unassign_warehouse(WarehouseAssignmentDeleteRequest {
                assignment_kind: "apparatus".to_string(),
                warehouse: "Apparatus snapshot".to_string(),
                warehouse_name: None,
                apparatus_id: Some("apparatus:test:one".to_string()),
                principal_role: principal.role.clone(),
                principal_ref: principal.ref_.clone(),
            })
            .await
            .expect("delete apparatus assignment");
        assert_eq!(removed_apparatus.assignment_kind, "apparatus");
        assert_eq!(service.warehouse_assignments("").await.unwrap().len(), 1);

        let removed_warehouse = service
            .unassign_warehouse(WarehouseAssignmentDeleteRequest {
                assignment_kind: "warehouse".to_string(),
                warehouse: "Shared warehouse".to_string(),
                warehouse_name: Some("Shared warehouse".to_string()),
                apparatus_id: None,
                principal_role: principal.role.clone(),
                principal_ref: principal.ref_.clone(),
            })
            .await
            .expect("delete warehouse assignment");
        assert_eq!(removed_warehouse.assignment_kind, "warehouse");
        assert!(
            service
                .warehouse_assignments_for_principal(&principal)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

fn assignment_key(assignment: &WarehouseAssignment) -> String {
    format!(
        "{}::{}::{:?}::{}",
        assignment.assignment_kind.trim().to_lowercase(),
        assignment_identity_key(assignment).to_lowercase(),
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
