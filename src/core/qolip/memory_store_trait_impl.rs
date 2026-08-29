#[async_trait]
impl QolipStorePort for MemoryQolipStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        MemoryQolipStore::assigned_warehouses(self, _principal).await
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        MemoryQolipStore::assigned_blocks(self, _principal).await
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        MemoryQolipStore::all_blocks(self).await
    }

    async fn rename_block(
        &self,
        block: &str,
        new_block: &str,
        warehouse: &str,
    ) -> Result<QolipBlock, QolipError> {
        MemoryQolipStore::rename_block(self, block, new_block, warehouse).await
    }

    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        MemoryQolipStore::products(self, query, limit, with_qolip_only).await
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        MemoryQolipStore::product_spec(self, item_code).await
    }

    async fn product_specs(&self, item_code: &str) -> Result<Vec<QolipProductSpec>, QolipError> {
        MemoryQolipStore::product_specs(self, item_code).await
    }

    async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        MemoryQolipStore::product_spec_by_qolip_code(self, qolip_code).await
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        MemoryQolipStore::put_product_spec(self, spec).await
    }

    async fn put_product_specs(
        &self,
        batch_specs: Vec<QolipProductSpec>,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        MemoryQolipStore::put_product_specs(self, batch_specs).await
    }

    async fn rename_product_spec(
        &self,
        previous_qolip_code: &str,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        MemoryQolipStore::rename_product_spec(self, previous_qolip_code, spec).await
    }

    async fn delete_product_specs(&self, qolip_codes: &[String]) -> Result<usize, QolipError> {
        MemoryQolipStore::delete_product_specs(self, qolip_codes).await
    }

    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        MemoryQolipStore::locations(self, block).await
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        MemoryQolipStore::put_location(self, location).await
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        MemoryQolipStore::get_or_create_cell_qr(self, cell).await
    }

    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError> {
        MemoryQolipStore::location_by_id(self, location_id).await
    }

    async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        MemoryQolipStore::location_by_qolip_code(self, qolip_code).await
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        MemoryQolipStore::issue_checkout(self, checkout).await
    }

    async fn open_checkout_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        MemoryQolipStore::open_checkout_by_qolip_code(self, qolip_code).await
    }

    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        MemoryQolipStore::checkouts(self, block, allowed_blocks, status, limit).await
    }

    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError> {
        MemoryQolipStore::checkout_by_id(self, checkout_id).await
    }

    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        MemoryQolipStore::return_checkout(self, checkout_id, row_letter, column_number).await
    }

    async fn move_location(
        &self,
        location_id: &str,
        block: &str,
        warehouse: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        MemoryQolipStore::move_location(self, location_id, block, warehouse, row_letter, column_number, quantity).await
    }

    async fn move_locations(
        &self,
        moves: &[QolipLocationMove],
    ) -> Result<Vec<QolipLocation>, QolipError> {
        MemoryQolipStore::move_locations(self, moves).await
    }

    async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        MemoryQolipStore::cell_qr_by_payload(self, qr_payload).await
    }
}
