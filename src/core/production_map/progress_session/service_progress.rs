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
    fn progress_batch_ref(&self, batch_id: &str) -> Option<&OrderProgressBatch> {
        let batch_id = batch_id.trim();
        if batch_id.is_empty() {
            return None;
        }
        self.progress_batches
            .iter()
            .find(|batch| batch.batch_id.trim() == batch_id)
            .or_else(|| {
                self.input_progress_batch
                    .as_ref()
                    .filter(|batch| batch.batch_id.trim() == batch_id)
            })
    }

    fn progress_batch_for_input(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Option<&OrderProgressBatch> {
        let progress_batch_id = progress_batch_id.trim();
        let qr_payload = qr_payload.trim();
        if !progress_batch_id.is_empty() {
            self.progress_batch_ref(progress_batch_id)
        } else if !qr_payload.is_empty() {
            self.progress_batches
                .iter()
                .find(|batch| batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload))
                .or_else(|| {
                    self.input_progress_batch.as_ref().filter(|batch| {
                        batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload)
                    })
                })
        } else {
            None
        }
    }

    pub(super) fn progress_batch(&self, batch_id: &str) -> Option<OrderProgressBatch> {
        self.progress_batch_ref(batch_id).cloned()
    }

    pub(super) fn progress_batch_for_qr(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let progress_batch_id = progress_batch_id.trim();
        let qr_payload = qr_payload.trim();
        let batch = if !progress_batch_id.is_empty() || !qr_payload.is_empty() {
            self.progress_batch_for_input(progress_batch_id, qr_payload)
        } else {
            return Err(ProductionMapError::ProgressInputInvalid);
        };
        if let Some(batch) = batch {
            if !qr_payload.is_empty()
                && !batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload)
            {
                return Err(ProductionMapError::ProgressBatchNotFound);
            }
            Ok(Some(batch.clone()))
        } else {
            Ok(None)
        }
    }

    fn opening_wip_batch_ref(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Option<(&OpeningWipIntake, &OpeningWipBatch)> {
        let batch_id = batch_id.trim();
        let qr_payload = qr_payload.trim();
        self.opening_wip_records
            .iter()
            .find_map(|record| {
                record.batches.iter().find(|batch| {
                    (!batch_id.is_empty() && batch.batch_id.trim() == batch_id)
                        || (!qr_payload.is_empty()
                            && batch.qr_payload.trim() == qr_payload)
                }).map(|batch| (&record.intake, batch))
            })
            .or_else(|| {
                self.input_opening_wip_batch
                    .as_ref()
                    .filter(|record| {
                        (!batch_id.is_empty() && record.batch.batch_id.trim() == batch_id)
                            || (!qr_payload.is_empty()
                                && record.batch.qr_payload.trim() == qr_payload)
                    })
                    .map(|record| (&record.intake, &record.batch))
            })
    }

    pub(super) fn opening_wip_batch_id(&self, batch_id: &str, qr_payload: &str) -> Option<&str> {
        self.opening_wip_batch_ref(batch_id, qr_payload)
            .map(|(_, batch)| batch.batch_id.as_str())
    }

    pub(super) fn opening_wip_batch(
        &self,
        batch_id: &str,
        qr_payload: &str,
    ) -> Option<OpeningWipBatchRecord> {
        self.opening_wip_batch_ref(batch_id, qr_payload)
            .map(|(intake, batch)| OpeningWipBatchRecord {
                intake: intake.clone(),
                batch: batch.clone(),
            })
    }
}

#[derive(Clone)]
struct ProgressOutputValue {
    quantity: Option<ProgressQuantity>,
    metrics: ProgressMetrics,
    issue_note: String,
}

include!("rezka_output_report.rs");

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
        let first_healthy_index = progress.rezka_frames.iter()
            .position(|frame| frame.issue_note.trim().is_empty());
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
                            | queue_state::ApparatusQueueAction::Pause
                            | queue_state::ApparatusQueueAction::DetachRoll
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
                if Some(index) != first_healthy_index && !has_explicit_waste {
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

impl ProductionMapService {
    pub async fn progress_batch_for_qr(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        let progress_batch_id = progress_batch_id.trim();
        let qr_payload = qr_payload.trim();
        let batch = if !progress_batch_id.is_empty() {
            let batch = self.store.progress_batch(progress_batch_id).await?;
            if let Some(batch) = batch {
                if !qr_payload.is_empty()
                    && !batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload)
                {
                    return Err(ProductionMapError::ProgressBatchNotFound);
                }
                Some(batch)
            } else {
                None
            }
        } else if !qr_payload.is_empty() {
            self.store.progress_batch_by_qr(qr_payload).await?
        } else {
            return Err(ProductionMapError::ProgressInputInvalid);
        };
        let mut batch = batch.ok_or(ProductionMapError::ProgressBatchNotFound)?;
        restore_self_consumed_wip(&mut batch);
        batch.refresh_status_detail();
        Ok(batch)
    }

    pub(in crate::core::production_map) async fn previous_stage_start_progress_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let default_stage = chain::work_stage_for_station(order_map, apparatus, "")
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if progress.qr_payload.trim().is_empty() {
            if chain::previous_work_stage_for_node(order_map, &default_stage.node_id).is_none() {
                return Ok(None);
            }
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
        let preferred_stage_node_id = json_string_field(&batch.payload_json, "next_stage_node_id");
        let stage = if preferred_stage_node_id.is_empty() {
            default_stage
        } else {
            chain::work_stage_for_station(order_map, apparatus, &preferred_stage_node_id)
                .ok_or(ProductionMapError::ProgressBatchNotAccepted)?
        };
        let previous = chain::previous_work_stage_for_node(order_map, &stage.node_id)
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let previous_apparatus = previous
            .apparatus_id
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if batch.order_id.trim() != order_id
            || !super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
            || !batch.action.records_progress_output()
            || (!batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
            || (!preferred_stage_node_id.is_empty()
                && preferred_stage_node_id.trim() != stage.node_id.trim())
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        Ok(Some(batch))
    }

    pub(in crate::core::production_map) async fn opening_wip_start_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<Option<OpeningWipBatchRecord>, ProductionMapError> {
        let records = self
            .store
            .opening_wip_records(OpeningWipQuery {
                order_id: order_id.trim().to_string(),
                wip_status: None,
                limit: 10_000,
            })
            .await?;
        let opening_wip_exists = records.iter().any(|record| {
            record.intake.status == OpeningWipIntakeStatus::Confirmed
                && Self::opening_wip_target_stage(order_map, &record.intake, apparatus, "").is_some()
                && record
                    .batches
                    .iter()
                    .any(|batch| batch.wip_status == OpeningWipBatchStatus::Waiting)
        });
        if !opening_wip_exists {
            return Ok(None);
        }
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let Some(record) = self
            .store
            .opening_wip_batch(
                progress.progress_batch_id.trim(),
                progress.qr_payload.trim(),
            )
            .await?
        else {
            return Ok(None);
        };
        if record.intake.status != OpeningWipIntakeStatus::Confirmed
            || record.intake.order_id.trim() != order_id.trim()
            || record.batch.order_id.trim() != order_id.trim()
            || Self::opening_wip_target_stage(order_map, &record.intake, apparatus, "").is_none()
            || record.batch.wip_status != OpeningWipBatchStatus::Waiting
            || (!progress.progress_batch_id.trim().is_empty()
                && record.batch.batch_id.trim() != progress.progress_batch_id.trim())
            || !record
                .batch
                .qr_payload
                .trim()
                .eq_ignore_ascii_case(progress.qr_payload.trim())
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        Ok(Some(record))
    }

    pub(in crate::core::production_map) async fn previous_stage_active_progress_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
        session_id: &str,
        stage_node_id: &str,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let stage = chain::work_stage_for_station(order_map, apparatus, stage_node_id)
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let Some(previous) = chain::previous_work_stage_for_node(order_map, &stage.node_id) else {
            return Ok(None);
        };
        let previous_apparatus = previous
            .apparatus_id
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let mut batch = if let Some(read_snapshot) = read_snapshot {
            read_snapshot
                .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)?
                .ok_or(ProductionMapError::ProgressBatchNotFound)?
        } else {
            self.progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
                .await?
        };
        if read_snapshot.is_some() {
            restore_self_consumed_wip(&mut batch);
            batch.refresh_status_detail();
        }
        let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
            batch.current_apparatus.as_str()
        } else {
            batch.used_by_apparatus.as_str()
        };
        let source_wip_is_usable = batch.wip_status == OrderProgressBatchWipStatus::Waiting
            || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                && super::types::apparatus_ids_match(used_by_apparatus, apparatus)
                && (batch.used_by_session_id.trim().is_empty()
                    || batch.used_by_session_id.trim() == session_id.trim()));
        if batch.order_id.trim() != order_id
            || !super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
            || !batch.action.records_progress_output()
            || !source_wip_is_usable
            || (!batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
            || (!json_string_field(&batch.payload_json, "next_stage_node_id").is_empty()
                && json_string_field(&batch.payload_json, "next_stage_node_id")
                    != stage.node_id.trim())
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        Ok(Some(batch))
    }

    async fn recoverable_session_input_batch(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        session: &OrderRunSession,
        now: i64,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<Option<RecoveredSessionInputBatch>, ProductionMapError> {
        if let Some(read_snapshot) = read_snapshot {
            return Self::recoverable_session_input_batch_from_batches(
                apparatus,
                order_id,
                order_map,
                session,
                now,
                &read_snapshot.progress_batches,
            );
        }
        let batches = self.store.progress_batches_for_order(order_id).await?;
        Self::recoverable_session_input_batch_from_batches(
            apparatus,
            order_id,
            order_map,
            session,
            now,
            &batches,
        )
    }

    fn recoverable_session_input_batch_from_batches(
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        session: &OrderRunSession,
        now: i64,
        batches: &[OrderProgressBatch],
    ) -> Result<Option<RecoveredSessionInputBatch>, ProductionMapError> {
        let session_links = session_progress_links(session);
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &session_links.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let Some(previous) = chain::previous_work_stage_for_node(order_map, &stage.node_id) else {
            return Ok(None);
        };
        let previous_apparatus = previous
            .apparatus_id
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let linked_batch_id = session_links.batch_id;
        let mut output_candidates = batches
            .iter()
            .filter(|batch| {
                let linked_candidate = !linked_batch_id.trim().is_empty()
                    && batch.batch_id.trim() == linked_batch_id.trim();
                let unlinked_candidate = linked_batch_id.trim().is_empty()
                    && (batch.status.is_resumable()
                        || batch.wip_status == OrderProgressBatchWipStatus::InUse);
                batch.order_id.trim() == order_id.trim()
                    && batch.session_id.trim() == session.session_id.trim()
                    && batch.action.creates_resumable_output()
                    && super::types::apparatus_ids_match(&batch.apparatus, apparatus)
                    && !batch.parent_batch_id.trim().is_empty()
                    && (linked_candidate || unlinked_candidate)
            })
            .cloned()
            .collect::<Vec<_>>();
        if output_candidates.len() > 1 {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let Some(output_batch) = output_candidates.pop() else {
            return Ok(None);
        };
        let Some(parent_batch) = batches.into_iter().find(|batch| {
            batch.batch_id.trim() == output_batch.parent_batch_id.trim()
                && batch.order_id.trim() == order_id.trim()
                && super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
                && (batch.next_apparatus.trim().is_empty()
                    || chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
        }) else {
            return Ok(None);
        };
        let used_by_apparatus = if parent_batch.used_by_apparatus.trim().is_empty() {
            parent_batch.current_apparatus.as_str()
        } else {
            parent_batch.used_by_apparatus.as_str()
        };
        let owned_in_use = parent_batch.wip_status == OrderProgressBatchWipStatus::InUse
            && super::types::apparatus_ids_match(used_by_apparatus, apparatus)
            && (parent_batch.used_by_session_id.trim().is_empty()
                || parent_batch.used_by_session_id.trim() == session.session_id.trim());
        let prematurely_processed = parent_batch.wip_status
            == OrderProgressBatchWipStatus::Processed
            && super::types::apparatus_ids_match(&parent_batch.processed_by_apparatus, apparatus)
            && (parent_batch.processed_by_session_id.trim().is_empty()
                || parent_batch.processed_by_session_id.trim() == session.session_id.trim());
        if parent_batch.wip_status != OrderProgressBatchWipStatus::Waiting
            && !owned_in_use
            && !prematurely_processed
        {
            return Ok(None);
        }
        let mut input_batch =
            wip_batch_in_use(parent_batch.clone(), apparatus, &session.session_id, now);
        input_batch.payload_json["recovered_original_input_link"] = serde_json::json!(true);
        input_batch.payload_json["recovered_at_unix"] = serde_json::json!(now);
        Ok(Some(RecoveredSessionInputBatch {
            input_batch,
            output_update: restore_misbound_output_wip(output_batch, now),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn build_progress_records_with_snapshot(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        canonical: &RuntimeApparatusConfiguration,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let now = unix_seconds();
        if action == queue_state::ApparatusQueueAction::Freeze {
            return self
                .build_frozen_progress(apparatus, order_id, actor, progress, now)
                .await;
        }
        if (action == queue_state::ApparatusQueueAction::Pause && progress.worker_handoff)
            || (action == queue_state::ApparatusQueueAction::DetachRoll
                && progress.remove_roll_from_apparatus)
        {
            return self
                .build_laminatsiya_worker_transition(
                    apparatus, order_id, order_map, action, actor, progress, now, canonical,
                )
                .await;
        }
        let context = ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
        };
        match action {
            queue_state::ApparatusQueueAction::Start => {
                self.build_started_progress(context, progress).await
            }
            queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete => {
                self.build_output_progress(context, progress, read_snapshot)
                    .await
            }
            queue_state::ApparatusQueueAction::Freeze => {
                unreachable!("freeze is handled before progress action dispatch")
            }
            queue_state::ApparatusQueueAction::Resume => {
                self.build_resumed_progress(context, progress).await
            }
            queue_state::ApparatusQueueAction::Merge => {
                self.build_merged_progress(context, progress).await
            }
        }
    }

    async fn build_started_progress(
        &self,
        context: ProgressBuildContext<'_>,
        progress: QueueProgressInput,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical: _,
            now,
        } = context;
        let opening_wip_batch = self
            .opening_wip_start_batch(order_id, order_map, apparatus, &progress)
            .await?;
        let input_progress_batch = if opening_wip_batch.is_none() {
            self.previous_stage_start_progress_batch(order_id, order_map, apparatus, &progress)
                .await?
        } else {
            None
        };
        let opening_wip_target_stage_node_id = opening_wip_batch
            .as_ref()
            .and_then(|record| {
                Self::opening_wip_target_stage(order_map, &record.intake, apparatus, "")
            })
            .map(|stage| stage.node_id)
            .unwrap_or_default();
        let input_progress = input_progress_batch
            .as_ref()
            .map(progress_links_from_batch)
            .or_else(|| {
                opening_wip_batch
                    .as_ref()
                    .map(|record| {
                        progress_links_from_opening_wip(
                            record,
                            &opening_wip_target_stage_node_id,
                        )
                    })
            })
            .unwrap_or_default();
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &input_progress.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let session_payload = start_session_payload(
            actor,
            &input_progress,
            input_progress_batch.as_ref(),
            &stage.node_id,
            now,
        );
        let session = OrderRunSession {
            session_id: progress_session_id(apparatus, order_id, actor),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            stage_node_id: stage.node_id.clone(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            started_at_unix: now,
            updated_at_unix: now,
            payload_json: session_payload,
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let event = zero_quantity_event(
            context,
            String::new(),
            String::new(),
            start_event_payload(
                &input_progress,
                input_progress_batch.as_ref(),
                &stage.node_id,
            ),
        );
        let mut progress_batch_updates = Vec::new();
        if let Some(input_batch) = input_progress_batch {
            let recovered_self_consumed = input_batch
                .payload_json
                .get("recovered_self_consumed_wip")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            if recovered_self_consumed {
                for mut sibling in self.store.progress_batches_for_order(order_id).await? {
                    if repair_self_consumed_sibling_lineage(&mut sibling, &input_batch) {
                        progress_batch_updates.push(sibling);
                    }
                }
            }
            progress_batch_updates.push(wip_batch_in_use(
                input_batch,
                apparatus,
                &session.session_id,
                now,
            ));
        }
        let opening_wip_batch_updates = opening_wip_batch
            .map(|record| {
                vec![opening_wip_batch_in_use(
                    record.batch,
                    apparatus,
                    &session.session_id,
                    now,
                )]
            })
            .unwrap_or_default();
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates,
            opening_wip_batch_updates,
        })
    }
}

impl ProductionMapService {
    async fn build_output_progress(
        &self,
        context: ProgressBuildContext<'_>,
        mut progress: QueueProgressInput,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
        } = context;
        if action == queue_state::ApparatusQueueAction::RollComplete
            && !apparatus::is_rezka_apparatus(canonical)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let description = progress.description.trim().to_string();
        let session = if let Some(read_snapshot) = read_snapshot {
            read_snapshot.active_session.clone()
        } else {
            self.store
                .active_order_run_session(apparatus, order_id)
                .await?
        }
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let session_input_progress = session_progress_links(&session);
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &session_input_progress.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let session_uses_opening_wip = session_input_progress.source_kind == "opening_wip";
        if !session_uses_opening_wip
            && action != queue_state::ApparatusQueueAction::Freeze
            && apparatus::requires_previous_stage(canonical)
            && chain::previous_stage_resolution_is_unavailable(order_map, apparatus)
        {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let opening_input_batch = if session_uses_opening_wip {
            let record = if let Some(read_snapshot) = read_snapshot {
                read_snapshot.opening_wip_batch(
                    &session_input_progress.batch_id,
                    &session_input_progress.qr_payload,
                )
            } else {
                self.store
                    .opening_wip_batch(
                        &session_input_progress.batch_id,
                        &session_input_progress.qr_payload,
                    )
                    .await?
            }
                .ok_or(ProductionMapError::ProgressBatchNotFound)?;
            let explicit_batch_matches = progress.progress_batch_id.trim().is_empty()
                || record.batch.batch_id.trim() == progress.progress_batch_id.trim();
            let explicit_qr_matches = progress.qr_payload.trim().is_empty()
                || record
                    .batch
                    .qr_payload
                    .trim()
                    .eq_ignore_ascii_case(progress.qr_payload.trim());
            if record.intake.status != OpeningWipIntakeStatus::Confirmed
                || record.intake.order_id.trim() != order_id.trim()
                || record.batch.order_id.trim() != order_id.trim()
                || Self::opening_wip_target_stage(
                    order_map,
                    &record.intake,
                    apparatus,
                    &stage.node_id,
                )
                .is_none()
                || record.batch.wip_status != OpeningWipBatchStatus::InUse
                || !super::types::apparatus_ids_match(
                    &record.batch.used_by_apparatus,
                    apparatus,
                )
                || record.batch.used_by_session_id.trim() != session.session_id.trim()
                || !explicit_batch_matches
                || !explicit_qr_matches
            {
                return Err(ProductionMapError::ProgressBatchNotAccepted);
            }
            Some(record.batch)
        } else {
            None
        };
        let session_input_batch = if session_uses_opening_wip
            || session_input_progress.batch_id.trim().is_empty()
        {
            None
        } else if let Some(read_snapshot) = read_snapshot {
            read_snapshot.progress_batch(&session_input_progress.batch_id)
        } else {
            self.store
                .progress_batch(&session_input_progress.batch_id)
                .await?
        };
        let explicit_input_batch = if !session_uses_opening_wip
            && (!progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
            ) {
            self.previous_stage_active_progress_batch(
                order_id,
                order_map,
                apparatus,
                &progress,
                &session.session_id,
                &stage.node_id,
                read_snapshot,
            )
            .await?
        } else {
            None
        };
        let previous_apparatus = chain::previous_work_stage_for_node(order_map, &stage.node_id)
            .and_then(|stage| stage.apparatus_id);
        let linked_input_batch = session_input_batch
            .as_ref()
            .filter(|batch| {
                let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
                    batch.current_apparatus.as_str()
                } else {
                    batch.used_by_apparatus.as_str()
                };
                previous_apparatus.as_ref().is_some_and(|previous| {
                    batch.order_id.trim() == order_id.trim()
                        && super::types::apparatus_ids_match(&batch.apparatus, previous)
                        && (batch.next_apparatus.trim().is_empty()
                            || chain::stage_ids_match_for_map(
                                order_map,
                                &batch.next_apparatus,
                                apparatus,
                            ))
                        && (json_string_field(&batch.payload_json, "next_stage_node_id").is_empty()
                            || json_string_field(&batch.payload_json, "next_stage_node_id")
                                == stage.node_id.trim())
                        && batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && super::types::apparatus_ids_match(used_by_apparatus, apparatus)
                        && (batch.used_by_session_id.trim().is_empty()
                            || batch.used_by_session_id.trim() == session.session_id.trim())
                })
            })
            .cloned();
        let recovered_input =
            if explicit_input_batch.is_none() && linked_input_batch.is_none() {
                self.recoverable_session_input_batch(
                    apparatus,
                    order_id,
                    order_map,
                    &session,
                    now,
                    read_snapshot,
                )
                .await?
            } else {
                None
            };
        let input_batch = if let Some(batch) = explicit_input_batch {
            if linked_input_batch.as_ref().is_some_and(|session_batch| {
                session_batch.batch_id.trim() != batch.batch_id.trim()
            }) {
                return Err(ProductionMapError::ProgressBatchNotAccepted);
            }
            Some(batch)
        } else if let Some(batch) = linked_input_batch {
            Some(batch)
        } else if let Some(recovered) = recovered_input.as_ref() {
            Some(recovered.input_batch.clone())
        } else if previous_apparatus.is_some() && !session_uses_opening_wip {
            return Err(ProductionMapError::ProgressQrRequired);
        } else {
            None
        };
        let mut input_progress = input_batch
            .as_ref()
            .map(progress_links_from_batch)
            .unwrap_or_else(|| session_input_progress.clone());
        if input_progress.stage_node_id.trim().is_empty() {
            input_progress.stage_node_id = stage.node_id.clone();
        }
        if input_progress.contained_kadr_count.is_none() {
            input_progress.contained_kadr_count = session_input_progress.contained_kadr_count;
        }
        let mut output_identities = if apparatus::is_rezka_apparatus(canonical) {
            let input_lineage = order_run_input_links_from_payload(&session.payload_json)
                .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
            let active_rolls = rezka_active_partial_rolls_from_payload(&session.payload_json)
                .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
            if !rezka_merge_state_is_consistent(&input_lineage, &active_rolls) {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            if active_rolls.is_empty() {
                rezka_output_identities(
                    apparatus,
                    order_id,
                    action,
                    order_map,
                    &stage.node_id,
                    input_progress.contained_kadr_count,
                )?
            } else {
                let active_output_kadr_counts = active_rolls
                    .iter()
                    .map(|roll| {
                        usize::try_from(roll.contained_kadr_count)
                            .map_err(|_| ProductionMapError::InvalidRezkaFrameGroups)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                rezka_output_identities_from_kadr_counts(
                    apparatus,
                    order_id,
                    action,
                    order_map,
                    &stage.node_id,
                    &active_output_kadr_counts,
                )?
            }
        } else {
            vec![progress_output_identity(
                apparatus,
                order_id,
                action,
                &progress,
                &input_progress,
            )]
        };
        let report = if apparatus::is_rezka_apparatus(canonical) {
            Some(RezkaOutputReport::prepare(
                &session, &mut progress, &mut output_identities,
            )?)
        } else {
            None
        };
        let recording = progress.rezka_record_frame_index.is_some();
        let frame_values = if let Some(index) = progress.rezka_record_frame_index {
            let value = progress_values_for_outputs(
                canonical, action, &progress, &output_identities[index - 1..index], false,
            )?.remove(0);
            let mut values: Vec<_> = (0..output_identities.len()).map(|_| ProgressOutputValue {
                quantity: None, metrics: ProgressMetrics::default(), issue_note: String::new(),
            }).collect();
            if !report.as_ref().is_some_and(|report| report.is_saved(index - 1)) {
                values[index - 1] = value;
            }
            values
        } else {
            progress_values_for_outputs(
                canonical,
                action,
                &progress,
                &output_identities,
                apparatus::is_rezka_apparatus(canonical)
                    && action == queue_state::ApparatusQueueAction::Complete
                    && !chain::is_final_work_stage_node(order_map, &stage.node_id),
            )?
        };
        let frame_issues = if apparatus::is_rezka_apparatus(canonical) {
            rezka_frame_issues_json(&frame_values, output_identities.len(), &input_progress)
        } else {
            serde_json::Value::Array(Vec::new())
        };
        let first_healthy_index = frame_values
            .iter()
            .position(|value| value.quantity.is_some());
        let (session_qty, session_uom, session_metrics) =
            if let Some(index) = first_healthy_index {
                let value = &frame_values[index];
                let quantity = value.quantity.as_ref().expect("healthy output");
                (quantity.produced_qty, quantity.uom.as_str(), value.metrics)
            } else {
                // An issue has no output quantity, but closing the cycle must
                // retain any waste entered in the ordinary completion report.
                let mut metrics = ProgressMetrics::default();
                if !recording && matches!(action,
                    queue_state::ApparatusQueueAction::Complete | queue_state::ApparatusQueueAction::RollComplete)
                {
                    let valid = |value: Option<f64>| -> Result<Option<f64>, ProductionMapError> {
                        value.map(|value| crate::core::quantity::positive_erp_quantity(value)
                            .ok_or(ProductionMapError::ProgressInputInvalid)).transpose()
                    };
                    metrics.total_waste = valid(progress.total_waste)?;
                    if chain::is_final_work_stage_node(order_map, &stage.node_id)
                        || action != queue_state::ApparatusQueueAction::Complete {
                        metrics.rezka_bosma_waste = valid(progress.rezka_bosma_waste)?;
                        metrics.rezka_lamination_waste = valid(progress.rezka_lamination_waste)?;
                        metrics.rezka_edge_waste = valid(progress.rezka_edge_waste)?;
                    }
                }
                (0.0, "m", metrics)
            };
        let mut payload_json = preserve_qolip_lineage(
            &session,
            progress_session_payload(
                action,
                session_qty,
                session_uom,
                session_metrics,
                &description,
                &input_progress,
            ),
        );
        if frame_issues
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            payload_json["rezka_frame_issues"] = frame_issues.clone();
        }
        if recording {
            payload_json = session.payload_json.clone();
        }
        let mut session = OrderRunSession {
            status: run_status_for_progress_action(action),
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json,
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let mut batches = Vec::with_capacity(output_identities.len());
        for (index, identity) in output_identities.iter().enumerate() {
            if report.as_ref().is_some_and(|report| report.is_saved(index)) {
                continue;
            }
            let frame_value = if progress.rezka_frames.is_empty() {
                frame_values.first()
            } else {
                frame_values.get(index)
            };
            let Some(frame_value) = frame_value else {
                continue;
            };
            let Some(frame_quantity) = frame_value.quantity.as_ref() else {
                continue;
            };
            let mut batch = progress_batch_record(ProgressBatchRecordInput {
                order_map,
                context,
                quantity: frame_quantity,
                output_identity: identity,
                input_progress: &input_progress,
                metrics: frame_value.metrics,
                frame_gross_qty: progress
                    .rezka_frames
                    .get(if recording { 0 } else { index })
                    .and_then(|frame| frame.gross_qty),
                description: &description,
            })?;
            if apparatus::is_rezka_apparatus(canonical) {
                apply_rezka_frame_metadata(&mut batch, identity, order_map, apparatus);
                if frame_issues
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                {
                    batch.payload_json["rezka_frame_issues"] = frame_issues.clone();
                }
                if index > 0 && progress.rezka_frames.is_empty() {
                    clear_rezka_duplicate_metrics(&mut batch);
                }
            }
            batches.push(batch);
        }
        let input_was_recovered = recovered_input.is_some();
        let mut progress_batch_updates = recovered_input
            .into_iter()
            .map(|recovered| recovered.output_update)
            .collect::<Vec<_>>();
        // Printed rolls keep their identity, measurements and WIP location.
        // Completion only attaches the accounting metrics that were not known
        // at print time; optimistic batch revisions protect concurrent intake.
        if !recording && let Some(report) = &report {
            for slot in &report.saved {
                let metrics = frame_values[slot.frame_index - 1].metrics;
                if metrics.total_waste.is_some() || metrics.rezka_bosma_waste.is_some()
                    || metrics.rezka_lamination_waste.is_some() || metrics.rezka_edge_waste.is_some()
                {
                    let mut batch = self.store.progress_batch(&slot.batch_id).await?
                        .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                    batch.total_waste = metrics.total_waste;
                    batch.rezka_bosma_waste = metrics.rezka_bosma_waste;
                    batch.rezka_lamination_waste = metrics.rezka_lamination_waste;
                    batch.rezka_edge_waste = metrics.rezka_edge_waste;
                    for (key, value) in [
                        ("total_waste", metrics.total_waste),
                        ("rezka_bosma_waste", metrics.rezka_bosma_waste),
                        ("rezka_lamination_waste", metrics.rezka_lamination_waste),
                        ("rezka_edge_waste", metrics.rezka_edge_waste),
                    ] { batch.payload_json[key] = serde_json::json!(value); }
                    progress_batch_updates.push(batch);
                }
            }
        }
        if let Some(input_batch) = input_batch {
            if matches!(
                action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::DetachRoll
                    | queue_state::ApparatusQueueAction::RollComplete
            ) {
                if input_was_recovered {
                    progress_batch_updates.push(input_batch);
                }
            } else {
                let mut processed_input =
                    wip_batch_processed(input_batch, apparatus, &session.session_id, now);
                if frame_issues
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                {
                    processed_input.payload_json["rezka_frame_issues"] =
                        frame_issues.clone();
                    processed_input.payload_json["rezka_issue"] = serde_json::json!(true);
                }
                progress_batch_updates.push(processed_input);
            }
        }
        let opening_wip_batch_updates = opening_input_batch
            .and_then(|batch| {
                (!matches!(
                    action,
                    queue_state::ApparatusQueueAction::Pause
                        | queue_state::ApparatusQueueAction::DetachRoll
                        | queue_state::ApparatusQueueAction::RollComplete
                ))
                .then(|| {
                    opening_wip_batch_processed(batch, apparatus, &session.session_id, now)
                })
            })
            .into_iter()
            .collect();
        let event_output_index = frame_values.iter().enumerate().position(|(index, value)|
            value.quantity.is_some() && !report.as_ref().is_some_and(|report| report.is_saved(index)));
        let mut event = if let Some(index) = event_output_index {
            let output_identity = output_identities
                .get(index)
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            let frame_value = frame_values
                .get(index)
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            let quantity = frame_value
                .quantity
                .as_ref()
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            progress_event_record(ProgressEventRecordInput {
                context,
                quantity: quantity.clone(),
                output_identity: ProgressOutputIdentity {
                    batch_id: output_identity.batch_id.clone(),
                    qr_payload: output_identity.qr_payload.clone(),
                    frame_index: output_identity.frame_index,
                    frame_count: output_identity.frame_count,
                    contained_kadr_count: output_identity.contained_kadr_count,
                    rezka_output_kind: output_identity.rezka_output_kind,
                },
                metrics: frame_value.metrics,
                description: &description,
            })
        } else {
            let mut event = zero_quantity_event(
                context,
                String::new(),
                String::new(),
                progress_event_payload(action, ProgressMetrics::default(), &description),
            );
            event.description = description.clone();
            event
        };
        if !recording && let Some(report) = &report && !report.saved.is_empty() {
            event.payload_json["rezka_previously_recorded_batches"] = serde_json::json!(
                report.saved.iter().filter(|slot| !slot.batch_id.is_empty()).map(|slot| &slot.batch_id).collect::<Vec<_>>()
            );
            // Waste belongs to the closing report even when its first healthy
            // roll was already printed or every card was resolved as an issue.
            // Keep any newly produced roll's quantity metrics on this event.
            if first_healthy_index.is_none_or(|index| report.is_saved(index)) {
                event.total_waste = session_metrics.total_waste;
                event.rezka_bosma_waste = session_metrics.rezka_bosma_waste;
                event.rezka_lamination_waste = session_metrics.rezka_lamination_waste;
                event.rezka_edge_waste = session_metrics.rezka_edge_waste;
                for (key, value) in [
                    ("total_waste", session_metrics.total_waste),
                    ("rezka_bosma_waste", session_metrics.rezka_bosma_waste),
                    ("rezka_lamination_waste", session_metrics.rezka_lamination_waste),
                    ("rezka_edge_waste", session_metrics.rezka_edge_waste),
                ] { event.payload_json[key] = serde_json::json!(value); }
            }
        }
        if frame_issues
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            event.payload_json["rezka_frame_issues"] = frame_issues;
        }
        if batches.len() > 1 {
            event.payload_json["rezka_output_batches"] = serde_json::Value::Array(
                batches
                    .iter()
                    .map(|batch| {
                        serde_json::json!({
                            "batch_id": batch.batch_id,
                            "qr_payload": batch.qr_payload,
                            "frame_index": batch
                                .payload_json
                                .get("rezka_frame_index")
                                .and_then(serde_json::Value::as_u64),
                            "frame_count": batch
                                .payload_json
                                .get("rezka_frame_count")
                                .and_then(serde_json::Value::as_u64),
                        })
                    })
                    .collect(),
            );
        }
        if recording {
            report.as_ref().expect("validated Rezka recording")
                .finish_record(&mut session, &progress, &batches)?;
            event.payload_json["rezka_record_frame_index"] = serde_json::json!(progress.rezka_record_frame_index);
        } else {
            apply_output_boundary_to_session_payload(
                &mut session.payload_json,
                action,
                &input_progress.batch_id,
                now,
            )?;
            if report.is_some() {
                session.payload_json["rezka_output_report"] = serde_json::json!([]);
                session.payload_json["rezka_output_cycle"] = serde_json::json!(event.event_id);
                if action.creates_resumable_output()
                    && report.as_ref().is_some_and(|report| !report.saved.is_empty())
                {
                    session.payload_json["rezka_recorded_output_closed"] = serde_json::json!(true);
                }
            }
        }
        let progress_batch = batches.first().cloned();
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch,
            progress_batches: batches,
            progress_batch_updates,
            opening_wip_batch_updates,
        })
    }
}

impl ProductionMapService {
    async fn build_resumed_progress(
        &self,
        context: ProgressBuildContext<'_>,
        progress: QueueProgressInput,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
            ..
        } = context;
        let opening_record = if !progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
        {
            self.store
                .opening_wip_batch(
                    progress.progress_batch_id.trim(),
                    progress.qr_payload.trim(),
                )
                .await?
        } else {
            None
        };
        if let Some(record) = opening_record {
                let current = self
                    .store
                    .active_order_run_session(apparatus, order_id)
                    .await?
                    .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                let links = session_progress_links(&current);
                if record.intake.status != OpeningWipIntakeStatus::Confirmed
                    || record.intake.order_id.trim() != order_id.trim()
                    || record.batch.order_id.trim() != order_id.trim()
                    || record.batch.wip_status != OpeningWipBatchStatus::Waiting
                    || links.source_kind != "opening_wip"
                    || links.batch_id.trim() != record.batch.batch_id.trim()
                    || !matches!(
                        current.status,
                        OrderRunStatus::Paused | OrderRunStatus::RollDetached
                    )
                    || Self::opening_wip_target_stage(
                        order_map,
                        &record.intake,
                        apparatus,
                        &links.stage_node_id,
                    )
                    .is_none()
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                let session = OrderRunSession {
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json: resumed_handoff_session_payload(&current, &links),
                    ..current
                };
                let event = zero_quantity_event(
                    ProgressRecordContext {
                        session: &session,
                        apparatus,
                        order_id,
                        action,
                        actor,
                    },
                    record.batch.batch_id.clone(),
                    record.batch.qr_payload.clone(),
                    resume_event_payload(),
                );
                let opening_update = opening_wip_batch_in_use(
                    record.batch,
                    apparatus,
                    &session.session_id,
                    now,
                );
            return Ok(QueueProgressRecords {
                session: Some(session),
                progress_event: Some(event),
                progress_batch: None,
                progress_batches: Vec::new(),
                progress_batch_updates: Vec::new(),
                opening_wip_batch_updates: vec![opening_update],
            });
        }
        if progress.progress_batch_id.trim().is_empty()
            && progress.qr_payload.trim().is_empty()
        {
            let mut session = self
                .store
                .active_order_run_session(apparatus, order_id)
                .await?
                .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
            if !matches!(
                session.status,
                OrderRunStatus::Paused
                    | OrderRunStatus::Frozen
                    | OrderRunStatus::RollDetached
            ) {
                return Err(ProductionMapError::ProgressBatchNotResumable);
            }
            let session_input_progress = session_progress_links(&session);
            let is_requeued = session
                .payload_json
                .get("requeued_at_tail")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            let is_frozen = session.status == OrderRunStatus::Frozen
                || session
                    .payload_json
                    .get("frozen_order")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                || session
                    .payload_json
                    .get("freeze_with_issue")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                || is_requeued;
            let recorded_rezka_pause = apparatus::is_rezka_apparatus(canonical)
                && session.payload_json.get("rezka_recorded_output_closed")
                    .and_then(serde_json::Value::as_bool) == Some(true);
            if is_frozen || recorded_rezka_pause {
                let mut payload_json = std::mem::take(&mut session.payload_json);
                if !payload_json.is_object() {
                    payload_json = serde_json::json!({});
                }
                if let Some(payload) = payload_json.as_object_mut() {
                    payload.remove("requeued_at_tail");
                    payload.remove("rezka_recorded_output_closed");
                    payload.insert(
                        "resumed_after_freeze".to_string(),
                        serde_json::json!(is_frozen),
                    );
                    payload.insert(
                        "resumed_without_progress_qr".to_string(),
                        serde_json::json!(true),
                    );
                }
                let session = OrderRunSession {
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json,
                    ..session
                };
                let context = ProgressRecordContext {
                    session: &session,
                    apparatus,
                    order_id,
                    action,
                    actor,
                };
                let event = zero_quantity_event(
                    context,
                    String::new(),
                    String::new(),
                    resume_event_payload(),
                );
                return Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: None,
                    progress_batches: Vec::new(),
                    progress_batch_updates: Vec::new(),
                    opening_wip_batch_updates: Vec::new(),
                });
            }
            if session_input_progress.source_kind == "opening_wip"
                && session
                    .payload_json
                    .get("worker_handoff")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                let record = self
                    .store
                    .opening_wip_batch(
                        &session_input_progress.batch_id,
                        &session_input_progress.qr_payload,
                    )
                    .await?
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                if record.intake.status != OpeningWipIntakeStatus::Confirmed
                    || record.intake.order_id.trim() != order_id.trim()
                    || record.batch.order_id.trim() != order_id.trim()
                    || record.batch.wip_status != OpeningWipBatchStatus::InUse
                    || record.batch.used_by_session_id.trim() != session.session_id.trim()
                    || Self::opening_wip_target_stage(
                        order_map,
                        &record.intake,
                        apparatus,
                        &session_input_progress.stage_node_id,
                    )
                    .is_none()
                    || !super::types::apparatus_ids_match(
                        &record.batch.used_by_apparatus,
                        apparatus,
                    )
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                let payload_json = resumed_handoff_session_payload(
                    &session,
                    &session_input_progress,
                );
                let session = OrderRunSession {
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json,
                    ..session
                };
                let event = zero_quantity_event(
                    ProgressRecordContext {
                        session: &session,
                        apparatus,
                        order_id,
                        action,
                        actor,
                    },
                    record.batch.batch_id,
                    record.batch.qr_payload,
                    resume_event_payload(),
                );
                return Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: None,
                    progress_batches: Vec::new(),
                    progress_batch_updates: Vec::new(),
                    opening_wip_batch_updates: Vec::new(),
                });
            }
            let handoff_batch = if !session_input_progress.batch_id.trim().is_empty() {
                self.store
                    .progress_batch(&session_input_progress.batch_id)
                    .await?
                    .and_then(|batch| {
                        let is_worker_handoff = batch
                            .payload_json
                            .get("worker_handoff")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true);
                        let is_removed_roll = batch
                            .payload_json
                            .get("roll_removed_from_apparatus")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true);
                        let can_claim = is_worker_handoff
                            || (is_removed_roll
                                && batch.wip_status
                                    == OrderProgressBatchWipStatus::Waiting);
                        can_claim.then(|| {
                            wip_batch_claimed_after_handoff(
                                batch,
                                apparatus,
                                &session.session_id,
                                now,
                            )
                        })
                    })
            } else {
                None
            };
            let is_handoff = handoff_batch.is_some();
            let mut resumed_batches = if let Some(batch) = handoff_batch {
                vec![batch]
            } else {
                let mut paused_batches = self
                    .store
                    .progress_batches_for_order(order_id)
                    .await?
                    .into_iter()
                    .filter(|batch| {
                        batch.session_id.trim() == session.session_id.trim()
                            && batch.action.creates_resumable_output()
                            && batch.status.is_resumable()
                            && batch.wip_status == OrderProgressBatchWipStatus::Waiting
                            && super::types::apparatus_ids_match(
                                &batch.apparatus,
                                apparatus,
                            )
                    })
                    .collect::<Vec<_>>();
                if apparatus::is_rezka_apparatus(canonical) {
                    let source_batch_id = session_input_progress.batch_id.trim();
                    if !source_batch_id.is_empty() {
                        paused_batches.retain(|batch| {
                            batch.parent_batch_id.trim() == source_batch_id
                        });
                    }
                    if paused_batches.is_empty() {
                        return Err(ProductionMapError::ProgressBatchNotResumable);
                    }
                } else if paused_batches.len() != 1 {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                paused_batches
                    .into_iter()
                    .map(|mut batch| {
                        batch.status = OrderProgressBatchStatus::Resumed;
                        batch.payload_json = resumed_batch_payload(&batch, actor, now);
                        batch.refresh_status_detail();
                        batch
                    })
                    .collect::<Vec<_>>()
            };
            let resumed_batch = resumed_batches
                .first()
                .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
            let payload_json = if is_handoff {
                resumed_handoff_session_payload(&session, &session_input_progress)
            } else {
                resumed_session_payload(&session, resumed_batch, true)
            };
            let session = OrderRunSession {
                status: OrderRunStatus::Active,
                worker_role: actor.role.trim().to_string(),
                worker_ref: actor.ref_.trim().to_string(),
                worker_display_name: actor.display_name.trim().to_string(),
                updated_at_unix: now,
                payload_json,
                ..session
            };
            let context = ProgressRecordContext {
                session: &session,
                apparatus,
                order_id,
                action,
                actor,
            };
            let event = zero_quantity_event(
                context,
                resumed_batch.batch_id.clone(),
                resumed_batch.qr_payload.clone(),
                resume_event_payload(),
            );
            let is_rezka = apparatus::is_rezka_apparatus(canonical);
            let (progress_batch, progress_batches, progress_batch_updates) = if is_handoff {
                let resumed_batch = resumed_batches
                    .pop()
                    .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
                (
                    Some(resumed_batch.clone()),
                    Vec::new(),
                    vec![resumed_batch],
                )
            } else if is_rezka {
                (resumed_batches.first().cloned(), resumed_batches, Vec::new())
            } else {
                (resumed_batches.pop(), Vec::new(), Vec::new())
            };
            return Ok(QueueProgressRecords {
                session: Some(session),
                progress_event: Some(event),
                progress_batch,
                progress_batches,
                progress_batch_updates,
                opening_wip_batch_updates: Vec::new(),
            });
        }
        let mut batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
        if !batch.status.is_resumable()
            || !batch.action.creates_resumable_output()
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        if batch.order_id.trim() != order_id
            || !super::types::apparatus_ids_match(&batch.apparatus, apparatus)
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        batch.status = OrderProgressBatchStatus::Resumed;
        batch.payload_json = resumed_batch_payload(&batch, actor, now);
        batch.refresh_status_detail();
        let session = self
            .store
            .order_run_session(&batch.session_id)
            .await?
            .map(|session| {
                let payload_json = resumed_session_payload(&session, &batch, false);
                OrderRunSession {
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json,
                    ..session
                }
            })
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let event = zero_quantity_event(
            context,
            batch.batch_id.clone(),
            batch.qr_payload.clone(),
            resume_event_payload(),
        );
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: Some(batch),
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: Vec::new(),
        })
    }
}

impl ProductionMapService {
    async fn build_frozen_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let description = progress.description.trim().to_string();
        let session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let input_progress = session_progress_links(&session);
        let metrics = ProgressMetrics::default();
        let mut session_payload = preserve_qolip_lineage(
            &session,
            progress_session_payload(
                queue_state::ApparatusQueueAction::Freeze,
                0.0,
                &non_empty_or(&progress.uom, "kg"),
                metrics,
                &description,
                &input_progress,
            ),
        );
        session_payload["frozen_order"] = serde_json::json!(true);
        if progress.freeze_with_issue {
            session_payload["freeze_with_issue"] = serde_json::json!(true);
            session_payload["issue_note"] = serde_json::json!(&description);
        }
        let session = OrderRunSession {
            status: OrderRunStatus::Frozen,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: session_payload,
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action: queue_state::ApparatusQueueAction::Freeze,
            actor,
        };
        let mut progress_event = zero_quantity_event(
            context,
            String::new(),
            String::new(),
            progress_event_payload(
                queue_state::ApparatusQueueAction::Freeze,
                metrics,
                &description,
            ),
        );
        progress_event.description = description;
        progress_event.payload_json["frozen_order"] = serde_json::json!(true);
        if progress.freeze_with_issue {
            progress_event.payload_json["freeze_with_issue"] = serde_json::json!(true);
            let issue_note = progress_event.description.clone();
            progress_event.payload_json["issue_note"] = serde_json::json!(issue_note);
        }
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(progress_event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
        canonical: &RuntimeApparatusConfiguration,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        if !apparatus::is_laminatsiya_apparatus(canonical)
            || (progress.worker_handoff && progress.remove_roll_from_apparatus)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let remove_roll = progress.remove_roll_from_apparatus;
        let session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if (!remove_roll && session.status != OrderRunStatus::Active)
            || (remove_roll && session.status != OrderRunStatus::Paused)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let input_progress = session_progress_links(&session);
        if input_progress.batch_id.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        if input_progress.source_kind == "opening_wip" {
            return self
                .build_opening_wip_laminatsiya_worker_transition(
                    apparatus,
                    order_id,
                    order_map,
                    action,
                    actor,
                    progress,
                    now,
                    canonical,
                    session,
                    input_progress,
                )
                .await;
        }
        let input_batch = self
            .store
            .progress_batch(&input_progress.batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let previous_apparatus = chain::previous_work_stage_station(order_map, apparatus);
        let used_by_apparatus = if input_batch.used_by_apparatus.trim().is_empty() {
            input_batch.current_apparatus.as_str()
        } else {
            input_batch.used_by_apparatus.as_str()
        };
        if input_batch.order_id.trim() != order_id.trim()
            || input_batch.wip_status != OrderProgressBatchWipStatus::InUse
            || !super::types::apparatus_ids_match(used_by_apparatus, apparatus)
            || previous_apparatus.as_ref().is_some_and(|previous| {
                !super::types::apparatus_ids_match(&input_batch.apparatus, previous)
            })
            || (!input_batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(
                    order_map,
                    &input_batch.next_apparatus,
                    apparatus,
                ))
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        if remove_roll
            && input_batch
                .payload_json
                .get("worker_handoff")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let metrics = if remove_roll {
            validated_laminatsiya_removed_roll_metrics(canonical, &progress)?
        } else {
            validated_laminatsiya_worker_handoff_metrics(canonical, &progress)?
        };
        let description = progress.description.trim().to_string();
        let updated_input_batch = if remove_roll {
            wip_batch_removed_from_apparatus(
                input_batch.clone(),
                apparatus,
                metrics
                    .finished_goods_meter
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                metrics
                    .finished_goods_kg
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                metrics
                    .bobina_kg
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                now,
            )
        } else {
            wip_batch_worker_handoff(input_batch.clone(), apparatus, &session.session_id, now)
        };
        let session_payload = if remove_roll {
            removed_roll_session_payload(metrics, &description, &input_progress)
        } else {
            worker_handoff_session_payload(metrics, &description, &input_progress)
        };
        let session = OrderRunSession {
            status: if remove_roll {
                OrderRunStatus::RollDetached
            } else {
                OrderRunStatus::Paused
            },
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: preserve_qolip_lineage(&session, session_payload),
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let event = progress_metrics_event(
            context,
            input_batch.batch_id,
            input_batch.qr_payload,
            metrics,
            &description,
            if remove_roll {
                "roll_removed_from_apparatus"
            } else {
                "worker_handoff"
            },
        );
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: vec![updated_input_batch],
            opening_wip_batch_updates: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_opening_wip_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
        canonical: &RuntimeApparatusConfiguration,
        session: OrderRunSession,
        input_progress: SessionProgressLinks,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let record = self
            .store
            .opening_wip_batch(&input_progress.batch_id, &input_progress.qr_payload)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if record.intake.status != OpeningWipIntakeStatus::Confirmed
            || record.intake.order_id.trim() != order_id.trim()
            || record.batch.order_id.trim() != order_id.trim()
            || record.batch.wip_status != OpeningWipBatchStatus::InUse
            || Self::opening_wip_target_stage(
                order_map,
                &record.intake,
                apparatus,
                &input_progress.stage_node_id,
            )
            .is_none()
            || !super::types::apparatus_ids_match(
                &record.batch.used_by_apparatus,
                apparatus,
            )
            || record.batch.used_by_session_id.trim() != session.session_id.trim()
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let remove_roll = progress.remove_roll_from_apparatus;
        if remove_roll
            && session
                .payload_json
                .get("worker_handoff")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let metrics = if remove_roll {
            validated_laminatsiya_removed_roll_metrics(canonical, &progress)?
        } else {
            validated_laminatsiya_worker_handoff_metrics(canonical, &progress)?
        };
        let description = progress.description.trim().to_string();
        let session_payload = if remove_roll {
            removed_roll_session_payload(metrics, &description, &input_progress)
        } else {
            worker_handoff_session_payload(metrics, &description, &input_progress)
        };
        let session = OrderRunSession {
            status: if remove_roll {
                OrderRunStatus::RollDetached
            } else {
                OrderRunStatus::Paused
            },
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: preserve_qolip_lineage(&session, session_payload),
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let event = progress_metrics_event(
            context,
            record.batch.batch_id.clone(),
            record.batch.qr_payload.clone(),
            metrics,
            &description,
            if remove_roll {
                "roll_removed_from_apparatus"
            } else {
                "worker_handoff"
            },
        );
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: if remove_roll {
                vec![opening_wip_batch_waiting(record.batch, now)]
            } else {
                Vec::new()
            },
        })
    }
}

enum MergeInputRecord {
    Progress(OrderProgressBatch),
    Opening(OpeningWipBatchRecord),
}

impl MergeInputRecord {
    fn batch_id(&self) -> &str {
        match self {
            Self::Progress(batch) => &batch.batch_id,
            Self::Opening(record) => &record.batch.batch_id,
        }
    }

    fn links(&self, stage_node_id: &str) -> SessionProgressLinks {
        match self {
            Self::Progress(batch) => progress_links_from_batch(batch),
            Self::Opening(record) => progress_links_from_opening_wip(record, stage_node_id),
        }
    }

    fn material_balance_payload(&self, splice_waste_kg: Option<f64>) -> serde_json::Value {
        let (meter, net_kg) = match self {
            Self::Progress(batch) => (
                batch.finished_goods_meter.or_else(|| {
                    batch
                        .uom
                        .trim()
                        .eq_ignore_ascii_case("m")
                        .then_some(batch.produced_qty)
                }),
                batch.finished_goods_kg,
            ),
            Self::Opening(record) => (
                record.batch.finished_goods_meter.or_else(|| {
                    record
                        .batch
                        .uom
                        .trim()
                        .eq_ignore_ascii_case("m")
                        .then_some(record.batch.quantity)
                        .flatten()
                }),
                record.batch.finished_goods_kg,
            ),
        };
        serde_json::json!({
            "processed_input_batch_id": self.batch_id(),
            "processed_input_meter": meter,
            "processed_input_net_kg": net_kg,
            "splice_waste_kg": splice_waste_kg,
            "output_measurement_deferred": true,
            "diameter_combined": false,
        })
    }
}

fn laminatsiya_merge_contained_kadr_count(
    active: Option<usize>,
    scanned: Option<usize>,
) -> Result<Option<usize>, ProductionMapError> {
    match (active, scanned) {
        (Some(active), Some(scanned)) if active != scanned => {
            Err(ProductionMapError::MergeInputFrameCountMismatch {
                active_kadr_count: active,
                scanned_kadr_count: scanned,
            })
        }
        (Some(active), Some(_)) => Ok(Some(active)),
        (None, None) => Ok(None),
        _ => Err(ProductionMapError::MergeInputNotAccepted),
    }
}

impl ProductionMapService {
    async fn merge_input_record(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        stage_node_id: &str,
        progress: &QueueProgressInput,
    ) -> Result<MergeInputRecord, ProductionMapError> {
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::MergeInputRequired);
        }
        if let Some(record) = self
            .store
            .opening_wip_batch(
                progress.progress_batch_id.trim(),
                progress.qr_payload.trim(),
            )
            .await?
        {
            if record.intake.status != OpeningWipIntakeStatus::Confirmed
                || record.intake.order_id.trim() != order_id.trim()
                || record.batch.order_id.trim() != order_id.trim()
                || Self::opening_wip_target_stage(
                    order_map,
                    &record.intake,
                    apparatus,
                    stage_node_id,
                )
                .is_none()
                || (!progress.progress_batch_id.trim().is_empty()
                    && record.batch.batch_id.trim() != progress.progress_batch_id.trim())
            {
                return Err(ProductionMapError::MergeInputNotAccepted);
            }
            if record.batch.wip_status != OpeningWipBatchStatus::Waiting {
                return Err(ProductionMapError::MergeInputAlreadyUsed);
            }
            return Ok(MergeInputRecord::Opening(record));
        }

        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await
            .map_err(|error| match error {
                ProductionMapError::ProgressBatchNotFound => {
                    ProductionMapError::MergeInputNotAccepted
                }
                other => other,
            })?;
        let stage = chain::work_stage_for_station(order_map, apparatus, stage_node_id)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        let previous_apparatus = chain::previous_work_stage_for_node(order_map, &stage.node_id)
            .and_then(|stage| stage.apparatus_id)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        if batch.order_id.trim() != order_id.trim()
            || !super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
            || !batch.action.records_progress_output()
            || (!batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
            || (!json_string_field(&batch.payload_json, "next_stage_node_id").is_empty()
                && json_string_field(&batch.payload_json, "next_stage_node_id")
                    != stage.node_id.trim())
        {
            return Err(ProductionMapError::MergeInputNotAccepted);
        }
        if batch.wip_status != OrderProgressBatchWipStatus::Waiting {
            return Err(ProductionMapError::MergeInputAlreadyUsed);
        }
        Ok(MergeInputRecord::Progress(batch))
    }

    async fn active_merge_input_record(
        &self,
        order_id: &str,
        session: &OrderRunSession,
        input: &SessionProgressLinks,
    ) -> Result<MergeInputRecord, ProductionMapError> {
        if input.source_kind == OrderRunInputSourceKind::OpeningWip.as_str() {
            let record = self
                .store
                .opening_wip_batch(&input.batch_id, &input.qr_payload)
                .await?
                .ok_or(ProductionMapError::MergeInputNotAccepted)?;
            if record.intake.order_id.trim() != order_id.trim()
                || record.batch.order_id.trim() != order_id.trim()
                || record.batch.wip_status != OpeningWipBatchStatus::InUse
                || record.batch.used_by_session_id.trim() != session.session_id.trim()
                || !super::types::apparatus_ids_match(
                    &record.batch.used_by_apparatus,
                    &session.apparatus,
                )
            {
                return Err(ProductionMapError::MergeInputNotAccepted);
            }
            return Ok(MergeInputRecord::Opening(record));
        }

        let batch = self
            .store
            .progress_batch(&input.batch_id)
            .await?
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        if batch.order_id.trim() != order_id.trim()
            || batch.wip_status != OrderProgressBatchWipStatus::InUse
            || (!batch.used_by_session_id.trim().is_empty()
                && batch.used_by_session_id.trim() != session.session_id.trim())
            || !super::types::apparatus_ids_match(
                if batch.used_by_apparatus.trim().is_empty() {
                    &batch.current_apparatus
                } else {
                    &batch.used_by_apparatus
                },
                &session.apparatus,
            )
        {
            return Err(ProductionMapError::MergeInputNotAccepted);
        }
        Ok(MergeInputRecord::Progress(batch))
    }

    async fn build_merged_progress(
        &self,
        context: ProgressBuildContext<'_>,
        progress: QueueProgressInput,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
        } = context;
        let is_rezka = apparatus::is_rezka_apparatus(canonical);
        let is_laminatsiya = apparatus::is_laminatsiya_apparatus(canonical);
        if action != queue_state::ApparatusQueueAction::Merge
            || (!is_rezka && !is_laminatsiya)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let splice_waste_kg = match progress.total_waste {
            Some(value) if value.is_finite() && value >= 0.0 => Some(value),
            Some(_) => return Err(ProductionMapError::ProgressInputInvalid),
            None => None,
        };
        if !progress.rezka_frames.is_empty()
            || progress.produced_qty.is_some()
            || progress.gross_qty.is_some()
            || progress.return_ink_kg.is_some()
            || progress.lamination_print_leftover_rolls.is_some()
            || progress.lamination_film_leftover_rolls.is_some()
            || progress.rezka_bosma_waste.is_some()
            || progress.rezka_lamination_waste.is_some()
            || progress.rezka_edge_waste.is_some()
            || progress.finished_goods_kg.is_some()
            || progress.bobina_kg.is_some()
            || progress.finished_goods_meter.is_some()
            || progress.diameter.is_some()
            || (!progress.uom.trim().is_empty() && !progress.uom.trim().eq_ignore_ascii_case("kg"))
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let current_session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .filter(|session| session.status == OrderRunStatus::Active)
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let current_links = session_progress_links(&current_session);
        if current_links.batch_id.trim().is_empty() {
            return Err(ProductionMapError::MergeInputNotAccepted);
        }
        if (!progress.progress_batch_id.trim().is_empty()
            && progress.progress_batch_id.trim() == current_links.batch_id.trim())
            || (!progress.qr_payload.trim().is_empty()
                && progress
                    .qr_payload
                    .trim()
                    .eq_ignore_ascii_case(current_links.qr_payload.trim()))
        {
            return Err(ProductionMapError::MergeInputSame);
        }
        let current_input = self
            .active_merge_input_record(order_id, &current_session, &current_links)
            .await?;
        let next_input = self
            .merge_input_record(
                order_id,
                order_map,
                apparatus,
                &current_links.stage_node_id,
                &progress,
            )
            .await?;
        if current_input.batch_id().trim() == next_input.batch_id().trim() {
            return Err(ProductionMapError::MergeInputSame);
        }

        let next_links = next_input.links(&current_links.stage_node_id);
        let material_balance = current_input.material_balance_payload(splice_waste_kg);
        let mut payload = current_session.payload_json.clone();
        let mut input_lineage = order_run_input_links_from_payload(&payload)
            .map_err(|_| ProductionMapError::MergeInputNotAccepted)?;
        if input_lineage.is_empty() {
            let source_kind = OrderRunInputSourceKind::parse(&current_links.source_kind)
                .ok_or(ProductionMapError::MergeInputNotAccepted)?;
            input_lineage.push(OrderRunInputLink {
                input_batch_id: current_links.batch_id.clone(),
                input_qr_payload: current_links.qr_payload.clone(),
                source_apparatus: current_links.apparatus.clone(),
                source_kind,
                stage_node_id: current_links.stage_node_id.clone(),
                sequence_no: 1,
                status: OrderRunInputStatus::InUse,
                linked_at_unix: current_session.started_at_unix,
                processed_at_unix: None,
            });
            write_order_run_input_links(&mut payload, &input_lineage);
        }
        if input_lineage
            .iter()
            .any(|link| link.input_batch_id.trim() == next_links.batch_id.trim())
        {
            return Err(ProductionMapError::MergeInputAlreadyUsed);
        }
        let mut active_rolls = if is_rezka {
            rezka_active_partial_rolls_from_payload(&payload)
                .map_err(|_| ProductionMapError::MergeInputNotAccepted)?
        } else {
            Vec::new()
        };
        let merged_contained_kadr_count = if is_rezka {
            if active_rolls.is_empty() {
                let output_kadr_counts = rezka_output_kadr_counts(
                    order_map,
                    apparatus,
                    &current_links.stage_node_id,
                    current_links.contained_kadr_count,
                )?;
                initialize_rezka_active_partial_rolls(&mut payload, &output_kadr_counts, now)?;
                active_rolls = rezka_active_partial_rolls_from_payload(&payload)
                    .map_err(|_| ProductionMapError::MergeInputNotAccepted)?;
            }
            let active_output_kadr_counts = active_rolls
                .iter()
                .map(|roll| {
                    usize::try_from(roll.contained_kadr_count)
                        .map_err(|_| ProductionMapError::InvalidRezkaFrameGroups)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let next_output_kadr_counts = rezka_output_kadr_counts(
                order_map,
                apparatus,
                &current_links.stage_node_id,
                next_links.contained_kadr_count,
            )?;
            if active_output_kadr_counts != next_output_kadr_counts {
                return Err(ProductionMapError::MergeInputFrameCountMismatch {
                    active_kadr_count: active_output_kadr_counts.iter().sum(),
                    scanned_kadr_count: next_output_kadr_counts.iter().sum(),
                });
            }
            Some(next_output_kadr_counts.iter().sum::<usize>())
        } else {
            laminatsiya_merge_contained_kadr_count(
                current_links.contained_kadr_count,
                next_links.contained_kadr_count,
            )?
        };

        let current_link = input_lineage
            .iter_mut()
            .find(|link| {
                link.input_batch_id.trim() == current_links.batch_id.trim()
                    && link.status == OrderRunInputStatus::InUse
            })
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        current_link.status = OrderRunInputStatus::Processed;
        current_link.processed_at_unix = Some(now);

        let next_source_kind = OrderRunInputSourceKind::parse(&next_links.source_kind)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        let next_sequence = input_lineage
            .iter()
            .map(|link| link.sequence_no)
            .max()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        input_lineage.push(OrderRunInputLink {
            input_batch_id: next_links.batch_id.clone(),
            input_qr_payload: next_links.qr_payload.clone(),
            source_apparatus: next_links.apparatus.clone(),
            source_kind: next_source_kind,
            stage_node_id: current_links.stage_node_id.clone(),
            sequence_no: next_sequence,
            status: OrderRunInputStatus::InUse,
            linked_at_unix: now,
            processed_at_unix: None,
        });
        if is_rezka {
            for roll in &mut active_rolls {
                if !roll
                    .source_input_batch_ids
                    .iter()
                    .any(|batch_id| batch_id.trim() == next_links.batch_id.trim())
                {
                    roll.source_input_batch_ids
                        .push(next_links.batch_id.clone());
                }
                roll.updated_at_unix = now;
            }
            if !rezka_merge_state_is_consistent(&input_lineage, &active_rolls) {
                return Err(ProductionMapError::MergeInputNotAccepted);
            }
        }
        let source_input_batch_ids = input_lineage
            .iter()
            .map(|link| link.input_batch_id.clone())
            .collect::<Vec<_>>();
        write_order_run_input_links(&mut payload, &input_lineage);
        if is_rezka {
            write_rezka_active_partial_rolls(&mut payload, &active_rolls);
        }
        payload["last_action"] = serde_json::json!("merge");
        payload["input_progress_batch_id"] = serde_json::json!(next_links.batch_id);
        payload["input_progress_qr_payload"] = serde_json::json!(next_links.qr_payload);
        payload["input_progress_apparatus"] = serde_json::json!(next_links.apparatus);
        payload["input_wip_source_kind"] = serde_json::json!(next_links.source_kind);
        if let Some(contained_kadr_count) = merged_contained_kadr_count {
            payload["contained_kadr_count"] = serde_json::json!(contained_kadr_count);
        } else if let Some(object) = payload.as_object_mut() {
            object.remove("contained_kadr_count");
        }
        payload["merge_from_input_batch_id"] = serde_json::json!(current_links.batch_id);
        payload["merge_to_input_batch_id"] = serde_json::json!(next_links.batch_id);
        payload["merge_count"] = serde_json::json!(next_sequence - 1);

        let session = OrderRunSession {
            status: OrderRunStatus::Active,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: payload,
            ..current_session
        };
        let mut event = zero_quantity_event(
            ProgressRecordContext {
                session: &session,
                apparatus,
                order_id,
                action,
                actor,
            },
            next_links.batch_id.clone(),
            next_links.qr_payload.clone(),
            serde_json::json!({
                "event": "merge",
                "from_input_batch_id": current_links.batch_id,
                "to_input_batch_id": next_links.batch_id,
                "input_sequence_no": next_sequence,
                "source_input_batch_ids": source_input_batch_ids,
                "material_balance_basis": "measured_at_output",
                "material_balance": material_balance,
                "splice_waste_kg": splice_waste_kg,
                "diameter_combined": false,
            }),
        );
        event.description = progress.description.trim().to_string();
        event.total_waste = splice_waste_kg;

        let mut progress_batch_updates = Vec::new();
        let mut opening_wip_batch_updates = Vec::new();
        match current_input {
            MergeInputRecord::Progress(batch) => progress_batch_updates.push(wip_batch_processed(
                batch,
                apparatus,
                &session.session_id,
                now,
            )),
            MergeInputRecord::Opening(record) => opening_wip_batch_updates.push(
                opening_wip_batch_processed(record.batch, apparatus, &session.session_id, now),
            ),
        }
        match next_input {
            MergeInputRecord::Progress(batch) => progress_batch_updates.push(wip_batch_in_use(
                batch,
                apparatus,
                &session.session_id,
                now,
            )),
            MergeInputRecord::Opening(record) => opening_wip_batch_updates.push(
                opening_wip_batch_in_use(record.batch, apparatus, &session.session_id, now),
            ),
        }

        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates,
            opening_wip_batch_updates,
        })
    }
}

#[cfg(test)]
mod laminatsiya_merge_tests {
    use super::*;

    #[test]
    fn laminatsiya_merge_requires_matching_complete_kadr_metadata() {
        assert_eq!(
            laminatsiya_merge_contained_kadr_count(Some(2), Some(2)).unwrap(),
            Some(2)
        );
        assert_eq!(
            laminatsiya_merge_contained_kadr_count(None, None).unwrap(),
            None
        );
        assert!(matches!(
            laminatsiya_merge_contained_kadr_count(Some(2), Some(1)),
            Err(ProductionMapError::MergeInputFrameCountMismatch {
                active_kadr_count: 2,
                scanned_kadr_count: 1,
            })
        ));
        assert!(matches!(
            laminatsiya_merge_contained_kadr_count(Some(2), None),
            Err(ProductionMapError::MergeInputNotAccepted)
        ));
    }
}
