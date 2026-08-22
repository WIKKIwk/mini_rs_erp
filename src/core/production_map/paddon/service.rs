use std::collections::HashSet;

use super::*;

const MAX_PADDON_BATCH_ITEMS: usize = 500;

fn normalize_paddon_batch_ids(
    progress_batch_ids: &[String],
) -> Result<Vec<String>, ProductionMapError> {
    let mut seen = HashSet::with_capacity(progress_batch_ids.len());
    let mut normalized = Vec::with_capacity(progress_batch_ids.len());
    for progress_batch_id in progress_batch_ids {
        let progress_batch_id = progress_batch_id.trim();
        if progress_batch_id.is_empty() || !seen.insert(progress_batch_id.to_string()) {
            if progress_batch_id.is_empty() {
                return Err(ProductionMapError::PaddonInvalidInput);
            }
            continue;
        }
        normalized.push(progress_batch_id.to_string());
    }
    if normalized.is_empty() || normalized.len() > MAX_PADDON_BATCH_ITEMS {
        return Err(ProductionMapError::PaddonInvalidInput);
    }
    Ok(normalized)
}

impl ProductionMapService {
    pub async fn paddons(&self, limit: usize) -> Result<Vec<PaddonSummary>, ProductionMapError> {
        self.store.paddons(limit.clamp(1, 200)).await
    }

    pub async fn paddon_summary(&self, code: &str) -> Result<PaddonSummary, ProductionMapError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        self.store
            .paddon_summary(code)
            .await?
            .ok_or(ProductionMapError::PaddonNotFound)
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

    pub async fn paddon_snapshot(&self, code: &str) -> Result<PaddonSnapshot, ProductionMapError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        self.store
            .paddon_snapshot(code)
            .await?
            .ok_or(ProductionMapError::PaddonNotFound)
    }

    pub async fn paddon_scan_snapshot(
        &self,
        code: &str,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        self.store
            .paddon_scan_snapshot(code)
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
        self.reject_training_paddon_batches(&[progress_batch_id.to_string()])
            .await?;
        self.store
            .add_paddon_item(code, progress_batch_id, actor)
            .await
    }

    pub async fn add_paddon_items(
        &self,
        code: &str,
        progress_batch_ids: &[String],
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        let progress_batch_ids = normalize_paddon_batch_ids(progress_batch_ids)?;
        self.reject_training_paddon_batches(&progress_batch_ids).await?;
        self.store
            .add_paddon_items(code, &progress_batch_ids, actor)
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
        self.reject_training_paddon_batches(&[progress_batch_id.to_string()])
            .await?;
        self.store
            .remove_paddon_item(code, progress_batch_id, actor)
            .await
    }

    pub async fn remove_paddon_items(
        &self,
        code: &str,
        progress_batch_ids: &[String],
        actor: &QueueActionActor,
    ) -> Result<PaddonSnapshot, ProductionMapError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(ProductionMapError::PaddonInvalidInput);
        }
        let progress_batch_ids = normalize_paddon_batch_ids(progress_batch_ids)?;
        self.reject_training_paddon_batches(&progress_batch_ids).await?;
        self.store
            .remove_paddon_items(code, &progress_batch_ids, actor)
            .await
    }

    async fn reject_training_paddon_batches(
        &self,
        progress_batch_ids: &[String],
    ) -> Result<(), ProductionMapError> {
        for progress_batch_id in progress_batch_ids {
            if let Some(batch) = self.store.progress_batch(progress_batch_id).await? {
                reject_training_order_id(&batch.order_id)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_ids_trim_and_deduplicate() {
        let ids = vec![
            " wip-1 ".to_string(),
            "wip-1".to_string(),
            "wip-2".to_string(),
        ];

        assert_eq!(
            normalize_paddon_batch_ids(&ids).expect("normalized ids"),
            vec!["wip-1".to_string(), "wip-2".to_string()]
        );
    }

    #[test]
    fn batch_ids_reject_empty_and_oversized_inputs() {
        assert_eq!(
            normalize_paddon_batch_ids(&[String::new()]),
            Err(ProductionMapError::PaddonInvalidInput)
        );

        let oversized = (0..=MAX_PADDON_BATCH_ITEMS)
            .map(|index| format!("wip-{index}"))
            .collect::<Vec<_>>();
        assert_eq!(
            normalize_paddon_batch_ids(&oversized),
            Err(ProductionMapError::PaddonInvalidInput)
        );
    }
}
