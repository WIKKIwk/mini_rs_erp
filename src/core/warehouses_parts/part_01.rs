
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
    let WarehouseUpsert {
        warehouse,
        company,
        is_group,
        parent_warehouse,
    } = input;
    let warehouse = trim_owned(warehouse);
    if warehouse.is_empty() {
        return Err(WarehouseError::MissingWarehouse);
    }
    Ok(AdminWarehouse {
        warehouse,
        company: trim_owned(company),
        is_group,
        parent_warehouse: trim_owned(parent_warehouse),
    })
}

fn normalize_assignment(
    input: WarehouseAssignmentUpsert,
) -> Result<WarehouseAssignment, WarehouseError> {
    let WarehouseAssignmentUpsert {
        assignment_kind,
        warehouse,
        warehouse_name,
        apparatus_id,
        principal_role,
        principal_ref,
        display_name,
    } = input;
    let assignment_kind = normalize_assignment_kind(assignment_kind)?;
    let warehouse = trim_owned(warehouse);
    let warehouse_name = warehouse_name.map(trim_owned).filter(|value| !value.is_empty());
    let apparatus_id = apparatus_id
        .map(trim_owned)
        .filter(|value| !value.is_empty())
        .map(canonical_apparatus_id_owned)
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
    let principal_ref = trim_owned(principal_ref);
    if principal_ref.is_empty() {
        return Err(WarehouseError::MissingPrincipalRef);
    }
    Ok(WarehouseAssignment {
        assignment_kind,
        warehouse,
        warehouse_name,
        apparatus_id: apparatus_id.map(ApparatusId::into_string),
        principal_role,
        principal_ref,
        display_name: trim_owned(display_name),
    })
}

fn normalize_assignment_kind(value: String) -> Result<String, WarehouseError> {
    let mut value = trim_owned(value);
    if value.eq_ignore_ascii_case("warehouse") || value.eq_ignore_ascii_case("apparatus") {
        value.make_ascii_lowercase();
        Ok(value)
    } else {
        Err(WarehouseError::StoreFailed)
    }
}

fn canonical_apparatus_id_owned(value: String) -> Result<ApparatusId, WarehouseError> {
    ApparatusId::new(value).map_err(|_| WarehouseError::StoreFailed)
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
