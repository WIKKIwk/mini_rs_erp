impl QolipService {
    pub async fn mileage(&self, qolip_code: &str) -> Result<super::QolipMileage, QolipError> {
        let code = qolip_code.trim();
        if code.is_empty() || code.len() > 512 {
            return Err(QolipError::MissingQolipCode);
        }
        self.store.mileage(code).await
    }
}
