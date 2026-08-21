#[async_trait]
impl InventoryMovementStorePort for PostgresInventoryMovementStore {
    async fn locations(&self) -> Result<Vec<InventoryLocation>, InventoryMovementError> {
        PostgresInventoryMovementStore::locations(self).await
    }

    async fn raw_material_state_placements(
        &self,
        barcodes: &[String],
    ) -> Result<Vec<RawMaterialStatePlacement>, InventoryMovementError> {
        PostgresInventoryMovementStore::raw_material_state_placements(self, barcodes).await
    }

    async fn assets(
        &self,
        actor: &InventoryActor,
        query: &InventoryAssetQuery,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        PostgresInventoryMovementStore::assets(self, actor, query).await
    }

    async fn relocate(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationCreate,
    ) -> Result<InventoryAsset, InventoryMovementError> {
        PostgresInventoryMovementStore::relocate(self, actor, input).await
    }

    async fn relocate_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        PostgresInventoryMovementStore::relocate_batch(self, actor, input).await
    }

    async fn return_to_warehouses_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryReturnBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        PostgresInventoryMovementStore::return_to_warehouses_batch(self, actor, input).await
    }

    async fn create_transfer(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        input: &InventoryTransferCreate,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        PostgresInventoryMovementStore::create_transfer(self, actor, transfer_id, input).await
    }

    async fn transfers(
        &self,
        actor: &InventoryActor,
        query: &InventoryTransferQuery,
    ) -> Result<Vec<InventoryTransfer>, InventoryMovementError> {
        PostgresInventoryMovementStore::transfers(self, actor, query).await
    }

    async fn transfer_action(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        action: InventoryTransferActionKind,
        input: &InventoryTransferAction,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        PostgresInventoryMovementStore::transfer_action(self, actor, transfer_id, action, input).await
    }
}
