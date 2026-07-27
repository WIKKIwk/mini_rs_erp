use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::core::auth::models::{Principal, PrincipalRole};

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
        match raw.trim().to_ascii_lowercase().as_str() {
            "raw_material" => Ok(Self::RawMaterial),
            "finished_goods" => Ok(Self::FinishedGoods),
            "qolip" => Ok(Self::Qolip),
            _ => Err(InventoryMovementError::InvalidAssetKind),
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
        match raw.trim().to_ascii_lowercase().as_str() {
            "warehouse" => Ok(Self::Warehouse),
            "state" => Ok(Self::State),
            "transit" => Ok(Self::Transit),
            _ => Err(InventoryMovementError::InvalidLocation),
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
        match raw.trim().to_ascii_lowercase().as_str() {
            "requested" => Ok(Self::Requested),
            "approved" => Ok(Self::Approved),
            "in_transit" => Ok(Self::InTransit),
            "received" => Ok(Self::Received),
            "rejected" => Ok(Self::Rejected),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(InventoryMovementError::InvalidTransferStatus),
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

impl InventoryTransferActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::Dispatch => "dispatch",
            Self::Receive => "receive",
            Self::Cancel => "cancel",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, InventoryMovementError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "approve" => Ok(Self::Approve),
            "reject" => Ok(Self::Reject),
            "dispatch" => Ok(Self::Dispatch),
            "receive" => Ok(Self::Receive),
            "cancel" => Ok(Self::Cancel),
            _ => Err(InventoryMovementError::InvalidTransition),
        }
    }
}

#[derive(Clone)]
pub struct InventoryMovementService {
    store: Arc<dyn InventoryMovementStorePort>,
}

impl InventoryMovementService {
    pub fn new(store: Arc<dyn InventoryMovementStorePort>) -> Self {
        Self { store }
    }

    pub async fn locations(&self) -> Result<Vec<InventoryLocation>, InventoryMovementError> {
        self.store.locations().await
    }

    pub async fn assets(
        &self,
        actor: &InventoryActor,
        mut query: InventoryAssetQuery,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        query.warehouse_id = query.warehouse_id.trim().to_string();
        query.query = query.query.trim().to_string();
        query.limit = query.limit.clamp(1, 500);
        query.offset = query.offset.min(100_000);
        self.store.assets(actor, &query).await
    }

    pub async fn relocate(
        &self,
        actor: &InventoryActor,
        mut input: InventoryRelocationCreate,
    ) -> Result<InventoryAsset, InventoryMovementError> {
        input.asset_ref = required(input.asset_ref, InventoryMovementError::MissingAssetRef)?;
        input.physical_location_id = required(
            input.physical_location_id,
            InventoryMovementError::InvalidLocation,
        )?;
        input.idempotency_key = normalize_idempotency(input.idempotency_key)?;
        input.note = input.note.trim().to_string();
        self.store.relocate(actor, &input).await
    }

    pub async fn create_transfer(
        &self,
        actor: &InventoryActor,
        mut input: InventoryTransferCreate,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        input.source_warehouse_id = required(
            input.source_warehouse_id,
            InventoryMovementError::MissingWarehouse,
        )?;
        input.destination_warehouse_id = required(
            input.destination_warehouse_id,
            InventoryMovementError::MissingWarehouse,
        )?;
        if input.source_warehouse_id == input.destination_warehouse_id {
            return Err(InventoryMovementError::SameWarehouse);
        }
        input.idempotency_key = normalize_idempotency(input.idempotency_key)?;
        input.note = input.note.trim().to_string();
        let mut seen = BTreeSet::new();
        input.assets = input
            .assets
            .into_iter()
            .map(|mut asset| {
                asset.asset_ref = asset.asset_ref.trim().to_string();
                asset
            })
            .filter(|asset| !asset.asset_ref.is_empty())
            .collect();
        if input.assets.is_empty() {
            return Err(InventoryMovementError::MissingAssets);
        }
        for asset in &input.assets {
            if !seen.insert((asset.asset_kind, asset.asset_ref.to_ascii_lowercase())) {
                return Err(InventoryMovementError::DuplicateAsset);
            }
        }
        let transfer_id = movement_id("inventory_transfer");
        self.store
            .create_transfer(actor, &transfer_id, &input)
            .await
    }

    pub async fn transfers(
        &self,
        actor: &InventoryActor,
        mut query: InventoryTransferQuery,
    ) -> Result<Vec<InventoryTransfer>, InventoryMovementError> {
        query.direction = query.direction.trim().to_ascii_lowercase();
        if !matches!(
            query.direction.as_str(),
            "" | "all" | "incoming" | "outgoing"
        ) {
            return Err(InventoryMovementError::InvalidTransition);
        }
        query.status = query.status.trim().to_ascii_lowercase();
        if !query.status.is_empty() {
            InventoryTransferStatus::parse(&query.status)?;
        }
        query.limit = query.limit.clamp(1, 500);
        query.offset = query.offset.min(100_000);
        self.store.transfers(actor, &query).await
    }

    pub async fn transfer_action(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        action: InventoryTransferActionKind,
        mut input: InventoryTransferAction,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let transfer_id = transfer_id.trim();
        if transfer_id.is_empty() {
            return Err(InventoryMovementError::TransferNotFound);
        }
        input.idempotency_key = normalize_idempotency(input.idempotency_key)?;
        input.note = input.note.trim().to_string();
        self.store
            .transfer_action(actor, transfer_id, action, &input)
            .await
    }
}

fn required(
    value: String,
    error: InventoryMovementError,
) -> Result<String, InventoryMovementError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(error)
    } else {
        Ok(value)
    }
}

fn normalize_idempotency(value: String) -> Result<String, InventoryMovementError> {
    required(value, InventoryMovementError::MissingIdempotencyKey)
}

pub fn inventory_role_code(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

fn movement_id(prefix: &str) -> String {
    format!("{prefix}_{}", HEXLOWER.encode(&rand::random::<[u8; 16]>()))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Default)]
struct MemoryInventoryState {
    locations: BTreeMap<String, InventoryLocation>,
    assets: BTreeMap<(InventoryAssetKind, String), InventoryAsset>,
    transfers: BTreeMap<String, InventoryTransfer>,
    idempotency: BTreeMap<String, String>,
    relocation_idempotency: BTreeMap<String, (InventoryAssetKind, String, String)>,
    action_idempotency: BTreeMap<String, (String, InventoryTransferActionKind)>,
}

#[derive(Default)]
pub struct MemoryInventoryMovementStore {
    state: RwLock<MemoryInventoryState>,
}

impl MemoryInventoryMovementStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub async fn seed_locations(&self, locations: Vec<InventoryLocation>) {
        let mut state = self.state.write().await;
        for location in locations {
            state.locations.insert(location.id.clone(), location);
        }
    }

    #[cfg(test)]
    pub async fn seed_assets(&self, assets: Vec<InventoryAsset>) {
        let mut state = self.state.write().await;
        for asset in assets {
            state
                .assets
                .insert((asset.kind, asset.asset_ref.to_ascii_lowercase()), asset);
        }
    }
}

#[async_trait]
impl InventoryMovementStorePort for MemoryInventoryMovementStore {
    async fn locations(&self) -> Result<Vec<InventoryLocation>, InventoryMovementError> {
        let mut locations = self
            .state
            .read()
            .await
            .locations
            .values()
            .filter(|location| location.active)
            .cloned()
            .collect::<Vec<_>>();
        locations.sort_by(|left, right| {
            location_kind_rank(left.kind)
                .cmp(&location_kind_rank(right.kind))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Ok(locations)
    }

    async fn assets(
        &self,
        actor: &InventoryActor,
        query: &InventoryAssetQuery,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        let state = self.state.read().await;
        let requested_warehouse = state
            .locations
            .values()
            .find(|location| {
                location.kind == InventoryLocationKind::Warehouse
                    && location
                        .warehouse_id
                        .eq_ignore_ascii_case(query.warehouse_id.trim())
            })
            .map(|location| location.name.clone());
        if !query.warehouse_id.trim().is_empty() && requested_warehouse.is_none() {
            return Err(InventoryMovementError::WarehouseNotFound);
        }
        if let Some(warehouse) = requested_warehouse.as_deref() {
            if !actor.can_manage_warehouse(warehouse) {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
        }
        let needle = query.query.to_ascii_lowercase();
        let mut assets = state
            .assets
            .values()
            .filter(|asset| actor.can_manage_warehouse(&asset.custody_warehouse))
            .filter(|asset| {
                requested_warehouse
                    .as_deref()
                    .map(|warehouse| asset.custody_warehouse.eq_ignore_ascii_case(warehouse))
                    .unwrap_or(true)
            })
            .filter(|asset| {
                query
                    .asset_kind
                    .map(|kind| kind == asset.kind)
                    .unwrap_or(true)
            })
            .filter(|asset| {
                needle.is_empty()
                    || [
                        &asset.item_code,
                        &asset.item_name,
                        &asset.identifier,
                        &asset.asset_ref,
                    ]
                    .iter()
                    .any(|value| value.to_ascii_lowercase().contains(&needle))
            })
            .cloned()
            .collect::<Vec<_>>();
        assets.sort_by(|left, right| {
            left.item_name
                .to_lowercase()
                .cmp(&right.item_name.to_lowercase())
                .then_with(|| left.identifier.cmp(&right.identifier))
        });
        Ok(assets
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect())
    }

    async fn relocate(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationCreate,
    ) -> Result<InventoryAsset, InventoryMovementError> {
        let mut state = self.state.write().await;
        if let Some((kind, asset_ref, location_id)) =
            state.relocation_idempotency.get(&input.idempotency_key)
        {
            if *kind != input.asset_kind
                || !asset_ref.eq_ignore_ascii_case(&input.asset_ref)
                || location_id != &input.physical_location_id
            {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return state
                .assets
                .get(&(input.asset_kind, input.asset_ref.to_ascii_lowercase()))
                .cloned()
                .ok_or(InventoryMovementError::AssetNotFound);
        }
        let location = state
            .locations
            .get(input.physical_location_id.trim())
            .cloned()
            .ok_or(InventoryMovementError::LocationNotFound)?;
        if !location.active {
            return Err(InventoryMovementError::LocationInactive);
        }
        let key = (input.asset_kind, input.asset_ref.to_ascii_lowercase());
        let asset = state
            .assets
            .get_mut(&key)
            .ok_or(InventoryMovementError::AssetNotFound)?;
        if !actor.can_manage_warehouse(&asset.custody_warehouse) {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        if !asset.transfer_id.is_empty() || asset.status != "available" {
            return Err(InventoryMovementError::AssetUnavailable);
        }
        if location.kind == InventoryLocationKind::Warehouse
            && !location
                .warehouse_id
                .eq_ignore_ascii_case(&asset.custody_warehouse_id)
        {
            return Err(InventoryMovementError::CrossWarehouseRelocation);
        }
        asset.physical_location = InventoryLocationRef::from(&location);
        asset.placement_version += 1;
        let saved = asset.clone();
        state.relocation_idempotency.insert(
            input.idempotency_key.clone(),
            (
                input.asset_kind,
                input.asset_ref.clone(),
                input.physical_location_id.clone(),
            ),
        );
        Ok(saved)
    }

    async fn create_transfer(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        input: &InventoryTransferCreate,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let mut state = self.state.write().await;
        if let Some(existing_id) = state.idempotency.get(&input.idempotency_key) {
            let existing = state
                .transfers
                .get(existing_id)
                .cloned()
                .ok_or(InventoryMovementError::IdempotencyConflict)?;
            if !transfer_matches_create(&existing, input) {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return Ok(existing);
        }
        let source = warehouse_location(&state, &input.source_warehouse_id)?;
        let destination = warehouse_location(&state, &input.destination_warehouse_id)?;
        if !actor.can_manage_warehouse(&source.name) {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        let mut lines = Vec::new();
        for selector in &input.assets {
            let key = (
                selector.asset_kind,
                selector.asset_ref.trim().to_ascii_lowercase(),
            );
            let asset = state
                .assets
                .get(&key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            if !asset
                .custody_warehouse_id
                .eq_ignore_ascii_case(&source.warehouse_id)
            {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
            if !asset.transfer_id.is_empty() || asset.status != "available" {
                return Err(InventoryMovementError::AssetUnavailable);
            }
            lines.push(InventoryTransferLine {
                asset_kind: asset.kind,
                asset_ref: asset.asset_ref.clone(),
                item_code: asset.item_code.clone(),
                item_name: asset.item_name.clone(),
                identifier: asset.identifier.clone(),
                qty: asset.qty,
                uom: asset.uom.clone(),
                source_physical_location_id: asset.physical_location.id.clone(),
            });
        }
        for selector in &input.assets {
            let key = (
                selector.asset_kind,
                selector.asset_ref.trim().to_ascii_lowercase(),
            );
            let asset = state
                .assets
                .get_mut(&key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            asset.transfer_id = transfer_id.to_string();
            asset.status = "transfer_reserved".to_string();
        }
        let transfer = InventoryTransfer {
            id: transfer_id.to_string(),
            source_warehouse_id: source.warehouse_id,
            source_warehouse: source.name,
            destination_warehouse_id: destination.warehouse_id,
            destination_warehouse: destination.name,
            status: InventoryTransferStatus::Requested,
            note: input.note.clone(),
            requested_by_name: actor.principal.display_name.clone(),
            approved_by_name: String::new(),
            dispatched_by_name: String::new(),
            received_by_name: String::new(),
            rejected_by_name: String::new(),
            cancelled_by_name: String::new(),
            created_at_unix: now_unix(),
            approved_at_unix: None,
            dispatched_at_unix: None,
            received_at_unix: None,
            rejected_at_unix: None,
            cancelled_at_unix: None,
            lines,
        };
        state
            .idempotency
            .insert(input.idempotency_key.clone(), transfer.id.clone());
        state
            .transfers
            .insert(transfer.id.clone(), transfer.clone());
        Ok(transfer)
    }

    async fn transfers(
        &self,
        actor: &InventoryActor,
        query: &InventoryTransferQuery,
    ) -> Result<Vec<InventoryTransfer>, InventoryMovementError> {
        let state = self.state.read().await;
        let mut transfers = state
            .transfers
            .values()
            .filter(|transfer| match query.direction.as_str() {
                "incoming" => actor.can_manage_warehouse(&transfer.destination_warehouse),
                "outgoing" => actor.can_manage_warehouse(&transfer.source_warehouse),
                _ => {
                    actor.can_manage_warehouse(&transfer.source_warehouse)
                        || actor.can_manage_warehouse(&transfer.destination_warehouse)
                }
            })
            .filter(|transfer| query.status.is_empty() || transfer.status.as_str() == query.status)
            .cloned()
            .collect::<Vec<_>>();
        transfers.sort_by(|left, right| {
            right
                .created_at_unix
                .cmp(&left.created_at_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(transfers
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect())
    }

    async fn transfer_action(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        action: InventoryTransferActionKind,
        _input: &InventoryTransferAction,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let mut state = self.state.write().await;
        let transfer = state
            .transfers
            .get(transfer_id)
            .cloned()
            .ok_or(InventoryMovementError::TransferNotFound)?;
        let source_access = actor.can_manage_warehouse(&transfer.source_warehouse);
        let destination_access = actor.can_manage_warehouse(&transfer.destination_warehouse);
        let authorized = match action {
            InventoryTransferActionKind::Approve
            | InventoryTransferActionKind::Reject
            | InventoryTransferActionKind::Receive => destination_access,
            InventoryTransferActionKind::Dispatch | InventoryTransferActionKind::Cancel => {
                source_access
            }
        };
        if !authorized {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        if let Some((existing_transfer_id, existing_action)) =
            state.action_idempotency.get(&_input.idempotency_key)
        {
            if existing_transfer_id != transfer_id || *existing_action != action {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return Ok(transfer);
        }
        let now = now_unix();

        let mut updated = transfer.clone();
        if transfer_action_already_applied(updated.status, action) {
            state.action_idempotency.insert(
                _input.idempotency_key.clone(),
                (transfer_id.to_string(), action),
            );
            return Ok(updated);
        }
        match action {
            InventoryTransferActionKind::Approve => {
                if updated.status != InventoryTransferStatus::Requested {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                updated.status = InventoryTransferStatus::Approved;
                updated.approved_by_name = actor.principal.display_name.clone();
                updated.approved_at_unix = Some(now);
            }
            InventoryTransferActionKind::Reject => {
                if updated.status != InventoryTransferStatus::Requested {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                updated.status = InventoryTransferStatus::Rejected;
                updated.rejected_by_name = actor.principal.display_name.clone();
                updated.rejected_at_unix = Some(now);
                release_memory_assets(&mut state, &updated);
            }
            InventoryTransferActionKind::Dispatch => {
                if updated.status != InventoryTransferStatus::Approved {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                updated.status = InventoryTransferStatus::InTransit;
                updated.dispatched_by_name = actor.principal.display_name.clone();
                updated.dispatched_at_unix = Some(now);
                for line in &updated.lines {
                    if let Some(asset) = state
                        .assets
                        .get_mut(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
                    {
                        asset.status = "in_transit".to_string();
                    }
                }
            }
            InventoryTransferActionKind::Receive => {
                if updated.status != InventoryTransferStatus::InTransit {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                let destination = warehouse_location(&state, &updated.destination_warehouse_id)?;
                for line in &updated.lines {
                    let asset = state
                        .assets
                        .get_mut(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
                        .ok_or(InventoryMovementError::AssetNotFound)?;
                    if asset.transfer_id != updated.id {
                        return Err(InventoryMovementError::AssetUnavailable);
                    }
                    asset.custody_warehouse_id = updated.destination_warehouse_id.clone();
                    asset.custody_warehouse = updated.destination_warehouse.clone();
                    asset.physical_location = InventoryLocationRef::from(&destination);
                    asset.transfer_id.clear();
                    asset.status = "available".to_string();
                    asset.placement_version += 1;
                }
                updated.status = InventoryTransferStatus::Received;
                updated.received_by_name = actor.principal.display_name.clone();
                updated.received_at_unix = Some(now);
            }
            InventoryTransferActionKind::Cancel => {
                if !matches!(
                    updated.status,
                    InventoryTransferStatus::Requested | InventoryTransferStatus::Approved
                ) {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                updated.status = InventoryTransferStatus::Cancelled;
                updated.cancelled_by_name = actor.principal.display_name.clone();
                updated.cancelled_at_unix = Some(now);
                release_memory_assets(&mut state, &updated);
            }
        }
        state.transfers.insert(updated.id.clone(), updated.clone());
        state.action_idempotency.insert(
            _input.idempotency_key.clone(),
            (transfer_id.to_string(), action),
        );
        Ok(updated)
    }
}

fn transfer_matches_create(transfer: &InventoryTransfer, input: &InventoryTransferCreate) -> bool {
    if !transfer
        .source_warehouse_id
        .eq_ignore_ascii_case(&input.source_warehouse_id)
        || !transfer
            .destination_warehouse_id
            .eq_ignore_ascii_case(&input.destination_warehouse_id)
        || transfer.note != input.note
    {
        return false;
    }
    let existing = transfer
        .lines
        .iter()
        .map(|line| (line.asset_kind, line.asset_ref.to_ascii_lowercase()))
        .collect::<BTreeSet<_>>();
    let requested = input
        .assets
        .iter()
        .map(|asset| (asset.asset_kind, asset.asset_ref.to_ascii_lowercase()))
        .collect::<BTreeSet<_>>();
    existing == requested && existing.len() == transfer.lines.len()
}

fn transfer_action_already_applied(
    status: InventoryTransferStatus,
    action: InventoryTransferActionKind,
) -> bool {
    matches!(
        (status, action),
        (
            InventoryTransferStatus::Approved
                | InventoryTransferStatus::InTransit
                | InventoryTransferStatus::Received,
            InventoryTransferActionKind::Approve
        ) | (
            InventoryTransferStatus::Rejected,
            InventoryTransferActionKind::Reject
        ) | (
            InventoryTransferStatus::InTransit | InventoryTransferStatus::Received,
            InventoryTransferActionKind::Dispatch
        ) | (
            InventoryTransferStatus::Received,
            InventoryTransferActionKind::Receive
        ) | (
            InventoryTransferStatus::Cancelled,
            InventoryTransferActionKind::Cancel
        )
    )
}

fn warehouse_location(
    state: &MemoryInventoryState,
    warehouse_id: &str,
) -> Result<InventoryLocation, InventoryMovementError> {
    state
        .locations
        .values()
        .find(|location| {
            location.kind == InventoryLocationKind::Warehouse
                && location
                    .warehouse_id
                    .eq_ignore_ascii_case(warehouse_id.trim())
        })
        .cloned()
        .ok_or(InventoryMovementError::WarehouseNotFound)
}

fn release_memory_assets(state: &mut MemoryInventoryState, transfer: &InventoryTransfer) {
    for line in &transfer.lines {
        if let Some(asset) = state
            .assets
            .get_mut(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
        {
            if asset.transfer_id == transfer.id {
                asset.transfer_id.clear();
                asset.status = "available".to_string();
            }
        }
    }
}

fn location_kind_rank(kind: InventoryLocationKind) -> u8 {
    match kind {
        InventoryLocationKind::State => 0,
        InventoryLocationKind::Warehouse => 1,
        InventoryLocationKind::Transit => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(role: PrincipalRole, ref_: &str, name: &str) -> Principal {
        Principal {
            role,
            display_name: name.to_string(),
            legal_name: name.to_string(),
            ref_: ref_.to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    fn warehouse_location_fixture(id: &str, name: &str) -> InventoryLocation {
        InventoryLocation {
            id: format!("location:{id}"),
            kind: InventoryLocationKind::Warehouse,
            name: name.to_string(),
            warehouse_id: id.to_string(),
            factory_location_id: String::new(),
            active: true,
            apparatus: Vec::new(),
        }
    }

    #[tokio::test]
    async fn relocation_changes_only_physical_location() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let state_location = InventoryLocation {
            id: "location:state:bosma".to_string(),
            kind: InventoryLocationKind::State,
            name: "Bosma oldi".to_string(),
            warehouse_id: String::new(),
            factory_location_id: "state_bosma".to_string(),
            active: true,
            apparatus: Vec::new(),
        };
        store
            .seed_locations(vec![source.clone(), state_location.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::RawMaterial,
                asset_ref: "raw:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "PE".to_string(),
                item_name: "Polietilen".to_string(),
                identifier: "QR-1".to_string(),
                qty: 10.0,
                uom: "kg".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["A ombor".to_string()],
        );

        let moved = service
            .relocate(
                &actor,
                InventoryRelocationCreate {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:1".to_string(),
                    physical_location_id: state_location.id.clone(),
                    note: String::new(),
                    idempotency_key: "relocate-1".to_string(),
                },
            )
            .await
            .expect("relocate");

        assert_eq!(moved.qty, 10.0);
        assert_eq!(moved.custody_warehouse, "A ombor");
        assert_eq!(moved.physical_location.name, "Bosma oldi");
    }

    #[tokio::test]
    async fn relocation_cannot_change_custody_warehouse() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::RawMaterial,
                asset_ref: "raw:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "PE".to_string(),
                item_name: "Polietilen".to_string(),
                identifier: "QR-1".to_string(),
                qty: 10.0,
                uom: "kg".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["A ombor".to_string()],
        );

        let error = service
            .relocate(
                &actor,
                InventoryRelocationCreate {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:1".to_string(),
                    physical_location_id: destination.id,
                    note: String::new(),
                    idempotency_key: "relocate-cross-warehouse".to_string(),
                },
            )
            .await
            .expect_err("warehouse change must use bilateral transfer");

        assert_eq!(error, InventoryMovementError::CrossWarehouseRelocation);
    }

    #[tokio::test]
    async fn bilateral_transfer_preserves_total_quantity_and_requires_both_sides() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::Qolip,
                asset_ref: "qolip:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "Q-1".to_string(),
                item_name: "Qolip".to_string(),
                identifier: "QOLIP-1".to_string(),
                qty: 4.0,
                uom: "dona".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let source_actor = InventoryActor::new(
            principal(PrincipalRole::Qolipchi, "q1", "Qolipchi"),
            false,
            ["A ombor".to_string()],
        );
        let destination_actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["B ombor".to_string()],
        );

        let transfer = service
            .create_transfer(
                &source_actor,
                InventoryTransferCreate {
                    source_warehouse_id: source.warehouse_id.clone(),
                    destination_warehouse_id: destination.warehouse_id.clone(),
                    assets: vec![InventoryAssetSelector {
                        asset_kind: InventoryAssetKind::Qolip,
                        asset_ref: "qolip:1".to_string(),
                    }],
                    note: "Kelishildi".to_string(),
                    idempotency_key: "transfer-1".to_string(),
                },
            )
            .await
            .expect("request");
        assert_eq!(transfer.status, InventoryTransferStatus::Requested);

        let approved = service
            .transfer_action(
                &destination_actor,
                &transfer.id,
                InventoryTransferActionKind::Approve,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "approve-1".to_string(),
                },
            )
            .await
            .expect("approve");
        assert_eq!(approved.status, InventoryTransferStatus::Approved);

        let dispatched = service
            .transfer_action(
                &source_actor,
                &transfer.id,
                InventoryTransferActionKind::Dispatch,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "dispatch-1".to_string(),
                },
            )
            .await
            .expect("dispatch");
        assert_eq!(dispatched.status, InventoryTransferStatus::InTransit);

        let received = service
            .transfer_action(
                &destination_actor,
                &transfer.id,
                InventoryTransferActionKind::Receive,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "receive-1".to_string(),
                },
            )
            .await
            .expect("receive");
        assert_eq!(received.status, InventoryTransferStatus::Received);

        let destination_assets = service
            .assets(
                &destination_actor,
                InventoryAssetQuery {
                    warehouse_id: destination.warehouse_id.clone(),
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("destination assets");
        assert_eq!(destination_assets.len(), 1);
        assert_eq!(destination_assets[0].qty, 4.0);
        assert_eq!(destination_assets[0].custody_warehouse, "B ombor");
    }

    #[tokio::test]
    async fn rejected_transfer_releases_asset_and_action_keys_are_operation_scoped() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::FinishedGoods,
                asset_ref: "finished:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "T-1".to_string(),
                item_name: "Tayyor mahsulot".to_string(),
                identifier: "FG-1".to_string(),
                qty: 12.0,
                uom: "dona".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let source_actor = InventoryActor::new(
            principal(PrincipalRole::Werka, "w1", "Werka"),
            false,
            ["A ombor".to_string()],
        );
        let destination_actor = InventoryActor::new(
            principal(PrincipalRole::Qolipchi, "q1", "Qolipchi"),
            false,
            ["B ombor".to_string()],
        );

        let transfer = service
            .create_transfer(
                &source_actor,
                InventoryTransferCreate {
                    source_warehouse_id: source.warehouse_id.clone(),
                    destination_warehouse_id: destination.warehouse_id,
                    assets: vec![InventoryAssetSelector {
                        asset_kind: InventoryAssetKind::FinishedGoods,
                        asset_ref: "finished:1".to_string(),
                    }],
                    note: String::new(),
                    idempotency_key: "transfer-reject-1".to_string(),
                },
            )
            .await
            .expect("request");
        service
            .transfer_action(
                &destination_actor,
                &transfer.id,
                InventoryTransferActionKind::Reject,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "reject-1".to_string(),
                },
            )
            .await
            .expect("reject");

        let assets = service
            .assets(
                &source_actor,
                InventoryAssetQuery {
                    warehouse_id: source.warehouse_id,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("source assets");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].status, "available");
        assert!(assets[0].transfer_id.is_empty());

        let conflict = service
            .transfer_action(
                &source_actor,
                &transfer.id,
                InventoryTransferActionKind::Cancel,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "reject-1".to_string(),
                },
            )
            .await
            .expect_err("same action key cannot identify another operation");
        assert_eq!(conflict, InventoryMovementError::IdempotencyConflict);
    }
}
