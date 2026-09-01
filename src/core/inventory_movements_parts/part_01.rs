
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryAssetKind {
    RawMaterial,
    FinishedGoods,
    Qolip,
}

impl InventoryAssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RawMaterial => "raw_material",
            Self::FinishedGoods => "finished_goods",
            Self::Qolip => "qolip",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, InventoryMovementError> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("raw_material") {
            Ok(Self::RawMaterial)
        } else if raw.eq_ignore_ascii_case("finished_goods") {
            Ok(Self::FinishedGoods)
        } else if raw.eq_ignore_ascii_case("qolip") {
            Ok(Self::Qolip)
        } else {
            Err(InventoryMovementError::InvalidAssetKind)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryLocationKind {
    Warehouse,
    State,
    Transit,
}

impl InventoryLocationKind {
    pub fn parse(raw: &str) -> Result<Self, InventoryMovementError> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("warehouse") {
            Ok(Self::Warehouse)
        } else if raw.eq_ignore_ascii_case("state") {
            Ok(Self::State)
        } else if raw.eq_ignore_ascii_case("transit") {
            Ok(Self::Transit)
        } else {
            Err(InventoryMovementError::InvalidLocation)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryTransferStatus {
    Requested,
    Approved,
    InTransit,
    Received,
    Rejected,
    Cancelled,
}

impl InventoryTransferStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Approved => "approved",
            Self::InTransit => "in_transit",
            Self::Received => "received",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, InventoryMovementError> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("requested") {
            Ok(Self::Requested)
        } else if raw.eq_ignore_ascii_case("approved") {
            Ok(Self::Approved)
        } else if raw.eq_ignore_ascii_case("in_transit") {
            Ok(Self::InTransit)
        } else if raw.eq_ignore_ascii_case("received") {
            Ok(Self::Received)
        } else if raw.eq_ignore_ascii_case("rejected") {
            Ok(Self::Rejected)
        } else if raw.eq_ignore_ascii_case("cancelled") {
            Ok(Self::Cancelled)
        } else {
            Err(InventoryMovementError::InvalidTransferStatus)
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Requested | Self::Approved | Self::InTransit)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryLocationApparatus {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryLocation {
    pub id: String,
    pub kind: InventoryLocationKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub warehouse_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub factory_location_id: String,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apparatus: Vec<InventoryLocationApparatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryLocationRef {
    pub id: String,
    pub kind: InventoryLocationKind,
    pub name: String,
}

impl From<&InventoryLocation> for InventoryLocationRef {
    fn from(location: &InventoryLocation) -> Self {
        Self {
            id: location.id.clone(),
            kind: location.kind,
            name: location.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InventoryAsset {
    pub kind: InventoryAssetKind,
    pub asset_ref: String,
    pub custody_warehouse_id: String,
    pub custody_warehouse: String,
    pub item_code: String,
    pub item_name: String,
    pub identifier: String,
    pub qty: f64,
    pub uom: String,
    pub status: String,
    pub physical_location: InventoryLocationRef,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transfer_id: String,
    pub placement_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMaterialStatePlacement {
    pub barcode: String,
    pub location_id: String,
    pub location_name: String,
    /// Canonical apparatus IDs used by runtime placement validation.
    pub apparatus_ids: Vec<String>,
    /// Display snapshots retained for UI/audit only.
    pub apparatus: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InventoryTransferLine {
    pub asset_kind: InventoryAssetKind,
    pub asset_ref: String,
    pub item_code: String,
    pub item_name: String,
    pub identifier: String,
    pub qty: f64,
    pub uom: String,
    pub source_physical_location_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InventoryTransfer {
    pub id: String,
    pub source_warehouse_id: String,
    pub source_warehouse: String,
    pub destination_warehouse_id: String,
    pub destination_warehouse: String,
    pub status: InventoryTransferStatus,
    pub note: String,
    pub requested_by_name: String,
    pub approved_by_name: String,
    pub dispatched_by_name: String,
    pub received_by_name: String,
    pub rejected_by_name: String,
    pub cancelled_by_name: String,
    pub created_at_unix: i64,
    pub approved_at_unix: Option<i64>,
    pub dispatched_at_unix: Option<i64>,
    pub received_at_unix: Option<i64>,
    pub rejected_at_unix: Option<i64>,
    pub cancelled_at_unix: Option<i64>,
    pub lines: Vec<InventoryTransferLine>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct InventoryAssetQuery {
    #[serde(default)]
    pub warehouse_id: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub asset_kind: Option<InventoryAssetKind>,
    #[serde(default)]
    pub current_user_states_only: bool,
    #[serde(default = "default_asset_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_asset_limit() -> usize {
    100
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct InventoryTransferQuery {
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub status: String,
    #[serde(default = "default_transfer_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_transfer_limit() -> usize {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryAssetSelector {
    pub asset_kind: InventoryAssetKind,
    pub asset_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InventoryRelocationCreate {
    pub asset_kind: InventoryAssetKind,
    pub asset_ref: String,
    pub physical_location_id: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InventoryRelocationBatchCreate {
    pub assets: Vec<InventoryAssetSelector>,
    pub physical_location_id: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InventoryReturnBatchCreate {
    pub assets: Vec<InventoryAssetSelector>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InventoryTransferCreate {
    pub source_warehouse_id: String,
    pub destination_warehouse_id: String,
    pub assets: Vec<InventoryAssetSelector>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct InventoryTransferAction {
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub idempotency_key: String,
}

#[derive(Debug, Clone)]
pub struct InventoryActor {
    pub principal: Principal,
    pub is_admin: bool,
    pub assigned_warehouses: BTreeSet<String>,
}

impl InventoryActor {
    pub fn new(
        principal: Principal,
        is_admin: bool,
        assigned_warehouses: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            principal,
            is_admin,
            assigned_warehouses: assigned_warehouses
                .into_iter()
                .map(|warehouse| warehouse.trim().to_ascii_lowercase())
                .filter(|warehouse| !warehouse.is_empty())
                .collect(),
        }
    }

    pub fn can_manage_warehouse(&self, warehouse: &str) -> bool {
        self.is_admin
            || self
                .assigned_warehouses
                .contains(&warehouse.trim().to_ascii_lowercase())
    }

    pub fn is_assigned_to_warehouse(&self, warehouse: &str) -> bool {
        self.assigned_warehouses
            .contains(&warehouse.trim().to_ascii_lowercase())
    }

    pub fn manages_transfer_internally(
        &self,
        source_warehouse: &str,
        destination_warehouse: &str,
    ) -> bool {
        self.is_assigned_to_warehouse(source_warehouse)
            && self.is_assigned_to_warehouse(destination_warehouse)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum InventoryMovementError {
    #[error("asset kind is invalid")]
    InvalidAssetKind,
    #[error("asset ref is required")]
    MissingAssetRef,
    #[error("inventory asset not found")]
    AssetNotFound,
    #[error("inventory asset is not available")]
    AssetUnavailable,
    #[error("inventory asset is not physically located in the source warehouse")]
    AssetNotInSourceWarehouse,
    #[error("inventory location is invalid")]
    InvalidLocation,
    #[error("inventory location not found")]
    LocationNotFound,
    #[error("inventory location is inactive")]
    LocationInactive,
    #[error("cross-warehouse relocation requires a transfer")]
    CrossWarehouseRelocation,
    #[error("warehouse is required")]
    MissingWarehouse,
    #[error("warehouse not found")]
    WarehouseNotFound,
    #[error("destination warehouse has no assignee")]
    DestinationWarehouseUnassigned,
    #[error("source and destination warehouses must differ")]
    SameWarehouse,
    #[error("at least one asset is required")]
    MissingAssets,
    #[error("duplicate transfer asset")]
    DuplicateAsset,
    #[error("transfer not found")]
    TransferNotFound,
    #[error("transfer status is invalid")]
    InvalidTransferStatus,
    #[error("transfer transition is invalid")]
    InvalidTransition,
    #[error("warehouse access denied")]
    WarehouseForbidden,
    #[error("idempotency key is required")]
    MissingIdempotencyKey,
    #[error("idempotency key belongs to another operation")]
    IdempotencyConflict,
    #[error("inventory movement store failed")]
    StoreFailed,
}

#[async_trait]
pub trait InventoryMovementStorePort: Send + Sync {
    async fn locations(&self) -> Result<Vec<InventoryLocation>, InventoryMovementError>;

    async fn raw_material_state_placements(
        &self,
        barcodes: &[String],
    ) -> Result<Vec<RawMaterialStatePlacement>, InventoryMovementError>;

    async fn assets(
        &self,
        actor: &InventoryActor,
        query: &InventoryAssetQuery,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError>;

    async fn relocate(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationCreate,
    ) -> Result<InventoryAsset, InventoryMovementError>;

    async fn relocate_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError>;

    async fn return_to_warehouses_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryReturnBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError>;

    async fn create_transfer(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        input: &InventoryTransferCreate,
    ) -> Result<InventoryTransfer, InventoryMovementError>;

    async fn transfers(
        &self,
        actor: &InventoryActor,
        query: &InventoryTransferQuery,
    ) -> Result<Vec<InventoryTransfer>, InventoryMovementError>;

    async fn transfer_action(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        action: InventoryTransferActionKind,
        input: &InventoryTransferAction,
    ) -> Result<InventoryTransfer, InventoryMovementError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryTransferActionKind {
    Approve,
    Reject,
    Dispatch,
    Receive,
    Cancel,
}
