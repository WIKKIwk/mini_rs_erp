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

#[derive(Clone)]
struct ProgressOutputValue {
    quantity: Option<ProgressQuantity>,
    metrics: ProgressMetrics,
    issue_note: String,
}

fn progress_values_for_outputs(
    apparatus: &str,
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
                    apparatus,
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
        apparatus,
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
