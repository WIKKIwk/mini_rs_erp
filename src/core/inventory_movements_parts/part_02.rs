
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

    pub async fn raw_material_state_placements(
        &self,
        barcodes: &[String],
    ) -> Result<Vec<RawMaterialStatePlacement>, InventoryMovementError> {
        let barcodes = barcodes
            .iter()
            .map(|barcode| barcode.trim().to_ascii_uppercase())
            .filter(|barcode| !barcode.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        self.store.raw_material_state_placements(&barcodes).await
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

    pub async fn relocate_batch(
        &self,
        actor: &InventoryActor,
        mut input: InventoryRelocationBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        input.physical_location_id = required(
            input.physical_location_id,
            InventoryMovementError::InvalidLocation,
        )?;
        input.idempotency_key = normalize_idempotency(input.idempotency_key)?;
        input.note = input.note.trim().to_string();
        input.assets = normalize_asset_selectors(input.assets)?;
        self.store.relocate_batch(actor, &input).await
    }

    pub async fn return_to_warehouses_batch(
        &self,
        actor: &InventoryActor,
        mut input: InventoryReturnBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        input.idempotency_key = normalize_idempotency(input.idempotency_key)?;
        input.note = input.note.trim().to_string();
        input.assets = normalize_asset_selectors(input.assets)?;
        self.store.return_to_warehouses_batch(actor, &input).await
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
        if input
            .source_warehouse_id
            .eq_ignore_ascii_case(&input.destination_warehouse_id)
        {
            return Err(InventoryMovementError::SameWarehouse);
        }
        input.idempotency_key = normalize_idempotency(input.idempotency_key)?;
        input.note = input.note.trim().to_string();
        input.assets = normalize_asset_selectors(input.assets)?;
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

fn normalize_asset_selectors(
    assets: Vec<InventoryAssetSelector>,
) -> Result<Vec<InventoryAssetSelector>, InventoryMovementError> {
    let mut seen = BTreeSet::new();
    let mut assets = assets
        .into_iter()
        .map(|mut asset| {
            asset.asset_ref = asset.asset_ref.trim().to_string();
            asset
        })
        .filter(|asset| !asset.asset_ref.is_empty())
        .collect::<Vec<_>>();
    if assets.is_empty() {
        return Err(InventoryMovementError::MissingAssets);
    }
    for asset in &assets {
        if !seen.insert((asset.asset_kind, asset.asset_ref.to_ascii_lowercase())) {
            return Err(InventoryMovementError::DuplicateAsset);
        }
    }
    assets.sort_by(|left, right| {
        left.asset_kind.cmp(&right.asset_kind).then_with(|| {
            left.asset_ref
                .to_ascii_lowercase()
                .cmp(&right.asset_ref.to_ascii_lowercase())
        })
    });
    Ok(assets)
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
    placement_updated_by_ref: BTreeMap<(InventoryAssetKind, String), String>,
    transfers: BTreeMap<String, InventoryTransfer>,
    idempotency: BTreeMap<String, String>,
    relocation_idempotency: BTreeMap<String, (InventoryAssetKind, String, String)>,
    relocation_batch_idempotency: BTreeMap<String, (Vec<(InventoryAssetKind, String)>, String)>,
    return_batch_idempotency: BTreeMap<String, Vec<(InventoryAssetKind, String)>>,
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

include!("../inventory_movements_impl_parts/part_01.rs");
include!("../inventory_movements_impl_parts/part_02.rs");

include!("../inventory_movements_store_trait_impl.rs");

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
            && asset.transfer_id == transfer.id
        {
            asset.transfer_id.clear();
            asset.status = "available".to_string();
        }
    }
}

fn ensure_memory_transfer_assets(
    state: &MemoryInventoryState,
    transfer: &InventoryTransfer,
) -> Result<(), InventoryMovementError> {
    for line in &transfer.lines {
        let asset = state
            .assets
            .get(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
            .ok_or(InventoryMovementError::AssetNotFound)?;
        if asset.transfer_id != transfer.id
            || matches!(
                asset.status.as_str(),
                "available" | "consumed" | "dispatched"
            )
        {
            return Err(InventoryMovementError::AssetUnavailable);
        }
    }
    Ok(())
}

fn complete_memory_transfer(
    state: &mut MemoryInventoryState,
    transfer: &mut InventoryTransfer,
    actor: &InventoryActor,
    now: i64,
) -> Result<(), InventoryMovementError> {
    ensure_memory_transfer_assets(state, transfer)?;
    let destination = warehouse_location(state, &transfer.destination_warehouse_id)?;
    for line in &transfer.lines {
        let asset = state
            .assets
            .get_mut(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
            .ok_or(InventoryMovementError::AssetNotFound)?;
        if asset.transfer_id != transfer.id {
            return Err(InventoryMovementError::AssetUnavailable);
        }
        asset.custody_warehouse_id = transfer.destination_warehouse_id.clone();
        asset.custody_warehouse = transfer.destination_warehouse.clone();
        asset.physical_location = InventoryLocationRef::from(&destination);
        asset.transfer_id.clear();
        asset.status = "available".to_string();
        asset.placement_version += 1;
    }
    let actor_name = actor.principal.display_name.clone();
    if transfer.approved_at_unix.is_none() {
        transfer.approved_by_name = actor_name.clone();
        transfer.approved_at_unix = Some(now);
    }
    if transfer.dispatched_at_unix.is_none() {
        transfer.dispatched_by_name = actor_name.clone();
        transfer.dispatched_at_unix = Some(now);
    }
    transfer.status = InventoryTransferStatus::Received;
    transfer.received_by_name = actor_name;
    transfer.received_at_unix = Some(now);
    Ok(())
}

fn location_kind_rank(kind: InventoryLocationKind) -> u8 {
    match kind {
        InventoryLocationKind::State => 0,
        InventoryLocationKind::Warehouse => 1,
        InventoryLocationKind::Transit => 2,
    }
}

include!("../inventory_movements_inline_tests.rs");
