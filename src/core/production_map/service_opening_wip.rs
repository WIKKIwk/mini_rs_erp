use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::apparatus_standard::ExecutionOperation;

use super::*;

const MAX_OPENING_WIP_BATCHES: usize = 500;

impl ProductionMapService {
    pub async fn create_opening_wip(
        &self,
        input: OpeningWipCreateInput,
        actor: QueueActionActor,
    ) -> Result<OpeningWipRecord, ProductionMapError> {
        let mut normalized = normalize_opening_wip_input(input)?;
        let map = self
            .store
            .maps()
            .await?
            .into_iter()
            .find(|map| map.id.trim() == normalized.order_id)
            .ok_or(ProductionMapError::MapNotFound)?;
        let first_apparatus = chain::linear_work_stages(&map)
            .into_iter()
            .find_map(|stage| stage.apparatus_id)
            .ok_or(ProductionMapError::OpeningWipEntryMismatch)?;
        if !types::apparatus_ids_match(&first_apparatus, &normalized.entry_apparatus) {
            return Err(ProductionMapError::OpeningWipEntryMismatch);
        }
        let location_stage = chain::linear_work_stages(&map)
            .into_iter()
            .find(|stage| {
                stage.apparatus_id.as_deref().is_some_and(|apparatus_id| {
                    types::apparatus_ids_match(apparatus_id, &normalized.current_location)
                })
            })
            .ok_or(ProductionMapError::OpeningWipLocationMismatch)?;
        normalized.current_location = if location_stage.station_title.trim().is_empty() {
            location_stage.apparatus_id.unwrap_or_default()
        } else {
            location_stage.station_title.trim().to_string()
        };
        let entry_configuration = self
            .resolve_canonical_apparatus_text(&normalized.entry_apparatus)
            .await?;
        let operation = entry_configuration.runtime.execution_profile.operation;
        validate_opening_wip_batches(&normalized.batches, operation)?;
        normalized.source_operation = "unavailable_before_cutover".to_string();
        normalized.source_apparatus.clear();

        let fingerprint = opening_wip_fingerprint(&normalized);
        if let Some(existing) = self
            .store
            .opening_wip_by_idempotency_key(&normalized.idempotency_key)
            .await?
        {
            if existing.intake.request_fingerprint == fingerprint {
                return Ok(existing);
            }
            return Err(ProductionMapError::OpeningWipIdempotencyConflict);
        }
        let queue_states = self.store.apparatus_queue_states().await?;
        let already_started = queue_states.values().any(|states| {
            states.get(&normalized.order_id).is_some_and(|state| {
                !state.trim().is_empty() && !state.trim().eq_ignore_ascii_case("pending")
            })
        });
        if already_started {
            return Err(ProductionMapError::OpeningWipOrderAlreadyStarted);
        }

        let now = opening_wip_unix_seconds();
        let stamp = opening_wip_unix_nanos();
        let intake_id = format!(
            "opening-wip:{stamp}:{}",
            opening_wip_sanitize_id(&normalized.order_id)
        );
        let batches = normalized
            .batches
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let sequence_no = i32::try_from(index + 1)
                    .map_err(|_| ProductionMapError::OpeningWipInvalidInput)?;
                let batch_id = format!("{intake_id}:roll-{sequence_no}");
                Ok(OpeningWipBatch {
                    qr_payload: progress_qr_payload(&batch_id),
                    batch_id,
                    intake_id: intake_id.clone(),
                    order_id: normalized.order_id.clone(),
                    sequence_no,
                    quantity_basis: input.quantity_basis,
                    quantity: input.finished_goods_meter,
                    uom: "m".to_string(),
                    finished_goods_meter: input.finished_goods_meter,
                    finished_goods_kg: input.finished_goods_kg,
                    bobina_kg: input.bobina_kg,
                    diameter: input.diameter,
                    wip_status: OpeningWipBatchStatus::Waiting,
                    used_by_session_id: String::new(),
                    used_by_apparatus: String::new(),
                    processed_by_session_id: String::new(),
                    processed_by_apparatus: String::new(),
                    label_item_code: map.product_code.trim().to_string(),
                    label_item_name: map.title.trim().to_string(),
                    created_at_unix: now,
                    updated_at_unix: now,
                })
            })
            .collect::<Result<Vec<_>, ProductionMapError>>()?;
        let record = OpeningWipRecord {
            intake: OpeningWipIntake {
                intake_id,
                idempotency_key: normalized.idempotency_key,
                request_fingerprint: fingerprint,
                order_id: normalized.order_id,
                entry_apparatus: normalized.entry_apparatus,
                source_operation: normalized.source_operation,
                source_apparatus: normalized.source_apparatus,
                current_location: normalized.current_location,
                history_status: "unavailable_before_cutover".to_string(),
                status: OpeningWipIntakeStatus::Confirmed,
                note: normalized.note,
                actor,
                created_at_unix: now,
                updated_at_unix: now,
            },
            batches,
        };
        let saved = self
            .store
            .create_opening_wip(OpeningWipCreateWrite {
                record: record.clone(),
            })
            .await?;
        self.notify_live();
        Ok(saved)
    }

    pub async fn opening_wip_records(
        &self,
        query: OpeningWipQuery,
    ) -> Result<Vec<OpeningWipRecord>, ProductionMapError> {
        self.store.opening_wip_records(query).await
    }

    pub async fn opening_wip_batch(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Result<OpeningWipBatchRecord, ProductionMapError> {
        if batch_id.trim().is_empty() && qr_payload.trim().is_empty() {
            return Err(ProductionMapError::OpeningWipInvalidInput);
        }
        self.store
            .opening_wip_batch(batch_id.trim(), qr_payload.trim())
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)
    }
}

fn normalize_opening_wip_input(
    input: OpeningWipCreateInput,
) -> Result<OpeningWipCreateInput, ProductionMapError> {
    let normalized = OpeningWipCreateInput {
        idempotency_key: input.idempotency_key.trim().to_string(),
        order_id: input.order_id.trim().to_string(),
        entry_apparatus: input.entry_apparatus.trim().to_string(),
        source_operation: input.source_operation.trim().to_ascii_lowercase(),
        source_apparatus: input.source_apparatus.trim().to_string(),
        current_location: input.current_location.trim().to_string(),
        note: input.note.trim().to_string(),
        batches: input
            .batches
            .into_iter()
            .map(|batch| OpeningWipBatchInput {
                quantity_basis: batch.quantity_basis,
                finished_goods_meter: batch.finished_goods_meter,
                finished_goods_kg: batch.finished_goods_kg,
                bobina_kg: batch.bobina_kg,
                diameter: batch.diameter,
            })
            .collect(),
    };
    if normalized.idempotency_key.is_empty()
        || normalized.order_id.is_empty()
        || normalized.entry_apparatus.is_empty()
        || normalized.current_location.is_empty()
        || normalized.batches.is_empty()
        || normalized.batches.len() > MAX_OPENING_WIP_BATCHES
    {
        return Err(ProductionMapError::OpeningWipInvalidInput);
    }
    for batch in &normalized.batches {
        for value in [
            batch.finished_goods_meter,
            batch.finished_goods_kg,
            batch.bobina_kg,
            batch.diameter,
        ] {
            if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(ProductionMapError::OpeningWipInvalidInput);
            }
        }
    }
    Ok(normalized)
}

fn validate_opening_wip_batches(
    batches: &[OpeningWipBatchInput],
    operation: ExecutionOperation,
) -> Result<(), ProductionMapError> {
    let requires_diameter = operation == ExecutionOperation::Cut;
    for batch in batches {
        if batch.quantity_basis == OpeningWipQuantityBasis::Unknown
            || batch.finished_goods_meter.is_none()
            || batch.finished_goods_kg.is_none()
            || batch.bobina_kg.is_none()
            || (requires_diameter && batch.diameter.is_none())
            || (!requires_diameter && batch.diameter.is_some())
        {
            return Err(ProductionMapError::OpeningWipInvalidInput);
        }
    }
    Ok(())
}

fn opening_wip_fingerprint(input: &OpeningWipCreateInput) -> String {
    serde_json::to_string(input).unwrap_or_default()
}

fn opening_wip_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

fn opening_wip_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default()
}

fn opening_wip_sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}
