impl MemoryProductionMapStore {
    async fn receive_finished_goods_batch(
        &self,
        batch: OrderProgressBatch,
        stock: FinishedGoodsStockEntry,
    ) -> Result<(), ProductionMapError> {
        runs::receive_finished_goods_batch(self, batch, stock).await
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        materials::raw_material_assignments(self).await
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        materials::put_raw_material_assignment(self, assignment).await
    }

    async fn delete_raw_material_assignment(
        &self,
        order_id: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        materials::delete_raw_material_assignment(self, order_id, barcode).await
    }
}


include!("../memory_store_trait_impl.rs");
