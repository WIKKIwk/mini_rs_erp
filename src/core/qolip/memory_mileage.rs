impl MemoryQolipStore {
    pub(super) async fn memory_mileage(
        &self,
        code: &str,
    ) -> Result<super::QolipMileage, QolipError> {
        super::mileage::memory_mileage(&*self.mileage.read().await, code)
    }
}
