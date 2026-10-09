use std::collections::BTreeSet;

use super::super::service_progress_support::session_progress_links;
use super::super::*;
use super::snapshot_tolerance::{snapshot_rezka_output_kadr_counts, snapshot_session_lineage};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurrentRezkaOutputReport {
    pub(crate) report: serde_json::Value,
    pub(crate) kadr_counts: Vec<i64>,
    // Compare the target's authoritative inputs during unrelated live revision
    // churn without rebuilding any queue, inventory or historical snapshot.
    witness: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct RecordedFrame {
    frame_index: usize,
    batch_id: String,
    qr_payload: String,
    input: RezkaFrameProgressInput,
}

impl ProductionMapService {
    /// Read the active card cycle with keyed map/control/session lookups. This
    /// deliberately does not derive queue controls or fetch any progress batches.
    pub(crate) async fn current_rezka_output_report(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<CurrentRezkaOutputReport, ProductionMapError> {
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if !apparatus::is_rezka_apparatus(&canonical) {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let apparatus = canonical.runtime.apparatus_id.as_str();
        let (map, control, session) = tokio::try_join!(
            self.store.map_by_id(order_id),
            self.store.order_control_by_id(order_id),
            self.store.active_order_run_session(apparatus, order_id),
        )?;
        let map = map.ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
        let session = session.ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
        if session.status != OrderRunStatus::Active
            || session.order_id.trim() != order_id
            || session.apparatus.trim() != apparatus
            || control
                .as_ref()
                .is_some_and(|record| record.state != OrderControlState::Active)
        {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        if session.stage_node_id.trim().is_empty()
            && chain::linear_work_stages(&map)
                .iter()
                .filter(|stage| stage.apparatus_id.as_deref() == Some(apparatus))
                .count()
                != 1
        {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        let stage = chain::work_stage_for_station(&map, apparatus, session.stage_node_id.trim())
            .ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
        let (_, partial_rolls) =
            snapshot_session_lineage(Some(&session), true, order_id, apparatus)
                .ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
        let mut legacy_input_witness = serde_json::Value::Null;
        let kadr_counts = if partial_rolls.is_empty() {
            let input = session_progress_links(&session);
            let mut input_count = input.contained_kadr_count;
            if input_count.is_none() && (!input.batch_id.is_empty() || !input.qr_payload.is_empty())
            {
                if input.source_kind == "opening_wip" {
                    // Opening WIP carries no contained_kadr_count; its official
                    // progress path uses the map count. Still validate the one
                    // mounted opening input without enumerating the intake.
                    let record = self
                        .store
                        .opening_wip_batch(&input.batch_id, &input.qr_payload)
                        .await?
                        .ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
                    if record.batch.order_id != order_id
                        || record.batch.used_by_session_id != session.session_id
                        || record.batch.used_by_apparatus != apparatus
                    {
                        return Err(ProductionMapError::RezkaOutputCycleConflict);
                    }
                    legacy_input_witness = serde_json::to_value(record)
                        .map_err(|_| ProductionMapError::StoreFailed)?;
                } else {
                    let batch = if input.batch_id.is_empty() {
                        self.store.progress_batch_by_qr(&input.qr_payload).await?
                    } else {
                        self.store.progress_batch(&input.batch_id).await?
                    }
                    .ok_or(ProductionMapError::RezkaOutputCycleConflict)?;
                    if batch.order_id != order_id
                        || batch.used_by_session_id != session.session_id
                        || batch.used_by_apparatus != apparatus
                    {
                        return Err(ProductionMapError::RezkaOutputCycleConflict);
                    }
                    input_count = batch
                        .payload_json
                        .get("contained_kadr_count")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok())
                        .filter(|value| *value > 0);
                    legacy_input_witness =
                        serde_json::to_value(batch).map_err(|_| ProductionMapError::StoreFailed)?;
                }
            }
            snapshot_rezka_output_kadr_counts(
                &map,
                apparatus,
                &stage.node_id,
                input_count,
                order_id,
            )
            .ok_or(ProductionMapError::RezkaOutputCycleConflict)?
        } else {
            partial_rolls
                .iter()
                .map(|roll| i64::from(roll.contained_kadr_count))
                .collect()
        };
        if kadr_counts.is_empty() || kadr_counts.iter().any(|count| *count <= 0) {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        let cycle = session
            .payload_json
            .get("rezka_output_cycle")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&session.session_id);
        if cycle.trim().is_empty() {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        let frames = session
            .payload_json
            .get("rezka_output_report")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let saved: Vec<RecordedFrame> = serde_json::from_value(frames.clone())
            .map_err(|_| ProductionMapError::RezkaOutputCycleConflict)?;
        let mut indices = BTreeSet::new();
        if saved.iter().any(|slot| {
            slot.frame_index == 0
                || slot.frame_index > kadr_counts.len()
                || !indices.insert(slot.frame_index)
                || (slot.input.issue_note.trim().is_empty()
                    && (slot.batch_id.trim().is_empty() || slot.qr_payload.trim().is_empty()))
        }) || (!saved.is_empty()
            && session
                .payload_json
                .get("rezka_recorded_kadr_counts")
                .is_some_and(|counts| counts != &serde_json::json!(kadr_counts)))
        {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        let witness = serde_json::to_value((&map, &control, &session, legacy_input_witness))
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(CurrentRezkaOutputReport {
            report: serde_json::json!({"cycle_id": cycle, "frames": frames}),
            kadr_counts,
            witness,
        })
    }
}
