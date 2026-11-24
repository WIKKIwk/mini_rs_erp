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

    async fn list_order_assignments(
        &self,
        _actor: &InventoryActor,
        _barcodes: &[String],
        _assigned_apparatus: &[String],
    ) -> Result<Vec<InventoryOrderAssignment>, InventoryMovementError> {
        // An unsupported repository must not turn unknown assignments into
        // confirmed empty badges used by destructive actions.
        Err(InventoryMovementError::StoreFailed)
    }

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
