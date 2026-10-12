use super::*;
use crate::core::apparatus_standard::ExecutionOperation;
use crate::core::quantity::positive_erp_quantity;
use serde::{Deserialize, Serialize};

fn default_report_action() -> queue_state::ApparatusQueueAction {
    queue_state::ApparatusQueueAction::Pause
}

fn is_shift_report(action: &queue_state::ApparatusQueueAction) -> bool {
    *action == queue_state::ApparatusQueueAction::Pause
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaminatsiyaAstatkaLayerInput {
    pub source_kind: String,
    pub source_id: String,
    pub expected_revision: u64,
    pub remaining_net_kg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaminatsiyaAstatkaShiftInput {
    // Omit the defaults to preserve fingerprints of existing pending Astatka requests.
    #[serde(
        default = "default_report_action",
        skip_serializing_if = "is_shift_report"
    )]
    pub action: queue_state::ApparatusQueueAction,
    #[serde(default, skip_serializing_if = "is_false")]
    pub full_completion_report_required: bool,
    pub apparatus: String,
    pub order_id: String,
    pub session_id: String,
    pub request_id: String,
    pub lamination_print_leftover_rolls: f64,
    pub lamination_film_leftover_rolls: f64,
    pub total_waste: f64,
    pub finished_goods_meter: Option<f64>,
    pub finished_goods_kg: Option<f64>,
    pub bobina_kg: Option<f64>,
    pub description: String,
    pub layer_remainders: Vec<LaminatsiyaAstatkaLayerInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaminatsiyaAstatkaCommitResult {
    #[serde(
        default = "default_report_action",
        skip_serializing_if = "is_shift_report"
    )]
    pub action: queue_state::ApparatusQueueAction,
    pub request_id: String,
    pub report: Option<LaminatsiyaAstatkaReport>,
    pub session: OrderRunSession,
    pub progress_batches: Vec<OrderProgressBatch>,
    pub remainders: Vec<LayerRemainderRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaminatsiyaAstatkaSnapshot {
    pub session: Option<OrderRunSession>,
    pub sources: Vec<LayerRemainderSource>,
    pub remainders: Vec<LayerRemainderRecord>,
    pub request_result: Option<LaminatsiyaAstatkaCommitResult>,
}

impl ProductionMapService {
    pub async fn laminatsiya_astatka_snapshot(
        &self,
        apparatus: &str,
        order_id: &str,
        actor_ref: &str,
        request_id: &str,
    ) -> Result<LaminatsiyaAstatkaSnapshot, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        // A saved request is an immutable receipt. Do not make reconciliation
        // depend on stocks that were consumed by this request or a later shift.
        if !request_id.trim().is_empty()
            && let Some((_, result)) = self
                .store
                .laminatsiya_astatka_commit_by_request_id(actor_ref.trim(), request_id.trim())
                .await?
        {
            if result.session.apparatus != apparatus || result.session.order_id != order_id {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            return Ok(LaminatsiyaAstatkaSnapshot {
                session: Some(result.session.clone()),
                sources: Vec::new(),
                remainders: result.remainders.clone(),
                request_result: Some(result),
            });
        }
        let mut sources = self.layer_remainder_sources(apparatus, order_id).await?;
        let session = self.astatka_execution_anchor(order_id, apparatus).await?;
        sources.retain(|source| {
            session
                .as_ref()
                .is_some_and(|session| source.session_id == session.session_id)
        });
        let remainders = self.layer_remainder_history(apparatus, order_id).await?;
        Ok(LaminatsiyaAstatkaSnapshot {
            session,
            sources,
            remainders,
            request_result: None,
        })
    }

    /// Record the ordinary Pause or Complete and every layer return atomically.
    pub async fn record_laminatsiya_astatka_shift(
        &self,
        mut input: LaminatsiyaAstatkaShiftInput,
        actor: QueueActionActor,
        assigned_apparatus: &[String],
    ) -> Result<LaminatsiyaAstatkaCommitResult, ProductionMapError> {
        input.apparatus = input.apparatus.trim().to_string();
        input.order_id = input.order_id.trim().to_string();
        input.session_id = input.session_id.trim().to_string();
        input.request_id = input.request_id.trim().to_string();
        input.description = input.description.trim().to_string();
        if input.apparatus.is_empty()
            || input.order_id.is_empty()
            || input.session_id.is_empty()
            || !(8..=128).contains(&input.request_id.len())
            || actor.ref_.trim().is_empty()
            || !matches!(
                input.action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::Complete
            )
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let canonical = self
            .resolve_canonical_apparatus_text(&input.apparatus)
            .await?;
        if canonical.runtime.execution_profile.operation != ExecutionOperation::Laminate {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        for value in [
            input.lamination_print_leftover_rolls,
            input.lamination_film_leftover_rolls,
            input.total_waste,
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ProductionMapError::LaminatsiyaAstatkaMetricsRequired);
            }
        }
        if input
            .bobina_kg
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err(ProductionMapError::LaminatsiyaAstatkaMetricsRequired);
        }
        if input.bobina_kg == Some(0.0) {
            input.bobina_kg = None;
        }
        let mut identities = std::collections::BTreeSet::new();
        for layer in &mut input.layer_remainders {
            layer.source_kind = layer.source_kind.trim().to_string();
            layer.source_id = layer.source_id.trim().to_string();
            if !matches!(
                layer.source_kind.as_str(),
                "progress_batch" | "raw_material"
            ) || layer.source_id.is_empty()
                || !layer.remaining_net_kg.is_finite()
                || layer.remaining_net_kg < 0.0
                || !identities.insert((layer.source_kind.clone(), layer.source_id.clone()))
            {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
        }
        input
            .layer_remainders
            .sort_by(|a, b| (&a.source_kind, &a.source_id).cmp(&(&b.source_kind, &b.source_id)));
        let request_json =
            serde_json::to_value(&input).map_err(|_| ProductionMapError::ProgressInputInvalid)?;
        let _guard = self.queue_action_guard().await;
        // Retry is checked before mutable session/source state; the original
        // operation has already paused and consumed those source identities.
        if let Some((saved_request, result)) = self
            .store
            .laminatsiya_astatka_commit_by_request_id(actor.ref_.trim(), &input.request_id)
            .await?
        {
            return if saved_request == request_json {
                Ok(result)
            } else {
                Err(ProductionMapError::ProgressInputInvalid)
            };
        }
        let order_id = self.astatka_order_id(&input.order_id).await?;
        let session = self
            .astatka_execution_anchor(&order_id, &input.apparatus)
            .await?
            .filter(|session| session.session_id == input.session_id)
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        if !matches!(
            session.status,
            OrderRunStatus::Active
                | OrderRunStatus::Paused
                | OrderRunStatus::RollDetached
                | OrderRunStatus::Completed
        ) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if session
            .payload_json
            .get("input_wip_source_kind")
            .and_then(serde_json::Value::as_str)
            == Some("opening_wip")
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let control = self.order_control_state(&order_id).await?;
        match control.state {
            OrderControlState::Active => {}
            OrderControlState::FreezeRequested => {
                return Err(ProductionMapError::OrderFreezeRequested);
            }
            OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
        }
        let sources = self
            .layer_remainder_sources(&input.apparatus, &order_id)
            .await?;
        // Explicit zero answers matter: omitting a layer must not silently
        // consume it or leave an obsolete material mounted for the next shift.
        let sources = sources
            .into_iter()
            .filter(|s| s.session_id == session.session_id)
            .collect::<Vec<_>>();
        if sources.is_empty()
            || sources.len() != input.layer_remainders.len()
            || input.layer_remainders.iter().any(|layer| {
                !sources.iter().any(|source| {
                    source.source_kind == layer.source_kind
                        && source.source_id == layer.source_id
                        && source.revision == layer.expected_revision
                })
            })
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let completing = input.action == queue_state::ApparatusQueueAction::Complete;
        if completing {
            if !input.full_completion_report_required
                && !self
                    .queue_action_controls_for_apparatus(&input.apparatus)
                    .await?
                    .get(&input.apparatus)
                    .and_then(|orders| orders.get(&order_id))
                    .is_some_and(|control| control.complete_requires_full_report)
            {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            let count = |kind: &str| {
                input
                    .layer_remainders
                    .iter()
                    .filter(|layer| layer.source_kind == kind && layer.remaining_net_kg > 0.0)
                    .count() as f64
            };
            if session.status == OrderRunStatus::Completed
                || input.lamination_print_leftover_rolls != count("progress_batch")
                || input.lamination_film_leftover_rolls != count("raw_material")
            {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
        }
        let queue_write = if session.status == OrderRunStatus::Active || completing {
            if session.worker_ref.trim() != actor.ref_.trim() {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            let zero_output = !completing
                && input.finished_goods_meter == Some(0.0)
                && input.finished_goods_kg == Some(0.0);
            if !zero_output
                && [input.finished_goods_meter, input.finished_goods_kg]
                    .iter()
                    .any(|value| value.and_then(positive_erp_quantity).is_none())
            {
                return Err(ProductionMapError::LaminatsiyaAstatkaMetricsRequired);
            }
            if !zero_output
                && input
                    .finished_goods_kg
                    .and_then(|gross| positive_erp_quantity(gross - input.bobina_kg.unwrap_or(0.0)))
                    .is_none()
            {
                return Err(ProductionMapError::LaminatsiyaAstatkaMetricsRequired);
            }
            let progress = QueueProgressInput {
                produced_qty: if zero_output {
                    None
                } else {
                    input.finished_goods_meter
                },
                gross_qty: if zero_output {
                    None
                } else {
                    input.finished_goods_kg
                },
                uom: "m".into(),
                finished_goods_meter: if zero_output {
                    None
                } else {
                    input.finished_goods_meter
                },
                finished_goods_kg: if zero_output {
                    None
                } else {
                    input.finished_goods_kg
                },
                bobina_kg: if zero_output { None } else { input.bobina_kg },
                worker_handoff: zero_output,
                force_full_completion_metrics: input.full_completion_report_required,
                lamination_layer_dispositions_attached: completing,
                lamination_print_leftover_rolls: Some(input.lamination_print_leftover_rolls),
                lamination_film_leftover_rolls: Some(input.lamination_film_leftover_rolls),
                total_waste: Some(input.total_waste),
                description: input.description.clone(),
                ..Default::default()
            };
            let prepared = self
                .prepare_apparatus_queue_action_with_progress(
                    &input.apparatus,
                    &order_id,
                    input.action,
                    assigned_apparatus,
                    actor.clone(),
                    progress,
                )
                .await?;
            Some(prepared.into_lamination_report_queue_write())
        } else {
            // A paused shift already recorded its output. Optional stale form
            // metrics cannot manufacture a second copy of the same roll.
            None
        };
        let report = if completing {
            // Complete's ordinary progress event owns the output/waste counters.
            // Adding an Astatka report here would count those values twice.
            None
        } else {
            let previous_reports = self
                .store
                .laminatsiya_astatka_reports_for_order(&order_id)
                .await?;
            let from_at_unix = previous_reports
                .iter()
                .filter(|r| r.apparatus == input.apparatus)
                .map(|r| r.to_at_unix)
                .max()
                .or(self
                    .astatka_initial_from_at(&order_id, &input.apparatus)
                    .await?)
                .ok_or(ProductionMapError::OrderNotStarted)?;
            let now = super::progress::unix_seconds();
            if now < from_at_unix {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            Some(LaminatsiyaAstatkaReport {
                report_id: format!(
                    "laminatsiya-astatka:{:032x}:{order_id}",
                    rand::random::<u128>()
                ),
                order_id: order_id.clone(),
                apparatus: input.apparatus.clone(),
                from_at_unix,
                to_at_unix: now,
                lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
                lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
                total_waste: input.total_waste,
                finished_goods_meter: if queue_write.is_some() {
                    input.finished_goods_meter
                } else {
                    None
                },
                finished_goods_kg: if queue_write.is_some() {
                    input.finished_goods_kg
                } else {
                    None
                },
                bobina_kg: if queue_write.is_some() {
                    input.bobina_kg
                } else {
                    None
                },
                worker_role: actor.role.trim().into(),
                worker_ref: actor.ref_.trim().into(),
                worker_display_name: actor.display_name.trim().into(),
                description: input.description,
                created_at_unix: now,
            })
        };
        let remainders = input
            .layer_remainders
            .into_iter()
            .map(|layer| LayerRemainderInput {
                apparatus: input.apparatus.clone(),
                order_id: order_id.clone(),
                session_id: session.session_id.clone(),
                source_kind: layer.source_kind,
                source_id: layer.source_id,
                expected_revision: layer.expected_revision,
                remaining_net_kg: layer.remaining_net_kg,
                request_id: input.request_id.clone(),
            })
            .collect();
        let result = self
            .store
            .commit_laminatsiya_astatka(
                input.request_id,
                request_json,
                report,
                session,
                actor,
                queue_write,
                remainders,
            )
            .await?;
        self.notify_live();
        Ok(result)
    }
}
