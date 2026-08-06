use super::*;

impl ProductionMapService {
    pub async fn paddons(&self, limit: usize) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        self.store.paddons(limit.clamp(1, 200)).await
    }

    pub async fn create_paddon(
        &self,
        location: &str,
        note: &str,
        actor: &QueueActionActor,
    ) -> Result<PaddonSummary, ProductionMapError> {
        let location = location.trim();
        let note = note.trim();
        if location.len() > 160 || note.len() > 500 {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        let input = PaddonCreateInput {
            location: location.to_string(),
            note: note.to_string(),
            actor_ref: actor.ref_.trim().to_string(),
            actor_display_name: actor.display_name.trim().to_string(),
        };
        self.store.create_paddon(input).await
    }

    pub async fn paddon_snapshot(
        &self,
        code: &str,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        self.store
            .paddon_snapshot(code)
            .await?
            .ok_or(ProductionMapError::PaddonNotFound)
    }

    pub async fn add_paddon_item(
        &self,
        code: &str,
        progress_batch_id: &str,
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        let code = code.trim();
        let progress_batch_id = progress_batch_id.trim();
        if code.is_empty() || progress_batch_id.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        self.store
            .add_paddon_item(code, progress_batch_id, actor)
            .await
    }

    pub async fn remove_paddon_item(
        &self,
        code: &str,
        progress_batch_id: &str,
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        let code = code.trim();
        let progress_batch_id = progress_batch_id.trim();
        if code.is_empty() || progress_batch_id.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        self.store
            .remove_paddon_item(code, progress_batch_id, actor)
            .await
    }
}
