use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::calculate_orders::{
    owner_key, validate_template, CalculateOrderError, CalculateOrderTemplate,
};
use crate::core::production_map::{
    queue_state, ApparatusQueueOrderActionControl, ApparatusQueuePolicy,
    ApparatusQueuePolicyRecord, ProductionMapDefinition, ProductionMapLiveSnapshot,
    ProductionMapNodeKind, ProductionMapSaved, ProductionOrderStatusDetail,
};
use crate::db::postgres_training_workspace::{
    PostgresTrainingWorkspaceStore, TrainingImage, TrainingWorkspaceError,
};

#[derive(Default, Deserialize)]
pub struct TrainingMapsQuery {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize)]
struct TrainingMapSaveWithOrderRequest {
    map: ProductionMapDefinition,
    template: CalculateOrderTemplate,
}

#[derive(Default, Deserialize)]
struct TrainingApparatusModeInput {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Default, Deserialize)]
struct TrainingRestartInput {
    #[serde(default)]
    apparatus: String,
}

#[derive(Default, Deserialize)]
pub struct TrainingRawMaterialAssignmentsQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
}

#[derive(Default)]
pub(super) struct WorkerTrainingOverlay {
    pub active_apparatuses: Vec<String>,
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub visible_order_ids: BTreeMap<String, Vec<String>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub queue_action_controls:
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub order_statuses: BTreeMap<String, ProductionOrderStatusDetail>,
    pub order_customers: BTreeMap<String, String>,
}

#[derive(Default)]
pub(super) struct TrainingQueuePrintInput {
    pub driver_url: String,
    pub printer: String,
    pub print_mode: String,
    pub print_transport: String,
    pub progress_qty: Option<f64>,
    pub gross_qty: Option<f64>,
    pub finished_goods_kg: Option<f64>,
    pub bobina_kg: Option<f64>,
    pub uom: String,
    pub customer_name: String,
    pub print_count: u32,
}

pub(super) async fn worker_training_overlay(
    state: &AppState,
    principal: &Principal,
) -> Result<WorkerTrainingOverlay, TrainingWorkspaceError> {
    if !matches!(&principal.role, PrincipalRole::Aparatchi) {
        return Ok(WorkerTrainingOverlay::default());
    }
    let Some(store) = state.training_workspace.as_ref() else {
        return Ok(WorkerTrainingOverlay::default());
    };
    let assigned_apparatus = state.admin.principal_assigned_apparatus(principal).await;
    let modes = store.apparatus_modes().await?;
    let active_apparatuses = assigned_apparatus
        .into_iter()
        .filter(|apparatus| {
            modes.iter().any(|(configured, enabled)| {
                *enabled && queue_state::apparatus_titles_match(configured, apparatus)
            })
        })
        .map(|apparatus| apparatus.trim().to_string())
        .filter(|apparatus| !apparatus.is_empty())
        .collect::<Vec<_>>();
    if active_apparatuses.is_empty() {
        return Ok(WorkerTrainingOverlay::default());
    }

    let all_maps = store.maps().await?;
    let maps = all_maps
        .into_iter()
        .filter(|saved| {
            active_apparatuses
                .iter()
                .any(|apparatus| training_map_has_apparatus(saved, apparatus))
        })
        .collect::<Vec<_>>();
    let stored_states = store.queue_states().await?;
    let mut overlay = WorkerTrainingOverlay {
        active_apparatuses,
        maps,
        ..WorkerTrainingOverlay::default()
    };

    for apparatus in &overlay.active_apparatuses {
        let visible_order_ids = overlay
            .maps
            .iter()
            .filter(|saved| training_map_has_apparatus(saved, apparatus))
            .map(|saved| saved.map.id.trim().to_string())
            .filter(|order_id| !order_id.is_empty())
            .collect::<Vec<_>>();
        let sequence = queue_state::effective_apparatus_sequence(&[], &visible_order_ids);
        let visible_set = visible_order_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut states = BTreeMap::new();
        for (stored_apparatus, stored) in &stored_states {
            if !queue_state::apparatus_titles_match(stored_apparatus, apparatus) {
                continue;
            }
            for (order_id, state) in stored {
                if visible_set.contains(order_id) {
                    states.insert(order_id.clone(), state.clone());
                }
            }
        }
        let controls = training_queue_action_controls(&sequence, &states);
        let statuses = sequence
            .iter()
            .map(|order_id| {
                let state = controls
                    .get(order_id)
                    .map(|control| control.state)
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
                (
                    order_id.clone(),
                    training_order_status(state),
                )
            })
            .collect::<BTreeMap<_, _>>();
        overlay
            .sequences
            .insert(apparatus.clone(), sequence.clone());
        overlay
            .visible_order_ids
            .insert(apparatus.clone(), visible_order_ids);
        overlay
            .queue_states
            .insert(apparatus.clone(), states);
        overlay
            .queue_action_controls
            .insert(apparatus.clone(), controls);
        overlay.queue_policies.push(ApparatusQueuePolicyRecord {
            apparatus: apparatus.clone(),
            policy: ApparatusQueuePolicy::StrictSequence,
            locked: true,
            reason: "training_mode".to_string(),
        });
        overlay.order_statuses.extend(statuses);
    }
    overlay.order_customers = overlay
        .maps
        .iter()
        .filter_map(|saved| {
            let order_id = saved.map.id.trim();
            let customer = saved.map.customer_name.trim();
            (!order_id.is_empty() && !customer.is_empty())
                .then(|| (order_id.to_string(), customer.to_string()))
        })
        .collect();
    Ok(overlay)
}

pub(super) async fn training_map_for_principal(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
) -> Result<Option<ProductionMapSaved>, TrainingWorkspaceError> {
    let order_id = order_id.trim();
    if !order_id.starts_with("training-") {
        return Ok(None);
    }
    let store = state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    let apparatus = apparatus.trim();
    if matches!(&principal.role, PrincipalRole::Aparatchi) {
        let overlay = worker_training_overlay(state, principal).await?;
        let Some(active_apparatus) = overlay
            .active_apparatuses
            .iter()
            .find(|candidate| {
                !apparatus.is_empty()
                    && queue_state::apparatus_titles_match(candidate, apparatus)
            })
            else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        let Some(saved) = overlay.maps.iter().find(|saved| {
            saved.map.id.trim() == order_id
                && training_map_has_apparatus(saved, active_apparatus)
        }) else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        Ok(Some(saved.clone()))
    } else {
        let Some(saved) = store.map(order_id).await? else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        if !apparatus.is_empty() && !training_map_has_apparatus(&saved, apparatus) {
            return Err(TrainingWorkspaceError::MapNotFound);
        }
        Ok(Some(saved))
    }
}

pub(super) async fn training_material_assignments_for_principal(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
) -> Result<Option<Vec<serde_json::Value>>, TrainingWorkspaceError> {
    let Some(_) = training_map_for_principal(state, principal, order_id, apparatus).await? else {
        return Ok(None);
    };
    let store = state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    let order_id = order_id.trim();
    let apparatus = apparatus.trim();
    Ok(Some(
        store.raw_material_assignments(order_id, apparatus).await?,
    ))
}

pub(super) async fn training_raw_material_start_requirements(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
    material_barcodes: &str,
) -> Result<Option<serde_json::Value>, TrainingWorkspaceError> {
    let Some(assignments) = training_material_assignments_for_principal(
        state,
        principal,
        order_id,
        apparatus,
    )
    .await?
    else {
        return Ok(None);
    };
    let assigned_barcodes = assignments
        .iter()
        .filter_map(|assignment| assignment.get("barcode"))
        .filter_map(serde_json::Value::as_str)
        .map(normalize_training_barcode)
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>();
    let scanned_barcodes = material_barcodes
        .split(',')
        .map(normalize_training_barcode)
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>();
    let matched_scan_count = scanned_barcodes.intersection(&assigned_barcodes).count();
    let scan_satisfied = assigned_barcodes.is_empty()
        || (!scanned_barcodes.is_empty()
            && scanned_barcodes.is_subset(&assigned_barcodes)
            && scanned_barcodes == assigned_barcodes);
    let assigned_barcodes = assigned_barcodes.into_iter().collect::<Vec<_>>();
    Ok(Some(serde_json::json!({
        "policy": "state_all",
        "requires_material": !assigned_barcodes.is_empty(),
        "requirement_groups": [],
        "assigned_barcodes": assigned_barcodes.clone(),
        "staged_barcodes": assigned_barcodes.clone(),
        "eligible_barcodes": assigned_barcodes.clone(),
        "required_scan_count": assigned_barcodes.len(),
        "matched_scan_count": matched_scan_count,
        "assignments_satisfied": true,
        "scan_satisfied": scan_satisfied,
        "assignments": assignments.clone(),
        "start_assignments": assignments,
    })))
}

fn normalize_training_barcode(barcode: &str) -> String {
    barcode.trim().to_ascii_uppercase()
}

pub(super) async fn merge_worker_training_maps(
    state: &AppState,
    principal: &Principal,
    maps: &mut Vec<ProductionMapSaved>,
) -> Result<(), TrainingWorkspaceError> {
    let overlay = worker_training_overlay(state, principal).await?;
    if overlay.active_apparatuses.is_empty() {
        return Ok(());
    }
    maps.retain(|saved| {
        !overlay
            .active_apparatuses
            .iter()
            .any(|apparatus| training_map_has_apparatus(saved, apparatus))
    });
    maps.extend(overlay.maps);
    Ok(())
}

pub(super) async fn merge_worker_training_snapshot(
    state: &AppState,
    principal: &Principal,
    snapshot: &mut ProductionMapLiveSnapshot,
) -> Result<(), TrainingWorkspaceError> {
    let overlay = worker_training_overlay(state, principal).await?;
    if overlay.active_apparatuses.is_empty() {
        return Ok(());
    }

    let hidden_order_ids = snapshot
        .maps
        .iter()
        .filter(|saved| {
            overlay
                .active_apparatuses
                .iter()
                .any(|apparatus| training_map_has_apparatus(saved, apparatus))
        })
        .map(|saved| saved.map.id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<BTreeSet<_>>();
    snapshot.maps.retain(|saved| !hidden_order_ids.contains(saved.map.id.trim()));
    snapshot.maps.extend(overlay.maps.clone());
    snapshot
        .sequences
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot.visible_order_ids.retain(|apparatus, _| {
        !is_training_apparatus(apparatus, &overlay.active_apparatuses)
    });
    snapshot.queue_states.retain(|apparatus, _| {
        !is_training_apparatus(apparatus, &overlay.active_apparatuses)
    });
    snapshot.queue_action_controls.retain(|apparatus, _| {
        !is_training_apparatus(apparatus, &overlay.active_apparatuses)
    });
    snapshot.queue_policies.retain(|policy| {
        !is_training_apparatus(&policy.apparatus, &overlay.active_apparatuses)
    });
    snapshot
        .order_statuses
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot
        .order_controls
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot.sequences.extend(overlay.sequences);
    snapshot.visible_order_ids.extend(overlay.visible_order_ids);
    snapshot.queue_states.extend(overlay.queue_states);
    snapshot.queue_action_controls.extend(overlay.queue_action_controls);
    snapshot.queue_policies.extend(overlay.queue_policies);
    snapshot.order_statuses.extend(overlay.order_statuses);
    Ok(())
}

pub(super) async fn training_queue_action(
    state: &AppState,
    principal: &Principal,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    material_barcode: &str,
    material_barcodes: &[String],
    print_input: TrainingQueuePrintInput,
) -> Result<Option<serde_json::Value>, TrainingWorkspaceError> {
    let order_id = order_id.trim();
    if !order_id.starts_with("training-") {
        return Ok(None);
    }
    let overlay = worker_training_overlay(state, principal).await?;
    let Some(apparatus) = overlay
        .active_apparatuses
        .iter()
        .find(|candidate| queue_state::apparatus_titles_match(candidate, apparatus))
        .cloned()
    else {
        return Err(TrainingWorkspaceError::MapNotFound);
    };
    let training_map = overlay
        .maps
        .iter()
        .find(|saved| {
            saved.map.id.trim() == order_id && training_map_has_apparatus(saved, &apparatus)
        })
        .map(|saved| saved.map.clone())
        .ok_or(TrainingWorkspaceError::MapNotFound)?;
    let controls = overlay
        .queue_action_controls
        .get(&apparatus)
        .and_then(|controls| controls.get(order_id))
        .ok_or(TrainingWorkspaceError::MapNotFound)?;
    if !controls.allowed_actions.contains(&action) {
        return Err(TrainingWorkspaceError::InvalidInput(
            "queue_action_not_allowed".to_string(),
        ));
    }
    let store = state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    let current = overlay
        .queue_states
        .get(&apparatus)
        .and_then(|states| states.get(order_id))
        .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
        .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
    let next = queue_state::next_queue_state(current, action)
        .map_err(|_| TrainingWorkspaceError::InvalidInput("queue_action_not_allowed".to_string()))?;

    if matches!(action, queue_state::ApparatusQueueAction::Start) {
        let assigned = store
            .raw_material_barcodes_for_order_apparatus(order_id, &apparatus)
            .await?;
        let mut scanned = material_barcodes
            .iter()
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if scanned.is_empty() && !material_barcode.trim().is_empty() {
            scanned.push(material_barcode.trim().to_string());
        }
        if !assigned.is_empty() && scanned.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training_material_scan_required".to_string(),
            ));
        }
        if !assigned.is_empty()
            && scanned.iter().any(|barcode| {
                !assigned
                    .iter()
                    .any(|expected| expected.eq_ignore_ascii_case(barcode))
            })
        {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training_material_not_assigned".to_string(),
            ));
        }
    }

    let mut states = overlay
        .queue_states
        .get(&apparatus)
        .cloned()
        .unwrap_or_default();
    states.insert(order_id.to_string(), next.as_str().to_string());
    store
        .put_queue_state(&apparatus, order_id, next.as_str())
        .await?;
    state.production_maps.notify_live();
    let order_status = training_order_status(next);
    let (progress_batch, print, prints) = if training_print_action(action) {
        let progress_batch = training_progress_batch(
            &training_map,
            &apparatus,
            order_id,
            action,
            principal,
            &print_input,
        );
        let print_request = training_progress_print_request(
            &progress_batch,
            &print_input,
        );
        let print = training_progress_print(
            state,
            print_request,
            &print_input.print_transport,
        );
        let prints = vec![print.clone()];
        (Some(progress_batch), Some(print), prints)
    } else {
        (None, None, Vec::new())
    };
    let progress_batches = progress_batch
        .clone()
        .map(|batch| vec![batch])
        .unwrap_or_default();
    Ok(Some(serde_json::json!({
        "ok": true,
        "states": states,
        "order_status": order_status,
        "session": null,
        "progress_event": null,
        "progress_batch": progress_batch,
        "progress_batches": progress_batches,
        "print": print,
        "prints": prints,
    })))
}

fn training_map_has_apparatus(saved: &ProductionMapSaved, apparatus: &str) -> bool {
    saved.map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && queue_state::apparatus_titles_match(&node.title, apparatus)
    })
}

fn training_print_action(action: queue_state::ApparatusQueueAction) -> bool {
    matches!(
        action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete
    )
}

fn training_action_label(action: queue_state::ApparatusQueueAction) -> &'static str {
    match action {
        queue_state::ApparatusQueueAction::Pause => "pauza",
        queue_state::ApparatusQueueAction::DetachRoll => "rulon yechildi",
        queue_state::ApparatusQueueAction::RollComplete => "rulon tugatildi",
        queue_state::ApparatusQueueAction::Complete => "ish tugatildi",
        queue_state::ApparatusQueueAction::Start => "ish boshlandi",
        queue_state::ApparatusQueueAction::Resume => "ish davom etdi",
    }
}

fn training_positive_quantity(value: Option<f64>, fallback: f64) -> f64 {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(fallback)
}

fn training_progress_batch(
    map: &ProductionMapDefinition,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    principal: &Principal,
    input: &TrainingQueuePrintInput,
) -> serde_json::Value {
    let stamp = unix_micros();
    let batch_id = format!("training-progress-batch-{stamp}");
    let qr_payload = format!("TRAINING-PROGRESS:{stamp}");
    let item_code = if map.product_code.trim().is_empty() {
        if map.order_number.trim().is_empty() {
            order_id.trim().to_string()
        } else {
            map.order_number.trim().to_string()
        }
    } else {
        map.product_code.trim().to_string()
    };
    let title = if map.title.trim().is_empty() {
        item_code.clone()
    } else {
        map.title.trim().to_string()
    };
    let executor_name = if principal.display_name.trim().is_empty() {
        principal.ref_.trim().to_string()
    } else {
        principal.display_name.trim().to_string()
    };
    let uom = if input.uom.trim().is_empty() {
        "m"
    } else {
        input.uom.trim()
    };
    let produced_qty = training_positive_quantity(
        input.progress_qty,
        training_positive_quantity(input.finished_goods_kg, 1.0),
    );
    let gross_qty = training_positive_quantity(
        input.gross_qty.or(input.finished_goods_kg),
        produced_qty,
    );
    let timestamp = (stamp / 1_000_000) as i64;
    serde_json::json!({
        "batch_id": batch_id,
        "revision": 1,
        "session_id": format!("training-session-{stamp}"),
        "started_at_unix": timestamp,
        "completed_at_unix": timestamp,
        "apparatus": apparatus.trim(),
        "order_id": order_id.trim(),
        "action": match action {
            queue_state::ApparatusQueueAction::Pause => "pause",
            queue_state::ApparatusQueueAction::DetachRoll => "detach_roll",
            queue_state::ApparatusQueueAction::RollComplete => "roll_complete",
            queue_state::ApparatusQueueAction::Complete => "complete",
            queue_state::ApparatusQueueAction::Start => "start",
            queue_state::ApparatusQueueAction::Resume => "resume",
        },
        "status": match action {
            queue_state::ApparatusQueueAction::Pause => "paused",
            queue_state::ApparatusQueueAction::DetachRoll => "roll_detached",
            _ => "completed",
        },
        "produced_qty": produced_qty,
        "uom": uom,
        "qr_payload": qr_payload,
        "label_item_code": item_code,
        "label_item_name": format!(
            "{title}, apparat: {}, {}",
            apparatus.trim(),
            training_action_label(action),
        ),
        "executor_name": executor_name,
        "worker_role": "training",
        "worker_ref": principal.ref_.trim(),
        "worker_display_name": principal.display_name.trim(),
        "wip_status": "waiting",
        "current_apparatus": apparatus.trim(),
        "current_apparatus_key": apparatus.trim().to_ascii_lowercase(),
        "current_location": apparatus.trim(),
        "return_ink_kg": serde_json::Value::Null,
        "finished_goods_kg": gross_qty,
        "bobina_kg": input.bobina_kg,
        "description": "Training progress",
        "payload_json": {
            "training": true,
            "action_label": training_action_label(action),
        },
    })
}

fn training_progress_print_request(
    batch: &serde_json::Value,
    input: &TrainingQueuePrintInput,
) -> crate::core::gscale::models::ProgressLabelPrintRequest {
    let number = |key: &str| batch.get(key).and_then(serde_json::Value::as_f64);
    crate::core::gscale::models::ProgressLabelPrintRequest {
        driver_url: input.driver_url.clone(),
        qr_payload: batch
            .get("qr_payload")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        item_code: batch
            .get("label_item_code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        item_name: batch
            .get("label_item_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        apparatus: batch
            .get("apparatus")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        customer_name: input.customer_name.trim().to_string(),
        executor_name: batch
            .get("executor_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        printer: input.printer.clone(),
        print_mode: input.print_mode.clone(),
        gross_qty: number("finished_goods_kg")
            .or_else(|| number("produced_qty"))
            .unwrap_or(1.0),
        tare_enabled: input.bobina_kg.is_some_and(|value| value > 0.0),
        tare_kg: input.bobina_kg.unwrap_or(0.0),
        progress_qty: number("produced_qty").unwrap_or(1.0),
        unit: "kg".to_string(),
        progress_unit: batch
            .get("uom")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("m")
            .to_string(),
        label_kind: "progress".to_string(),
        print_count: input.print_count,
    }
}

fn training_progress_print(
    state: &AppState,
    request: crate::core::gscale::models::ProgressLabelPrintRequest,
    print_transport: &str,
) -> serde_json::Value {
    let prepared = state.gscale.prepare_progress_label(request.clone());
    let Ok(mut response) = prepared else {
        return serde_json::json!({
            "ok": false,
            "status": "failed",
            "code": "training_print_failed",
            "error": "training_progress_label_prepare_failed",
        });
    };
    if !print_transport.trim().eq_ignore_ascii_case("offline") {
        response.status = "queued".to_string();
        response.printer_status = "server_print_queued".to_string();
        let gscale = state.gscale.clone();
        tokio::spawn(async move {
            if let Err(error) = gscale.print_progress_label(request).await {
                tracing::warn!(error = %error, "training progress label print failed");
            }
        });
    }
    serde_json::to_value(response).unwrap_or(serde_json::Value::Null)
}

fn is_training_apparatus(apparatus: &str, active_apparatuses: &[String]) -> bool {
    active_apparatuses
        .iter()
        .any(|active| queue_state::apparatus_titles_match(active, apparatus))
}

fn training_queue_action_controls(
    sequence: &[String],
    states: &BTreeMap<String, String>,
) -> BTreeMap<String, ApparatusQueueOrderActionControl> {
    let parsed_states = sequence
        .iter()
        .map(|order_id| {
            (
                order_id.clone(),
                states
                    .get(order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let active_order_id = parsed_states.iter().find_map(|(order_id, state)| {
        state.is_active().then_some(order_id.as_str())
    });
    let actionable_order_id = queue_state::first_actionable_order_id(sequence, &parsed_states);
    sequence
        .iter()
        .map(|order_id| {
            let state = parsed_states
                .get(order_id)
                .copied()
                .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
            let active_order_is_this = active_order_id.is_none_or(|active| active == order_id);
            let queue_actionable = state.is_active()
                || (state == queue_state::ApparatusQueueOrderState::Pending
                    && active_order_is_this
                    && actionable_order_id.as_deref() == Some(order_id));
            let allowed_actions = if !queue_actionable {
                Vec::new()
            } else {
                match state {
                    queue_state::ApparatusQueueOrderState::Pending => {
                        vec![queue_state::ApparatusQueueAction::Start]
                    }
                    queue_state::ApparatusQueueOrderState::InProgress => vec![
                        queue_state::ApparatusQueueAction::Pause,
                        queue_state::ApparatusQueueAction::DetachRoll,
                        queue_state::ApparatusQueueAction::Complete,
                    ],
                    queue_state::ApparatusQueueOrderState::Paused => {
                        vec![queue_state::ApparatusQueueAction::Resume]
                    }
                    queue_state::ApparatusQueueOrderState::Completed => Vec::new(),
                }
            };
            (
                order_id.clone(),
                ApparatusQueueOrderActionControl {
                    state,
                    allowed_actions,
                    previous_stage: String::new(),
                    previous_stage_ready: true,
                    complete_requires_full_report: false,
                },
            )
        })
        .collect()
}

fn training_order_status(
    state: queue_state::ApparatusQueueOrderState,
) -> ProductionOrderStatusDetail {
    let status = state.as_str().to_string();
    ProductionOrderStatusDetail {
        order_status: status.clone(),
        work_status: status.clone(),
        flow_status: status,
        ..ProductionOrderStatusDetail::default()
    }
}

pub async fn training_production_maps(
    State(state): State<AppState>,
    Query(query): Query<TrainingMapsQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            if !query.id.trim().is_empty() {
                let saved = store
                    .map(&query.id)
                    .await
                    .map_err(training_workspace_error)?
                    .ok_or_else(|| not_found("training_map_not_found"))?;
                return Ok(json_response(saved));
            }
            let maps = store.maps().await.map_err(training_workspace_error)?;
            Ok(json_response(maps))
        }
        Method::DELETE => {
            let order_id = query.id.trim();
            if order_id.is_empty() {
                return Err(bad_request("training order id kerak"));
            }
            store
                .delete_order(order_id)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "id": order_id,
            })))
        }
        Method::PUT => {
            let map: ProductionMapDefinition = parse_json(&body)?;
            let saved = store
                .save_map(map)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(saved))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_production_map_save_with_order(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }

    let mut input: TrainingMapSaveWithOrderRequest = parse_json(&body)?;
    validate_template(&input.template).map_err(training_calculate_error)?;
    input.map.customer_name = input.template.customer.trim().to_string();
    if input.template.kg > 0.0 {
        let material_catalog = state
            .calculate_materials
            .list()
            .await
            .map_err(|_| server_error("calculate materials store failed"))?;
        super::production_maps::apply_authoritative_calculation(
            &mut input.map,
            &input.template,
            &material_catalog,
        )?;
    }

    let owner = owner_key("admin", &principal.ref_);
    let saved = training_store(&state)?
        .save_map_with_order(input.map, input.template, &owner)
        .await
        .map_err(training_workspace_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "saved": saved.saved,
        "template": saved.template,
    })))
}

pub async fn training_apparatus_modes(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            let modes = store
                .apparatus_modes()
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(serde_json::json!({"modes": modes})))
        }
        Method::PUT => {
            let input: TrainingApparatusModeInput = parse_json(&body)?;
            store
                .set_apparatus_mode(&input.apparatus, input.enabled)
                .await
                .map_err(training_workspace_error)?;
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "apparatus": input.apparatus.trim(),
                "enabled": input.enabled,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_restart(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: TrainingRestartInput = parse_json(&body)?;
    let apparatus = input.apparatus.trim().to_string();
    let reset_count = training_store(&state)?
        .reset_queue_states(&apparatus)
        .await
        .map_err(training_workspace_error)?;
    state.production_maps.notify_live();
    Ok(json_response(serde_json::json!({
        "ok": true,
        "apparatus": apparatus,
        "reset_count": reset_count,
    })))
}

pub async fn training_raw_material_assignments(
    State(state): State<AppState>,
    Query(query): Query<TrainingRawMaterialAssignmentsQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            let assignments = store
                .raw_material_assignments(&query.order_id, &query.apparatus)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(assignments))
        }
        Method::POST => {
            let payload: serde_json::Value = parse_json(&body)?;
            let order_id = payload
                .get("order_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim();
            if order_id.is_empty() {
                return Err(bad_request("order_id kerak"));
            }
            if store
                .map(order_id)
                .await
                .map_err(training_workspace_error)?
                .is_none()
            {
                return Err(not_found("training_order_not_found"));
            }
            let assignment = store
                .save_raw_material_assignment(payload)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(assignment))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_order_image_upload(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    if body.is_empty() {
        return Err(bad_request("rasm kerak"));
    }
    const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
    if body.len() > MAX_IMAGE_BYTES {
        return Err(bad_request("rasm hajmi katta"));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .trim()
        .to_ascii_lowercase();
    let extension = image_extension(&mime).ok_or_else(|| bad_request("rasm formati noto'g'ri"))?;
    let image_id = format!("training-img{}", unix_micros());
    let image_name = headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(clean_file_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("rang.{extension}"));
    let owner = owner_key("admin", &principal.ref_);
    let image = training_store(&state)?
        .save_image(
            &owner,
            TrainingImage {
                image_id,
                image_name,
                image_mime: mime,
                image_size_bytes: body.len() as u64,
                body: body.to_vec(),
            },
        )
        .await
        .map_err(training_workspace_error)?;
    let image_url = format!(
        "/v1/mobile/admin/training/images/view?id={}",
        image.image_id
    );
    Ok(json_response(serde_json::json!({
        "ok": true,
        "image": {
            "image_id": image.image_id,
            "image_name": image.image_name,
            "image_mime": image.image_mime,
            "image_size_bytes": image.image_size_bytes,
            "image_url": image_url,
        }
    })))
}

pub async fn training_order_image_view(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let image_id = query_value(&uri, "id")
        .filter(|value| safe_image_id(value))
        .ok_or_else(|| bad_request("id kerak"))?;
    let owner = owner_key("admin", &principal.ref_);
    let image = training_store(&state)?
        .image(&owner, &image_id)
        .await
        .map_err(training_workspace_error)?
        .ok_or_else(|| not_found("rasm topilmadi"))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, image.image_mime)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(image.body))
        .map_err(|_| server_error("training image response failed"))
}

fn training_store(state: &AppState) -> Result<&PostgresTrainingWorkspaceStore, AdminError> {
    state
        .training_workspace
        .as_ref()
        .ok_or_else(|| server_error("training workspace unavailable"))
}

fn training_calculate_error(error: CalculateOrderError) -> AdminError {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request(detail),
        CalculateOrderError::StoreFailed => server_error("calculate order save failed"),
    }
}

pub(super) fn training_workspace_error(error: TrainingWorkspaceError) -> AdminError {
    match error {
        TrainingWorkspaceError::StoreFailed => server_error("training workspace store failed"),
        TrainingWorkspaceError::MapNotFound => not_found("training_map_not_found"),
        TrainingWorkspaceError::DuplicateOrderNumber => conflict("training_order_number_exists"),
        TrainingWorkspaceError::DuplicateRawMaterialAssignment => {
            conflict("training_material_assignment_exists")
        }
        TrainingWorkspaceError::InvalidInput(detail)
        | TrainingWorkspaceError::InvalidMap(detail) => bad_request(detail),
    }
}

fn image_extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn clean_file_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '/' | '\\' | '\0' | '\r' | '\n'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn safe_image_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn query_value(uri: &Uri, key: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (raw_key == key).then(|| raw_value.trim().to_string())
    })
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}
