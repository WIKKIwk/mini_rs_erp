use super::*;

impl ProductionMapService {
    pub async fn correct_progress_batch(
        &self,
        input: ProgressBatchCorrectionInput,
        actor: &QueueActionActor,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        validate_progress_batch_correction_input(&input)?;
        let current = self
            .store
            .progress_batch(&input.batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if actor.ref_.trim().is_empty() {
            return Err(ProductionMapError::ProgressBatchCorrectionForbidden);
        }
        if current.worker_ref.trim() != actor.ref_.trim() {
            let actor_refs = vec![actor.ref_.trim().to_string()];
            let is_owned = self
                .store
                .progress_batches_for_worker(&actor_refs, "", 500)
                .await?
                .iter()
                .any(|batch| batch.batch_id.trim() == current.batch_id.trim());
            if !is_owned {
                return Err(ProductionMapError::ProgressBatchCorrectionForbidden);
            }
        }
        if current.wip_status != OrderProgressBatchWipStatus::Waiting {
            return Err(ProductionMapError::ProgressBatchCorrectionLocked);
        }
        if current.revision != input.expected_revision {
            return Err(ProductionMapError::ProgressBatchCorrectionConflict);
        }
        if progress_batch_correction_is_unchanged(&current, &input) {
            return Err(ProductionMapError::ProgressBatchCorrectionUnchanged);
        }
        let corrected = self
            .store
            .correct_progress_batch(current, input, actor.clone())
            .await?;
        self.notify_live();
        Ok(corrected)
    }
}

fn validate_progress_batch_correction_input(
    input: &ProgressBatchCorrectionInput,
) -> Result<(), ProductionMapError> {
    if input.batch_id.trim().is_empty()
        || input.expected_revision == 0
        || !input.produced_qty.is_finite()
        || input.produced_qty <= 0.0
        || input.uom.trim().is_empty()
    {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    if input.reason.trim().is_empty() {
        return Err(ProductionMapError::ProgressBatchCorrectionReasonRequired);
    }
    let optional_values = [
        input.return_ink_kg,
        input.lamination_print_leftover_rolls,
        input.lamination_film_leftover_rolls,
        input.rezka_bosma_waste,
        input.rezka_lamination_waste,
        input.rezka_edge_waste,
        input.total_waste,
        input.finished_goods_kg,
        input.bobina_kg,
        input.finished_goods_meter,
        input.diameter,
    ];
    if optional_values
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite() || value <= 0.0)
    {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    Ok(())
}

fn progress_batch_correction_is_unchanged(
    current: &OrderProgressBatch,
    input: &ProgressBatchCorrectionInput,
) -> bool {
    current.produced_qty == input.produced_qty
        && current.uom.trim() == input.uom.trim()
        && current.return_ink_kg == input.return_ink_kg
        && current.lamination_print_leftover_rolls == input.lamination_print_leftover_rolls
        && current.lamination_film_leftover_rolls == input.lamination_film_leftover_rolls
        && current.rezka_bosma_waste == input.rezka_bosma_waste
        && current.rezka_lamination_waste == input.rezka_lamination_waste
        && current.rezka_edge_waste == input.rezka_edge_waste
        && current.total_waste == input.total_waste
        && current.finished_goods_kg == input.finished_goods_kg
        && current.bobina_kg == input.bobina_kg
        && current.finished_goods_meter == input.finished_goods_meter
        && current.diameter == input.diameter
        && current.description.trim() == input.description.trim()
}
