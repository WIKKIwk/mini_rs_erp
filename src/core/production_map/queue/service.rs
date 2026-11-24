use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::super::*;
use super::{QueueActionPolicyInput, QueueActionPolicyProfile, allowed_actions_for_control};

use super::super::apparatus::{queue_order_ids_by_projection, visible_order_ids_by_apparatus};
use super::super::chain;
use super::super::materials::{
    TrustedQolipStartValidation, build_raw_material_start_requirements_refs, live_material_rule,
};
use super::super::progress::{
    effective_apparatus_queue_policy, effective_apparatus_queue_policy_record,
};
use super::super::service::QueueProgressRecords;
use super::super::service_progress::ProgressBuildReadSnapshot;
use super::super::service_progress_support::{
    bosma_closing_output, lamination_input_was_released, session_progress_links, validate_bosma_closing_metrics,
    wip_batch_was_consumed_by_producer,
};
use super::super::service_queue_support::*;
use super::super::store_port::{ApparatusQueueStateMap, OrderControlMap};
use super::snapshot_tolerance::{
    snapshot_rezka_output_kadr_counts_projected, snapshot_session_lineage, warn_skipped_snapshot_apparatus,
    warn_skipped_snapshot_order,
};

struct StationQueueSnapshot<'a> {
    apparatus: &'a str,
    storage_key: &'a str,
    canonical: &'a crate::core::apparatus_standard::RuntimeApparatusConfiguration,
    is_rezka: bool,
    is_laminatsiya: bool,
    is_bosma: bool,
    policy: ApparatusQueuePolicy,
    stage_controls: &'a [OrderControlRecord],
    order_controls: &'a OrderControlMap,
    effective_states: &'a BTreeMap<String, queue_state::ApparatusQueueOrderState>,
    all_states: &'a ApparatusQueueStateMap,
    stage_work_by_order: &'a BTreeMap<String, Vec<StageWorkStatus>>,
    active_sessions_by_order: &'a BTreeMap<String, Vec<OrderRunSession>>,
    material_assignments_by_order: &'a HashMap<String, Vec<RawMaterialAssignment>>,
    active_order_id: Option<&'a str>,
    actionable_order_id: Option<&'a str>,
    apparatus_preflight: Option<&'a PrintPreflightHold>,
    qolip_start_settings: &'a QolipStartSettings,
}

struct SnapshotOrderOutput {
    rezka_input_lineage: Vec<OrderRunInputLink>,
    rezka_active_partial_rolls: Vec<RezkaActivePartialRoll>,
    rezka_output_kadr_counts: Vec<i64>,
}

impl StationQueueSnapshot<'_> {
    fn preferred_stage_node_id(&self, input: &QueueControlOrderInput<'_>) -> String {
        let active_stage_node_id = input.active_session
            .map(|session| session.stage_node_id.trim())
            .unwrap_or_default();
        if !active_stage_node_id.is_empty() {
            active_stage_node_id.to_string()
        } else if let Some(stage_node_id) = input.waiting_reentry_stage_node_id.clone() {
            stage_node_id
        } else {
            input
                .opening_wip_stage_node_id
                .clone()
                .or_else(|| {
                    // Once local execution is closed there is no active
                    // session. Keep its occurrence for report/visibility
                    // instead of falling back to this machine's first use.
                    self.active_sessions_by_order
                        .get(input.order_id)?
                        .iter()
                        .filter(|s| {
                            s.apparatus == self.storage_key && !s.stage_node_id.trim().is_empty()
                        })
                        .max_by(|a, b| {
                            (a.started_at_unix, &a.session_id)
                                .cmp(&(b.started_at_unix, &b.session_id))
                        })
                        .map(|s| s.stage_node_id.clone())
                })
                .unwrap_or_default()
        }
    }

    fn configure_fresh_start(
        &self,
        order_id: &str,
        interaction: &mut ApparatusQueueWorkerInteraction,
        pending_actionable: bool,
        opening_wip_mode: ApparatusQueuePreviousWipMode,
        previous_wip_mode: ApparatusQueuePreviousWipMode,
        queue_blocking_reason: &str,
    ) -> bool {
        let assignments = self.material_assignments_by_order
            .get(order_id.trim())
            .into_iter()
            .flatten()
            .filter(|assignment| {
                assignment.apparatus_id == self.canonical.runtime.apparatus_id
            })
            .collect::<Vec<_>>();
        let rule = live_material_rule(self.canonical);
        let material_requirements = build_raw_material_start_requirements_refs(
            rule.as_ref(),
            &assignments,
            &[],
            &BTreeSet::new(),
        );
        let material_scan_required = material_requirements.requires_material
            || !material_requirements.assigned_barcodes.is_empty();
        let start_materials_mode = if opening_wip_mode
            == ApparatusQueuePreviousWipMode::ScanRequired
            || (self.is_laminatsiya
                && previous_wip_mode
                    == ApparatusQueuePreviousWipMode::ScanRequired)
        {
            ApparatusQueueStartMaterialsMode::Hidden
        } else if material_scan_required {
            ApparatusQueueStartMaterialsMode::ScanRequired
        } else {
            ApparatusQueueStartMaterialsMode::Hidden
        };

        if opening_wip_mode == ApparatusQueuePreviousWipMode::Waiting {
            interaction.mode =
                ApparatusQueueInteractionMode::WaitingPreviousStage;
            interaction.opening_wip_mode = opening_wip_mode;
            interaction.blocking_reason_code =
                "waiting_opening_wip".to_string();
        } else if previous_wip_mode == ApparatusQueuePreviousWipMode::Waiting {
            interaction.mode =
                ApparatusQueueInteractionMode::WaitingPreviousStage;
            interaction.previous_wip_mode = previous_wip_mode;
            interaction.opening_wip_mode = opening_wip_mode;
            interaction.blocking_reason_code =
                "waiting_previous_stage".to_string();
        } else if !pending_actionable {
            interaction.mode = ApparatusQueueInteractionMode::FreshStartBlocked;
            interaction.blocking_reason_code =
                queue_blocking_reason.to_string();
        } else {
            interaction.start_materials_mode = start_materials_mode;
            interaction.material_scan_required = start_materials_mode
                == ApparatusQueueStartMaterialsMode::ScanRequired;
            interaction.assigned_materials_display_only = false;
            interaction.previous_wip_mode = previous_wip_mode;
            interaction.qolip_mode =
                if apparatus::requires_qolip_scan(self.canonical) {
                    ApparatusQueueQolipMode::ScanRequired
                } else {
                    ApparatusQueueQolipMode::NotRequired
                };
            if interaction.qolip_mode == ApparatusQueueQolipMode::ScanRequired {
                interaction.qolip_automatic_confirmation = self.qolip_start_settings.automatic_confirmation_enabled;
                interaction.qolip_policy_revision = self.qolip_start_settings.revision;
            }
            if material_requirements.assignments_satisfied {
                interaction.mode = ApparatusQueueInteractionMode::FreshStart;
                return true;
            } else {
                interaction.mode =
                    ApparatusQueueInteractionMode::FreshStartBlocked;
                interaction.blocking_reason_code =
                    "raw_material_assignment_required".to_string();
            }
        }
        false
    }

    fn order_output(
        &self,
        input: &QueueControlOrderInput<'_>,
        stage_node_id: &str,
        projected_input_count: Option<usize>,
    ) -> Option<SnapshotOrderOutput> {
        let order_map = input.order_map;
        let active_session = input.active_session;
        let order_id = input.order_id;
        let projection = input.projection;
        // Corrupt session payloads must only hide one order's controls.
        // The write path revalidates everything, so the snapshot keeps
        // serving the remaining orders instead of erroring globally.
        let (rezka_input_lineage, rezka_active_partial_rolls) =
            if self.is_rezka || self.is_laminatsiya {
                snapshot_session_lineage(
                    active_session,
                    self.is_rezka,
                    order_id.trim(),
                    self.storage_key,
                )?
            } else {
                (Vec::new(), Vec::new())
            };
        let input_contained_kadr_count = active_session
            .and_then(|session| session.payload_json.get("contained_kadr_count"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .or(projected_input_count);
        let rezka_output_kadr_counts = if !rezka_active_partial_rolls.is_empty() {
            rezka_active_partial_rolls
                .iter()
                .map(|roll| i64::from(roll.contained_kadr_count))
                .collect()
        } else if self.is_rezka && !order_map.products.is_empty() {
            let lanes = active_session.map(|s| super::super::layout::payload_lanes(&s.payload_json)).unwrap_or_default();
            match super::super::layout::cut_counts(order_map, stage_node_id, &lanes) {
                Ok(counts) => counts.into_iter().map(|n| n as i64).collect(),
                Err(_) => return None,
            }
        } else if self.is_rezka {
            snapshot_rezka_output_kadr_counts_projected(
                projection,
                self.storage_key,
                stage_node_id,
                input_contained_kadr_count,
                order_id.trim(),
            )?
        } else {
            Vec::new()
        };
        Some(SnapshotOrderOutput {
            rezka_input_lineage,
            rezka_active_partial_rolls,
            rezka_output_kadr_counts,
        })
    }
}

include!("service_parts/apparatus_sequences.rs");

include!("service_parts/queue_action_controls_for_apparatus.rs");

include!("service_parts/snapshot_order_action_control.rs");

include!("service_parts/apply_apparatus_queue_action_with_progress.rs");
