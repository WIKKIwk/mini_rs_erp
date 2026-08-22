
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

include!("../warehouses_memory_store.rs");

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
