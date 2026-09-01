use super::*;

use super::progress::{non_empty_or, progress_session_id, unix_seconds};
use super::service::QueueProgressRecords;
use super::service_progress_metrics::{
    ProgressMetrics, validated_laminatsiya_removed_roll_metrics,
    validated_laminatsiya_worker_handoff_metrics, validated_progress_metrics,
};
use super::service_progress_support::*;
use crate::core::apparatus_standard::RuntimeApparatusConfiguration;

struct RecoveredSessionInputBatch {
    input_batch: OrderProgressBatch,
    output_update: OrderProgressBatch,
}

pub(super) struct ProgressBuildReadSnapshot {
    pub(super) active_session: Option<OrderRunSession>,
    pub(super) progress_batches: Vec<OrderProgressBatch>,
    pub(super) opening_wip_records: Vec<OpeningWipRecord>,
    pub(super) input_progress_batch: Option<OrderProgressBatch>,
    pub(super) input_opening_wip_batch: Option<OpeningWipBatchRecord>,
}

impl ProgressBuildReadSnapshot {
    pub(super) fn progress_batch(&self, batch_id: &str) -> Option<OrderProgressBatch> {
        let batch_id = batch_id.trim();
        if batch_id.is_empty() {
            return None;
        }
        self.progress_batches
            .iter()
            .find(|batch| batch.batch_id.trim() == batch_id)
            .cloned()
            .or_else(|| {
                self.input_progress_batch
                    .as_ref()
                    .filter(|batch| batch.batch_id.trim() == batch_id)
                    .cloned()
            })
    }

    pub(super) fn progress_batch_for_qr(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let progress_batch_id = progress_batch_id.trim();
        let qr_payload = qr_payload.trim();
        let batch = if !progress_batch_id.is_empty() {
            self.progress_batch(progress_batch_id)
        } else if !qr_payload.is_empty() {
            self.progress_batches
                .iter()
                .find(|batch| batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload))
                .cloned()
                .or_else(|| {
                    self.input_progress_batch
                        .as_ref()
                        .filter(|batch| {
                            batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload)
                        })
                        .cloned()
                })
        } else {
            return Err(ProductionMapError::ProgressInputInvalid);
        };
        if let Some(batch) = batch {
            if !qr_payload.is_empty()
                && !batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload)
            {
                return Err(ProductionMapError::ProgressBatchNotFound);
            }
            Ok(Some(batch))
        } else {
            Ok(None)
        }
    }

    pub(super) fn opening_wip_batch(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Option<OpeningWipBatchRecord> {
        let batch_id = batch_id.trim();
        let qr_payload = qr_payload.trim();
        self.opening_wip_records
            .iter()
            .find_map(|record| {
                record.batches.iter().find(|batch| {
                    (!batch_id.is_empty() && batch.batch_id.trim() == batch_id)
                        || (!qr_payload.is_empty()
                            && batch.qr_payload.trim() == qr_payload)
                }).map(|batch| OpeningWipBatchRecord {
                    intake: record.intake.clone(),
                    batch: batch.clone(),
                })
            })
            .or_else(|| {
                self.input_opening_wip_batch
                    .as_ref()
                    .filter(|record| {
                        (!batch_id.is_empty() && record.batch.batch_id.trim() == batch_id)
                            || (!qr_payload.is_empty()
                                && record.batch.qr_payload.trim() == qr_payload)
                    })
                    .cloned()
            })
    }
}

#[derive(Clone)]
struct ProgressOutputValue {
    quantity: Option<ProgressQuantity>,
    metrics: ProgressMetrics,
    issue_note: String,
}

fn progress_values_for_outputs(
    canonical: &RuntimeApparatusConfiguration,
    action: queue_state::ApparatusQueueAction,
    progress: &QueueProgressInput,
    output_identities: &[ProgressOutputIdentity],
    rezka_total_waste_only_completion: bool,
) -> Result<Vec<ProgressOutputValue>, ProductionMapError> {
    if apparatus::is_rezka_apparatus(canonical) && !progress.rezka_frames.is_empty() {
        if progress.rezka_frames.len() != output_identities.len() {
            return Err(ProductionMapError::RezkaFrameCountMismatch);
        }
        return progress
            .rezka_frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let issue_note = frame.issue_note.trim();
                if !issue_note.is_empty() {
                    if !matches!(
                        action,
                        queue_state::ApparatusQueueAction::RollComplete
                            | queue_state::ApparatusQueueAction::Complete
                    ) {
                        return Err(ProductionMapError::ProgressInputInvalid);
                    }
                    return Ok(ProgressOutputValue {
                        quantity: None,
                        metrics: ProgressMetrics::default(),
                        issue_note: issue_note.to_string(),
                    });
                }
                let has_explicit_waste = frame.has_explicit_waste();
                let frame_progress = frame.to_queue_progress(progress, !has_explicit_waste);
                let mut metrics = validated_progress_metrics(
                    canonical,
                    action,
                    &frame_progress,
                    rezka_total_waste_only_completion,
                )?;
                if index > 0 && !has_explicit_waste {
                    metrics.rezka_bosma_waste = None;
                    metrics.rezka_lamination_waste = None;
                    metrics.rezka_edge_waste = None;
                    metrics.total_waste = None;
                }
                let quantity = progress_quantity(&frame_progress, metrics)?;
                Ok(ProgressOutputValue {
                    quantity: Some(quantity),
                    metrics,
                    issue_note: String::new(),
                })
            })
            .collect();
    }

    let metrics = validated_progress_metrics(
        canonical,
        action,
        progress,
        rezka_total_waste_only_completion,
    )?;
    let quantity = progress_quantity(progress, metrics)?;
    Ok(vec![ProgressOutputValue {
        quantity: Some(quantity),
        metrics,
        issue_note: String::new(),
    }])
}

fn rezka_frame_issues_json(
    values: &[ProgressOutputValue],
    frame_count: usize,
    input_progress: &SessionProgressLinks,
) -> serde_json::Value {
    serde_json::Value::Array(
        values
            .iter()
            .enumerate()
            .filter(|(_, value)| !value.issue_note.trim().is_empty())
            .map(|(index, value)| {
                serde_json::json!({
                    "frame_index": index + 1,
                    "frame_count": frame_count,
                    "issue_note": value.issue_note.trim(),
                    "input_progress_batch_id": input_progress.batch_id,
                    "input_progress_apparatus": input_progress.apparatus,
                })
            })
            .collect(),
    )
}


#[derive(Clone, Copy)]
struct ProgressBuildContext<'a> {
    apparatus: &'a str,
    order_id: &'a str,
    order_map: &'a ProductionMapDefinition,
    action: queue_state::ApparatusQueueAction,
    actor: &'a QueueActionActor,
    canonical: &'a RuntimeApparatusConfiguration,
    now: i64,
}

include!("service_progress_impl_parts/part_01.rs");
include!("service_progress_impl_parts/part_02.rs");
include!("service_progress_impl_parts/part_03.rs");
include!("service_progress_impl_parts/part_04.rs");
include!("service_progress_impl_parts/part_05.rs");
