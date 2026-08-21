use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::Response;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::app::AppState;
use crate::core::apparatus_standard::{ApparatusId, RuntimeApparatusConfiguration};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderTemplate, owner_key, validate_template,
};
use crate::core::production_map::pechat;
use crate::core::production_map::{
    ApparatusQueueInteractionMode, ApparatusQueueOrderActionControl, ApparatusQueuePolicy,
    ApparatusQueuePolicyRecord, ApparatusQueuePreviousWipMode, ApparatusQueueQolipMode,
    ApparatusQueueWorkerInteraction, OrderProgressBatch, OrderProgressBatchStatus,
    OrderProgressBatchStatusDetail, OrderProgressBatchWipStatus, ProductionMapDefinition,
    ProductionMapEdge, ProductionMapLiveSnapshot, ProductionMapNode, ProductionMapNodeKind,
    ProductionMapSaved, ProductionOrderStatusDetail, chain, progress_batch_id, progress_qr_payload,
    queue_state,
};
use crate::core::returned_paint::{
    ReturnedPaintItem, calculate_returned_paint, returned_paint_astatka_total,
    returned_paint_report_can_close,
};
use crate::db::postgres_training_workspace::{
    PostgresTrainingWorkspaceStore, TRAINING_VIRTUAL_INPUT_BOSMA,
    TRAINING_VIRTUAL_INPUT_LAMINATSIYA, TrainingImage, TrainingInputBatchIdentity,
    TrainingWorkspaceError,
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

fn snapshot_new_training_order_rezka_kadr_count(
    map: &mut ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) {
    if map.order_number.trim().is_empty() {
        super::production_maps::apply_order_rezka_kadr_count(map, template);
    }
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
    #[serde(default)]
    barcode: String,
}

#[derive(Default, Deserialize)]
pub struct TrainingInputBatchesQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    qr_payload: String,
}

#[derive(Default, Deserialize)]
struct TrainingInputBatchRequest {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    count: Option<usize>,
}

#[derive(Default)]
pub(super) struct WorkerTrainingOverlay {
    pub active_apparatuses: Vec<String>,
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub visible_order_ids: BTreeMap<String, Vec<String>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub queue_action_controls: BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub input_progress_batches: BTreeMap<String, Vec<OrderProgressBatch>>,
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
    pub return_ink_kg: Option<f64>,
    pub lamination_print_leftover_rolls: Option<f64>,
    pub lamination_film_leftover_rolls: Option<f64>,
    pub rezka_bosma_waste: Option<f64>,
    pub rezka_lamination_waste: Option<f64>,
    pub rezka_edge_waste: Option<f64>,
    pub total_waste: Option<f64>,
    pub finished_goods_meter: Option<f64>,
    pub diameter: Option<f64>,
    pub returned_paint_items: Vec<ReturnedPaintItem>,
    pub returned_paint_image_id: String,
    pub description: String,
    pub uom: String,
    pub customer_name: String,
    pub print_count: u32,
}

const TRAINING_INPUT_NODE_ROLE: &str = "training_input";
const TRAINING_LAMINATSIYA_INPUT_APPARATUS: &str = "Bosma aparat";
const TRAINING_REZKA_INPUT_APPARATUS: &str = "Laminatsiya aparat";
const TRAINING_INPUT_QR_PREFIX: &str = "TRAINING-INPUT:";

fn canonical_training_apparatus(value: &str) -> Result<String, TrainingWorkspaceError> {
    ApparatusId::new(value.trim().to_string())
        .map(|id| id.to_string())
        .map_err(|_| {
            TrainingWorkspaceError::InvalidInput("canonical apparatus id kerak".to_string())
        })
}

fn training_input_order_id_from_qr(qr_payload: &str) -> Option<String> {
    let (prefix, order_id) = qr_payload.trim().split_once(':')?;
    if !prefix.eq_ignore_ascii_case(TRAINING_INPUT_QR_PREFIX.trim_end_matches(':')) {
        return None;
    }
    let order_id = order_id.trim().to_ascii_lowercase();
    order_id.starts_with("training-").then_some(order_id)
}

const TRAINING_LAMINATSIYA_ROLE: &str = "laminatsiya";
const TRAINING_REZKA_ROLE: &str = "rezka";

fn canonical_apparatus_matches(left: &str, right: &str) -> bool {
    let (Ok(left), Ok(right)) = (
        ApparatusId::new(left.trim()),
        ApparatusId::new(right.trim()),
    ) else {
        return false;
    };
    left == right
}

fn training_apparatus_role<'a>(
    map: &'a ProductionMapDefinition,
    apparatus_id: &str,
) -> Option<&'a str> {
    map.nodes.iter().find_map(|node| {
        (node.kind == ProductionMapNodeKind::Apparatus
            && canonical_apparatus_matches(&node.apparatus_id, apparatus_id))
        .then_some(node.role_code.trim())
    })
}

fn is_laminatsiya_apparatus(map: &ProductionMapDefinition, apparatus_id: &str) -> bool {
    training_apparatus_role(map, apparatus_id)
        .is_some_and(|role| role.eq_ignore_ascii_case(TRAINING_LAMINATSIYA_ROLE))
}

fn is_rezka_apparatus(map: &ProductionMapDefinition, apparatus_id: &str) -> bool {
    training_apparatus_role(map, apparatus_id)
        .is_some_and(|role| role.eq_ignore_ascii_case(TRAINING_REZKA_ROLE))
}

fn is_training_input_node(node: &ProductionMapNode) -> bool {
    node.kind == ProductionMapNodeKind::Apparatus
        && node
            .role_code
            .trim()
            .eq_ignore_ascii_case(TRAINING_INPUT_NODE_ROLE)
}

fn virtual_training_input_id_for_role(role: &str) -> Option<&'static str> {
    if role.eq_ignore_ascii_case(TRAINING_LAMINATSIYA_ROLE) {
        Some(TRAINING_VIRTUAL_INPUT_BOSMA)
    } else if role.eq_ignore_ascii_case(TRAINING_REZKA_ROLE) {
        Some(TRAINING_VIRTUAL_INPUT_LAMINATSIYA)
    } else {
        None
    }
}

fn virtual_training_input_display(input_id: &str) -> Option<&'static str> {
    match input_id {
        TRAINING_VIRTUAL_INPUT_BOSMA => Some(TRAINING_LAMINATSIYA_INPUT_APPARATUS),
        TRAINING_VIRTUAL_INPUT_LAMINATSIYA => Some(TRAINING_REZKA_INPUT_APPARATUS),
        _ => None,
    }
}

fn training_input_stage_for_map(map: &ProductionMapDefinition, apparatus: &str) -> Option<String> {
    let target_node_id = map.nodes.iter().find_map(|node| {
        (node.kind == ProductionMapNodeKind::Apparatus
            && canonical_apparatus_matches(&node.apparatus_id, apparatus))
        .then_some(node.id.as_str())
    });
    if let Some(target_node_id) = target_node_id
        && let Some(input) = map.nodes.iter().find(|node| {
            is_training_input_node(node)
                && !node.item_code.trim().is_empty()
                && map
                    .edges
                    .iter()
                    .any(|edge| edge.from == node.id && edge.to == target_node_id)
        })
    {
        return Some(input.item_code.trim().to_string());
    }
    if chain::previous_work_stage_station(map, apparatus).is_some() {
        return None;
    }
    training_apparatus_role(map, apparatus)
        .and_then(virtual_training_input_id_for_role)
        .map(str::to_string)
}

fn training_input_target_apparatus(map: &ProductionMapDefinition) -> Option<String> {
    map.nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !is_training_input_node(node)
                && !node.apparatus_id.trim().is_empty()
                && virtual_training_input_id_for_role(&node.role_code).is_some()
                && training_input_stage_for_map(map, &node.apparatus_id).is_some()
        })
        .map(|node| node.apparatus_id.trim().to_string())
        .filter(|id| !id.is_empty())
}

fn training_worker_map(mut map: ProductionMapDefinition) -> ProductionMapDefinition {
    let Some(target_id) = training_input_target_apparatus(&map) else {
        return map;
    };
    let Some(target_index) = map.nodes.iter().position(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && !is_training_input_node(node)
            && canonical_apparatus_matches(&node.apparatus_id, &target_id)
    }) else {
        return map;
    };
    let target = map.nodes[target_index].clone();
    let Some(input_apparatus) = virtual_training_input_id_for_role(&target.role_code) else {
        return map;
    };
    if chain::previous_work_stage_station(&map, &target_id).is_some()
        || map.nodes.iter().any(is_training_input_node)
    {
        return map;
    }

    let mut input_id = "training-input-apparatus".to_string();
    let mut suffix = 2;
    while map.nodes.iter().any(|node| node.id == input_id) {
        input_id = format!("training-input-apparatus-{suffix}");
        suffix += 1;
    }
    let input_node = ProductionMapNode {
        id: input_id.clone(),
        kind: ProductionMapNodeKind::Apparatus,
        title: virtual_training_input_display(input_apparatus)
            .unwrap_or(input_apparatus)
            .to_string(),
        apparatus_id: String::new(),
        formula: None,
        role_code: TRAINING_INPUT_NODE_ROLE.to_string(),
        item_code: input_apparatus.to_string(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        alternative_assigned_apparatus_id: String::new(),
        rezka_kadr_count: None,
        rezka_label_length: None,
        x: target.x,
        y: target.y - 132.0,
    };
    let mut edges = Vec::with_capacity(map.edges.len() + 1);
    let mut had_incoming_edge = false;
    for edge in &map.edges {
        if edge.to == target.id {
            had_incoming_edge = true;
            edges.push(ProductionMapEdge {
                from: edge.from.clone(),
                to: input_id.clone(),
                branch: edge.branch.clone(),
            });
        } else {
            edges.push(edge.clone());
        }
    }
    if had_incoming_edge {
        edges.push(ProductionMapEdge {
            from: input_id,
            to: target.id,
            branch: String::new(),
        });
        map.nodes.insert(target_index, input_node);
        map.edges = edges;
    }
    map
}

fn training_input_progress_batch(
    map: &ProductionMapDefinition,
    apparatus: &str,
    identity: &TrainingInputBatchIdentity,
) -> Option<OrderProgressBatch> {
    let order_id = map.id.trim();
    let previous_stage = training_input_stage_for_map(map, apparatus)?;
    if order_id.is_empty()
        || !identity.order_id.eq_ignore_ascii_case(order_id)
        || !canonical_apparatus_matches(&identity.apparatus, apparatus)
    {
        return None;
    }
    let item_code = if map.product_code.trim().is_empty() {
        if map.order_number.trim().is_empty() {
            order_id.to_string()
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
    let produced_qty = map
        .order_kg
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(1.0);
    let mut batch = OrderProgressBatch {
        batch_id: identity.batch_id.clone(),
        revision: 1,
        session_id: identity.session_id.clone(),
        started_at_unix: 0,
        completed_at_unix: 0,
        apparatus: apparatus.trim().to_string(),
        order_id: order_id.to_string(),
        action: queue_state::ApparatusQueueAction::Complete,
        status: OrderProgressBatchStatus::Completed,
        produced_qty,
        uom: "kg".to_string(),
        qr_payload: identity.qr_payload.clone(),
        label_item_code: item_code,
        label_item_name: format!("{title}, apparat: {previous_stage}, training input"),
        executor_name: format!("Training {previous_stage}"),
        worker_role: "training".to_string(),
        worker_ref: "training-input".to_string(),
        worker_display_name: format!("Training {previous_stage}"),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: apparatus.trim().to_string(),
        current_apparatus_key: queue_state::apparatus_search_key(&previous_stage),
        current_location: format!("{previous_stage} chiqim"),
        next_apparatus: apparatus.trim().to_string(),
        parent_batch_id: String::new(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: Some(produced_qty),
        bobina_kg: None,
        finished_goods_meter: None,
        diameter: None,
        description: format!("Training uchun generatsiya qilingan {previous_stage} input batch"),
        payload_json: serde_json::json!({
            "training": true,
            "training_input": true,
            "source": "generated_training_order_batch",
            "source_apparatus": previous_stage,
            "training_virtual_apparatus": previous_stage,
        }),
    };
    batch.refresh_status_detail();
    Some(batch)
}

fn training_input_batch_matches(
    batch: &OrderProgressBatch,
    order_id: &str,
    previous_stage: &str,
    apparatus: &str,
) -> bool {
    batch.order_id.trim().eq_ignore_ascii_case(order_id.trim())
        && batch
            .payload_json
            .get("training_virtual_apparatus")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case(previous_stage))
        && (batch.next_apparatus.trim().is_empty()
            || canonical_apparatus_matches(&batch.next_apparatus, apparatus))
}

fn training_input_batch_is_available(
    batch: &OrderProgressBatch,
    order_id: &str,
    previous_stage: &str,
    apparatus: &str,
) -> bool {
    training_input_batch_matches(batch, order_id, previous_stage, apparatus)
        && (batch.wip_status == OrderProgressBatchWipStatus::Waiting
            || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                && canonical_apparatus_matches(&batch.used_by_apparatus, apparatus)))
}

fn training_claim_input_batch(
    batch: &OrderProgressBatch,
    apparatus: &str,
    order_id: &str,
) -> OrderProgressBatch {
    let mut claimed = batch.clone();
    claimed.wip_status = OrderProgressBatchWipStatus::InUse;
    claimed.current_apparatus = apparatus.trim().to_string();
    claimed.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    claimed.current_location = apparatus.trim().to_string();
    claimed.used_by_session_id = format!(
        "training-input-use:{}:{}:{}",
        apparatus.trim(),
        order_id.trim(),
        claimed.batch_id.trim()
    );
    claimed.used_by_apparatus = apparatus.trim().to_string();
    claimed.refresh_status_detail();
    claimed
}

fn training_process_input_batch(
    batch: &OrderProgressBatch,
    apparatus: &str,
    order_id: &str,
) -> OrderProgressBatch {
    let mut processed = batch.clone();
    processed.wip_status = OrderProgressBatchWipStatus::Processed;
    processed.current_apparatus = apparatus.trim().to_string();
    processed.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    processed.current_location = format!("{} yakunlandi", apparatus.trim());
    processed.processed_by_session_id = format!(
        "training-input-use:{}:{}:{}",
        apparatus.trim(),
        order_id.trim(),
        processed.batch_id.trim()
    );
    processed.processed_by_apparatus = apparatus.trim().to_string();
    processed.refresh_status_detail();
    processed
}

fn training_has_unprocessed_previous_wips(
    batches: &[OrderProgressBatch],
    order_id: &str,
    previous_stage: &str,
    apparatus: &str,
    ignored_batch_id: &str,
) -> bool {
    batches.iter().any(|batch| {
        training_input_batch_is_available(batch, order_id, previous_stage, apparatus)
            && (ignored_batch_id.trim().is_empty()
                || !batch
                    .batch_id
                    .trim()
                    .eq_ignore_ascii_case(ignored_batch_id.trim()))
    })
}

async fn training_input_progress_batches_for_map(
    store: &PostgresTrainingWorkspaceStore,
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let Some(previous_stage) = training_input_stage_for_map(map, apparatus) else {
        return Ok(Vec::new());
    };
    let mut batches = store.training_progress_batches_for_order(&map.id).await?;
    let identities = store.training_input_batches(&map.id, apparatus).await?;
    for identity in identities {
        if batches.iter().any(|batch| {
            batch
                .batch_id
                .trim()
                .eq_ignore_ascii_case(identity.batch_id.trim())
        }) {
            continue;
        }
        if let Some(batch) = training_input_progress_batch(map, apparatus, &identity) {
            batches.push(batch);
        }
    }
    Ok(batches
        .into_iter()
        .filter(|batch| training_input_batch_matches(batch, &map.id, &previous_stage, apparatus))
        .collect())
}

async fn training_generated_input_progress_batches_for_map(
    store: &PostgresTrainingWorkspaceStore,
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let persisted_batches = store.training_progress_batches_for_order(&map.id).await?;
    let identities = store.training_input_batches(&map.id, apparatus).await?;
    Ok(identities
        .into_iter()
        .filter_map(|identity| {
            persisted_batches
                .iter()
                .find(|batch| {
                    batch
                        .batch_id
                        .trim()
                        .eq_ignore_ascii_case(identity.batch_id.trim())
                })
                .cloned()
                .or_else(|| training_input_progress_batch(map, apparatus, &identity))
        })
        .collect())
}

pub(super) async fn training_input_progress_batch_for_principal(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    previous_apparatus: &str,
    next_apparatus: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let order_id = order_id.trim();
    if !order_id.starts_with("training-") {
        return Ok(Vec::new());
    }
    let requested_next = if next_apparatus.trim().is_empty() {
        String::new()
    } else {
        canonical_training_apparatus(next_apparatus)?
    };
    let saved = if matches!(&principal.role, PrincipalRole::Aparatchi) {
        worker_training_overlay(state, principal)
            .await?
            .maps
            .into_iter()
            .find(|saved| {
                saved.map.id.trim() == order_id
                    && (requested_next.is_empty()
                        || training_map_has_apparatus(saved, &requested_next))
            })
    } else {
        state
            .training_workspace
            .as_ref()
            .ok_or(TrainingWorkspaceError::StoreFailed)?
            .map(order_id)
            .await?
    };
    let Some(saved) = saved else {
        return Ok(Vec::new());
    };
    let target = if requested_next.is_empty() {
        training_input_target_apparatus(&saved.map).unwrap_or_default()
    } else {
        requested_next.clone()
    };
    let Some(_) = training_input_stage_for_map(&saved.map, &target) else {
        return Ok(Vec::new());
    };
    let Some(store) = state.training_workspace.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(
        training_input_progress_batches_for_map(store, &saved.map, &target)
            .await?
            .into_iter()
            .filter(|batch| {
                previous_apparatus.trim().is_empty()
                    || canonical_apparatus_matches(&batch.apparatus, previous_apparatus)
            })
            .collect(),
    )
}

pub(super) async fn training_progress_batch_for_qr(
    state: &AppState,
    principal: &Principal,
    progress_batch_id: &str,
    qr_payload: &str,
) -> Result<Option<OrderProgressBatch>, TrainingWorkspaceError> {
    let qr_payload = qr_payload.trim();
    let Some(store) = state.training_workspace.as_ref() else {
        return Ok(None);
    };
    if let Some(batch) = store
        .training_progress_batch_for_key(progress_batch_id, qr_payload)
        .await?
    {
        let is_visible = if matches!(&principal.role, PrincipalRole::Aparatchi) {
            worker_training_overlay(state, principal)
                .await?
                .maps
                .iter()
                .any(|saved| saved.map.id.trim().eq_ignore_ascii_case(&batch.order_id))
        } else {
            store.map(&batch.order_id).await?.is_some()
        };
        return Ok(is_visible.then_some(batch));
    }
    let identity_for_qr = store.training_input_batch_for_qr(qr_payload).await?;
    let legacy_order_id = training_input_order_id_from_qr(qr_payload);
    let order_id = identity_for_qr
        .as_ref()
        .map(|identity| identity.order_id.clone())
        .or(legacy_order_id.clone());
    let Some(order_id) = order_id else {
        return Ok(None);
    };
    let saved = if matches!(&principal.role, PrincipalRole::Aparatchi) {
        worker_training_overlay(state, principal)
            .await?
            .maps
            .into_iter()
            .find(|saved| saved.map.id.trim().eq_ignore_ascii_case(&order_id))
    } else {
        state
            .training_workspace
            .as_ref()
            .ok_or(TrainingWorkspaceError::StoreFailed)?
            .map(&order_id)
            .await?
    };
    let Some(saved) = saved else {
        return Ok(None);
    };
    let apparatus = identity_for_qr
        .as_ref()
        .map(|identity| identity.apparatus.clone())
        .or_else(|| training_input_target_apparatus(&saved.map));
    let Some(apparatus) = apparatus else {
        return Ok(None);
    };
    let identity = match identity_for_qr {
        Some(identity) => identity,
        None => {
            let Some(previous_stage) = training_input_stage_for_map(&saved.map, &apparatus) else {
                return Ok(None);
            };
            let identities = store.training_input_batches(&order_id, &apparatus).await?;
            if identities.len() != 1 {
                return Ok(None);
            }
            let identity = identities
                .into_iter()
                .next()
                .ok_or(TrainingWorkspaceError::StoreFailed)?;
            if !canonical_apparatus_matches(&identity.apparatus, &apparatus)
                || previous_stage.trim().is_empty()
            {
                return Ok(None);
            }
            identity
        }
    };
    let Some(batch) = training_input_progress_batch(&saved.map, &apparatus, &identity) else {
        return Ok(None);
    };
    if legacy_order_id.is_some() || batch.qr_payload.eq_ignore_ascii_case(qr_payload) {
        Ok(Some(batch))
    } else {
        Ok(None)
    }
}

pub(super) async fn training_progress_batches_for_order(
    state: &AppState,
    order_id: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?
        .training_progress_batches_for_order(order_id)
        .await
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
        .filter_map(|apparatus| canonical_training_apparatus(&apparatus).ok())
        .filter(|apparatus| {
            modes.iter().any(|(configured, enabled)| {
                *enabled && canonical_apparatus_matches(configured, apparatus)
            })
        })
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
        .map(|mut saved| {
            saved.map = training_worker_map(saved.map);
            saved
        })
        .collect::<Vec<_>>();
    let stored_states = store.queue_states().await?;
    let mut overlay = WorkerTrainingOverlay {
        active_apparatuses,
        maps,
        ..WorkerTrainingOverlay::default()
    };

    for saved in &overlay.maps {
        let mut batches_by_id = BTreeMap::new();
        for apparatus in &overlay.active_apparatuses {
            if !training_map_has_apparatus(saved, apparatus) {
                continue;
            }
            for batch in
                training_input_progress_batches_for_map(store, &saved.map, apparatus).await?
            {
                batches_by_id.insert(batch.batch_id.trim().to_string(), batch);
            }
        }
        if !batches_by_id.is_empty() {
            overlay.input_progress_batches.insert(
                saved.map.id.trim().to_string(),
                batches_by_id.into_values().collect(),
            );
        }
    }

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
            if !canonical_apparatus_matches(stored_apparatus, apparatus) {
                continue;
            }
            for (order_id, state) in stored {
                if visible_set.contains(order_id) {
                    states.insert(order_id.clone(), state.clone());
                }
            }
        }
        let controls = training_queue_action_controls(
            apparatus,
            &*state
                .production_maps
                .resolve_canonical_apparatus_text(apparatus)
                .await
                .map_err(|_| TrainingWorkspaceError::StoreFailed)?,
            &sequence,
            &states,
            &overlay.maps,
            &overlay.input_progress_batches,
        );
        let statuses = sequence
            .iter()
            .map(|order_id| {
                let state = controls
                    .get(order_id)
                    .map(|control| control.state)
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
                (order_id.clone(), training_order_status(state))
            })
            .collect::<BTreeMap<_, _>>();
        overlay
            .sequences
            .insert(apparatus.clone(), sequence.clone());
        overlay
            .visible_order_ids
            .insert(apparatus.clone(), visible_order_ids);
        overlay.queue_states.insert(apparatus.clone(), states);
        overlay
            .queue_action_controls
            .insert(apparatus.clone(), controls);
        let apparatus_id = crate::core::apparatus_standard::ApparatusId::new(apparatus.clone())
            .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        overlay.queue_policies.push(ApparatusQueuePolicyRecord {
            apparatus_id,
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
    let apparatus = if apparatus.trim().is_empty() {
        None
    } else {
        Some(canonical_training_apparatus(apparatus)?)
    };
    if matches!(&principal.role, PrincipalRole::Aparatchi) {
        let overlay = worker_training_overlay(state, principal).await?;
        let Some(active_apparatus) = overlay.active_apparatuses.iter().find(|candidate| {
            apparatus
                .as_ref()
                .is_some_and(|id| canonical_apparatus_matches(candidate, id))
        }) else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        let Some(saved) = overlay.maps.iter().find(|saved| {
            saved.map.id.trim() == order_id && training_map_has_apparatus(saved, active_apparatus)
        }) else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        Ok(Some(saved.clone()))
    } else {
        let Some(saved) = store.map(order_id).await? else {
            return Err(TrainingWorkspaceError::MapNotFound);
        };
        if apparatus
            .as_ref()
            .is_some_and(|id| !training_map_has_apparatus(&saved, id))
        {
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
    let apparatus = canonical_training_apparatus(apparatus)?;
    Ok(Some(
        store.raw_material_assignments(order_id, &apparatus).await?,
    ))
}

pub(super) async fn training_raw_material_start_requirements(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
    material_barcodes: &str,
) -> Result<Option<serde_json::Value>, TrainingWorkspaceError> {
    let Some(assignments) =
        training_material_assignments_for_principal(state, principal, order_id, apparatus).await?
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
    snapshot
        .maps
        .retain(|saved| !hidden_order_ids.contains(saved.map.id.trim()));
    snapshot.maps.extend(overlay.maps.clone());
    snapshot
        .sequences
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot
        .visible_order_ids
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot
        .queue_states
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot
        .queue_action_controls
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot.queue_policies.retain(|policy| {
        !is_training_apparatus(policy.apparatus_id.as_str(), &overlay.active_apparatuses)
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
    snapshot
        .queue_action_controls
        .extend(overlay.queue_action_controls);
    snapshot.queue_policies.extend(overlay.queue_policies);
    snapshot.order_statuses.extend(overlay.order_statuses);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn training_queue_action(
    state: &AppState,
    principal: &Principal,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    material_barcode: &str,
    material_barcodes: &[String],
    progress_batch_id: &str,
    progress_qr: &str,
    print_input: TrainingQueuePrintInput,
) -> Result<Option<serde_json::Value>, TrainingWorkspaceError> {
    let order_id = order_id.trim();
    if !order_id.starts_with("training-") {
        return Ok(None);
    }
    let requested_apparatus = canonical_training_apparatus(apparatus)?;
    let overlay = worker_training_overlay(state, principal).await?;
    let Some(apparatus) = overlay
        .active_apparatuses
        .iter()
        .find(|candidate| canonical_apparatus_matches(candidate, &requested_apparatus))
        .cloned()
    else {
        return Err(TrainingWorkspaceError::MapNotFound);
    };
    let canonical = state
        .production_maps
        .resolve_canonical_apparatus_text(&apparatus)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    let mut training_map = overlay
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
    if is_rezka_apparatus(&training_map, &apparatus)
        && training_print_action(action)
        && training_rezka_frame_count(&training_map, &apparatus).is_err()
        && let Some(template) = store
            .template_for_order(order_id, &training_map.order_number)
            .await?
    {
        super::production_maps::apply_order_rezka_kadr_count(&mut training_map, &template);
        if training_rezka_frame_count(&training_map, &apparatus).is_ok() {
            store.save_map(training_map.clone()).await?;
        }
    }
    let current = overlay
        .queue_states
        .get(&apparatus)
        .and_then(|states| states.get(order_id))
        .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
        .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
    let next = queue_state::next_queue_state(current, action).map_err(|_| {
        TrainingWorkspaceError::InvalidInput("queue_action_not_allowed".to_string())
    })?;

    let previous_stage = training_input_stage_for_map(&training_map, &apparatus);
    let input_batches = overlay
        .input_progress_batches
        .get(order_id)
        .cloned()
        .unwrap_or_default();
    let active_input_batch = previous_stage.as_deref().and_then(|stage| {
        input_batches
            .iter()
            .find(|batch| {
                batch.wip_status == OrderProgressBatchWipStatus::InUse
                    && training_input_batch_matches(batch, order_id, stage, &apparatus)
                    && canonical_apparatus_matches(&batch.used_by_apparatus, &apparatus)
            })
            .cloned()
    });
    let current_input_batch_id = active_input_batch
        .as_ref()
        .map(|batch| batch.batch_id.as_str())
        .unwrap_or_default();
    let has_unprocessed_previous_wips = previous_stage.as_deref().is_some_and(|stage| {
        training_has_unprocessed_previous_wips(
            &input_batches,
            order_id,
            stage,
            &apparatus,
            current_input_batch_id,
        )
    });
    let full_completion_report_required = training_complete_requires_full_report(
        &training_map,
        &apparatus,
        has_unprocessed_previous_wips,
    );

    let is_complete = matches!(action, queue_state::ApparatusQueueAction::Complete);
    let has_returned_paint_items = !print_input.returned_paint_items.is_empty();
    let has_returned_paint_image = !print_input.returned_paint_image_id.trim().is_empty();
    if !is_complete && (has_returned_paint_items || has_returned_paint_image) {
        return Err(TrainingWorkspaceError::InvalidInput(
            "returned_paint_only_on_complete".to_string(),
        ));
    }
    if is_complete
        && pechat::is_pechat_apparatus(canonical.as_ref())
        && !returned_paint_report_can_close(
            &print_input.returned_paint_items,
            has_returned_paint_image,
        )
    {
        return Err(TrainingWorkspaceError::InvalidInput(
            "returned_paint_minimum_three_fields_or_image_only".to_string(),
        ));
    }
    let returned_paint_calculation = if has_returned_paint_items {
        Some(
            calculate_returned_paint(&print_input.returned_paint_items).map_err(|error| {
                TrainingWorkspaceError::InvalidInput(format!(
                    "training_returned_paint_invalid: {error}"
                ))
            })?,
        )
    } else {
        None
    };
    let returned_paint_astatka_kg = if has_returned_paint_items {
        let total =
            returned_paint_astatka_total(&print_input.returned_paint_items).map_err(|error| {
                TrainingWorkspaceError::InvalidInput(format!(
                    "training_returned_paint_invalid: {error}"
                ))
            })?;
        (total > 0.0).then_some(total)
    } else {
        None
    };
    let return_ink_kg = returned_paint_astatka_kg.or_else(|| {
        print_input
            .return_ink_kg
            .filter(|value| value.is_finite() && *value >= 0.0)
    });
    if is_rezka_apparatus(&training_map, &apparatus) && training_print_action(action) {
        if !training_rezka_progress_metrics_are_complete(&print_input) {
            return Err(TrainingWorkspaceError::InvalidInput(
                "rezka_progress_metrics_required".to_string(),
            ));
        }
        training_rezka_frame_count(&training_map, &apparatus)?;
    }
    if is_complete && is_laminatsiya_apparatus(&training_map, &apparatus) {
        let metrics_ready = if full_completion_report_required {
            training_laminatsiya_full_metrics_are_complete(&print_input)
        } else {
            training_laminatsiya_partial_metrics_are_complete(&print_input)
        };
        if !metrics_ready {
            return Err(TrainingWorkspaceError::InvalidInput(
                "laminatsiya_completion_metrics_required".to_string(),
            ));
        }
    }
    if is_complete
        && is_rezka_apparatus(&training_map, &apparatus)
        && full_completion_report_required
        && !training_rezka_waste_metrics_are_complete(&print_input)
    {
        return Err(TrainingWorkspaceError::InvalidInput(
            "rezka_progress_metrics_required".to_string(),
        ));
    }
    let returned_paint_report =
        if is_complete && (has_returned_paint_items || has_returned_paint_image) {
            Some(
                store
                    .save_returned_paint_report(
                        order_id,
                        &apparatus,
                        training_action_value(action),
                        &print_input.returned_paint_items,
                        &print_input.returned_paint_image_id,
                        return_ink_kg,
                        returned_paint_calculation.as_ref(),
                    )
                    .await?,
            )
        } else {
            None
        };

    let mut input_batch_update = None;
    let mut action_input_batch = active_input_batch.clone();
    if matches!(action, queue_state::ApparatusQueueAction::Start) {
        if let Some(previous_stage) = previous_stage.as_deref() {
            let scanned_qr = if progress_qr.trim().is_empty() {
                progress_batch_id.trim()
            } else {
                progress_qr.trim()
            };
            if scanned_qr.is_empty() {
                return Err(TrainingWorkspaceError::InvalidInput(
                    "progress_qr_required".to_string(),
                ));
            }
            let selected = input_batches.iter().find(|batch| {
                training_input_batch_matches(batch, order_id, previous_stage, &apparatus)
                    && (batch.qr_payload.eq_ignore_ascii_case(scanned_qr)
                        || batch.batch_id.eq_ignore_ascii_case(scanned_qr))
            });
            let Some(selected) = selected else {
                return Err(TrainingWorkspaceError::InvalidInput(
                    "progress_batch_not_accepted".to_string(),
                ));
            };
            if selected.wip_status != OrderProgressBatchWipStatus::Waiting {
                return Err(TrainingWorkspaceError::InvalidInput(
                    "progress_batch_not_accepted".to_string(),
                ));
            }
            let claimed = training_claim_input_batch(selected, &apparatus, order_id);
            action_input_batch = Some(claimed.clone());
            input_batch_update = Some(claimed);
        }
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

    if training_print_action(action) && previous_stage.is_some() && action_input_batch.is_none() {
        return Err(TrainingWorkspaceError::InvalidInput(
            "progress_qr_required".to_string(),
        ));
    }

    let parent_batch_id = if training_print_action(action) {
        action_input_batch
            .as_ref()
            .map(|batch| batch.batch_id.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let progress_batches = if training_print_action(action) {
        training_progress_batches(
            &training_map,
            &apparatus,
            order_id,
            action,
            principal,
            &print_input,
            returned_paint_report.as_ref(),
            return_ink_kg,
            &parent_batch_id,
        )?
    } else {
        Vec::new()
    };

    if matches!(
        action,
        queue_state::ApparatusQueueAction::Complete
            | queue_state::ApparatusQueueAction::RollComplete
    ) {
        input_batch_update = action_input_batch
            .as_ref()
            .map(|batch| training_process_input_batch(batch, &apparatus, order_id));
    }
    let mut persisted_progress_batches = progress_batches.clone();
    if let Some(input_batch_update) = input_batch_update {
        persisted_progress_batches.push(input_batch_update);
    }

    let mut states = overlay
        .queue_states
        .get(&apparatus)
        .cloned()
        .unwrap_or_default();
    let persisted_state = if is_complete && has_unprocessed_previous_wips {
        queue_state::ApparatusQueueOrderState::Pending
    } else {
        next
    };
    states.insert(order_id.to_string(), persisted_state.as_str().to_string());
    let event_id = format!("training-queue-event-{}-{}", unix_micros(), order_id);
    store
        .put_queue_state_with_event(
            &apparatus,
            order_id,
            persisted_state.as_str(),
            &event_id,
            training_action_value(action),
            current.as_str(),
            &principal.ref_,
            &principal.display_name,
            &persisted_progress_batches,
        )
        .await?;
    state.production_maps.notify_live();
    let order_status = training_order_status(persisted_state);
    let print_requests = progress_batches
        .iter()
        .map(|batch| training_progress_print_request(batch, &print_input))
        .collect::<Vec<_>>();
    let prints = training_progress_prints(
        state,
        print_requests,
        &print_input.print_transport,
        &apparatus,
        order_id,
        action,
    );
    let progress_batch = progress_batches.first().cloned();
    let print = prints.first().cloned();
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
            && !is_training_input_node(node)
            && canonical_apparatus_matches(&node.apparatus_id, apparatus)
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
        queue_state::ApparatusQueueAction::Freeze => "muzlatildi",
        queue_state::ApparatusQueueAction::DetachRoll => "rulon yechildi",
        queue_state::ApparatusQueueAction::RollComplete => "rulon tugatildi",
        queue_state::ApparatusQueueAction::Complete => "ish tugatildi",
        queue_state::ApparatusQueueAction::Start => "ish boshlandi",
        queue_state::ApparatusQueueAction::Resume => "ish davom etdi",
    }
}

fn training_action_value(action: queue_state::ApparatusQueueAction) -> &'static str {
    match action {
        queue_state::ApparatusQueueAction::Pause => "pause",
        queue_state::ApparatusQueueAction::Freeze => "freeze",
        queue_state::ApparatusQueueAction::DetachRoll => "detach_roll",
        queue_state::ApparatusQueueAction::RollComplete => "roll_complete",
        queue_state::ApparatusQueueAction::Complete => "complete",
        queue_state::ApparatusQueueAction::Start => "start",
        queue_state::ApparatusQueueAction::Resume => "resume",
    }
}

fn training_positive_quantity(value: Option<f64>, fallback: f64) -> f64 {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(fallback)
}

fn training_output_quantity(input: &TrainingQueuePrintInput) -> f64 {
    training_positive_quantity(
        input.progress_qty.or(input.finished_goods_meter),
        training_positive_quantity(input.finished_goods_kg, 1.0),
    )
}

fn training_rezka_progress_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    let is_positive =
        |value: Option<f64>| value.is_some_and(|value| value.is_finite() && value > 0.0);
    is_positive(input.progress_qty.or(input.finished_goods_meter))
        && is_positive(input.gross_qty.or(input.finished_goods_kg))
        && is_positive(input.diameter)
}

fn training_rezka_waste_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    [
        input.total_waste,
        input.rezka_bosma_waste,
        input.rezka_lamination_waste,
        input.rezka_edge_waste,
    ]
    .into_iter()
    .any(|value| value.is_some_and(|value| value.is_finite() && value > 0.0))
}

fn training_laminatsiya_partial_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    let is_positive =
        |value: Option<f64>| value.is_some_and(|value| value.is_finite() && value > 0.0);
    is_positive(input.finished_goods_kg.or(input.gross_qty))
        && is_positive(input.finished_goods_meter.or(input.progress_qty))
        && is_positive(input.bobina_kg)
}

fn training_laminatsiya_full_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    training_laminatsiya_partial_metrics_are_complete(input)
        && (input
            .lamination_print_leftover_rolls
            .is_some_and(|value| value.is_finite() && value >= 0.0)
            || input
                .lamination_film_leftover_rolls
                .is_some_and(|value| value.is_finite() && value >= 0.0))
        && input
            .total_waste
            .is_some_and(|value| value.is_finite() && value > 0.0)
}

fn training_apparatus_node<'a>(
    map: &'a ProductionMapDefinition,
    apparatus: &str,
) -> Option<&'a ProductionMapNode> {
    map.nodes.iter().find(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && (canonical_apparatus_matches(&node.apparatus_id, apparatus)
                || canonical_apparatus_matches(&node.alternative_assigned_apparatus_id, apparatus))
    })
}

fn training_rezka_frame_count(
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> Result<usize, TrainingWorkspaceError> {
    training_apparatus_node(map, apparatus)
        .and_then(|node| node.rezka_kadr_count)
        .filter(|value| *value > 0)
        .map(|value| value as usize)
        .ok_or_else(|| {
            TrainingWorkspaceError::InvalidInput("rezka_kadr_count_required".to_string())
        })
}

#[allow(clippy::too_many_arguments)]
fn training_progress_batches(
    map: &ProductionMapDefinition,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    principal: &Principal,
    input: &TrainingQueuePrintInput,
    returned_paint_report: Option<&serde_json::Value>,
    return_ink_kg: Option<f64>,
    parent_batch_id: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let stamp = unix_micros();
    let base_batch_id = progress_batch_id(apparatus, order_id, action, 0);
    let rezka_node = training_apparatus_node(map, apparatus);
    let frame_count = if is_rezka_apparatus(map, apparatus) {
        training_rezka_frame_count(map, apparatus)?
    } else {
        1
    };
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
    let produced_qty = training_output_quantity(input);
    let finished_goods_kg =
        training_positive_quantity(input.finished_goods_kg.or(input.gross_qty), produced_qty);
    let timestamp = (stamp / 1_000_000) as i64;
    let status = match action {
        queue_state::ApparatusQueueAction::Pause => OrderProgressBatchStatus::Paused,
        queue_state::ApparatusQueueAction::Freeze => OrderProgressBatchStatus::Completed,
        queue_state::ApparatusQueueAction::DetachRoll => OrderProgressBatchStatus::RollDetached,
        _ => OrderProgressBatchStatus::Completed,
    };
    let description = if input.description.trim().is_empty() {
        "Training progress"
    } else {
        input.description.trim()
    };
    let next_apparatus = chain::next_work_stage_station(map, apparatus).unwrap_or_default();
    let label_name = format!(
        "{title}, apparat: {}, {}",
        apparatus.trim(),
        training_action_label(action),
    );
    let session_id = format!("training-session:{base_batch_id}");
    let mut batches = Vec::with_capacity(frame_count);
    for index in 0..frame_count {
        let is_rezka = is_rezka_apparatus(map, apparatus);
        let owns_metrics = !is_rezka || index == 0;
        let batch_id = if is_rezka {
            format!("{base_batch_id}:frame:{}", index + 1)
        } else {
            base_batch_id.clone()
        };
        let qr_payload = progress_qr_payload(&batch_id);
        let rezka_bosma_waste = owns_metrics.then_some(input.rezka_bosma_waste).flatten();
        let rezka_lamination_waste = owns_metrics
            .then_some(input.rezka_lamination_waste)
            .flatten();
        let rezka_edge_waste = owns_metrics.then_some(input.rezka_edge_waste).flatten();
        let total_waste = owns_metrics.then_some(input.total_waste).flatten();
        let bobina_kg = owns_metrics.then_some(input.bobina_kg).flatten();
        let diameter = owns_metrics.then_some(input.diameter).flatten();
        let mut payload_json = serde_json::json!({
            "training": true,
            "order_title": map.title.trim(),
            "customer_name": map.customer_name.trim(),
            "apparatus": apparatus.trim(),
            "action": training_action_value(action),
            "action_label": training_action_label(action),
            "astatka_kg": return_ink_kg,
            "return_ink_kg": return_ink_kg,
            "lamination_print_leftover_rolls": input.lamination_print_leftover_rolls,
            "lamination_film_leftover_rolls": input.lamination_film_leftover_rolls,
            "rezka_bosma_waste": rezka_bosma_waste,
            "rezka_lamination_waste": rezka_lamination_waste,
            "rezka_edge_waste": rezka_edge_waste,
            "total_waste": total_waste,
            "total_waste_uom": "kg",
            "finished_goods_kg": finished_goods_kg,
            "bobina_kg": bobina_kg,
            "finished_goods_meter": input.finished_goods_meter,
            "diameter": diameter,
            "description": description,
            "returned_paint_report": returned_paint_report
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        });
        if is_rezka {
            payload_json["rezka_frame_index"] = serde_json::json!(index + 1);
            payload_json["rezka_frame_count"] = serde_json::json!(frame_count);
            payload_json["rezka_kadr_count"] = serde_json::json!(frame_count);
            payload_json["rezka_output_kind"] = serde_json::json!("frame");
            payload_json["rezka_metrics_owner"] = serde_json::json!(owns_metrics);
            if let Some(label_length) = rezka_node.and_then(|node| node.rezka_label_length) {
                payload_json["rezka_label_length"] = serde_json::json!(label_length);
            }
        }
        let mut batch = OrderProgressBatch {
            batch_id,
            revision: 1,
            session_id: session_id.clone(),
            started_at_unix: timestamp,
            completed_at_unix: timestamp,
            apparatus: apparatus.trim().to_string(),
            order_id: order_id.trim().to_string(),
            action,
            status,
            produced_qty,
            uom: uom.to_string(),
            qr_payload,
            label_item_code: item_code.clone(),
            label_item_name: label_name.clone(),
            executor_name: executor_name.clone(),
            worker_role: "training".to_string(),
            worker_ref: principal.ref_.trim().to_string(),
            worker_display_name: principal.display_name.trim().to_string(),
            wip_status: OrderProgressBatchWipStatus::Waiting,
            status_detail: OrderProgressBatchStatusDetail::default(),
            current_apparatus: apparatus.trim().to_string(),
            current_apparatus_key: queue_state::apparatus_search_key(apparatus),
            current_location: apparatus.trim().to_string(),
            next_apparatus: next_apparatus.clone(),
            parent_batch_id: parent_batch_id.trim().to_string(),
            used_by_session_id: String::new(),
            used_by_apparatus: String::new(),
            processed_by_session_id: String::new(),
            processed_by_apparatus: String::new(),
            return_ink_kg,
            lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
            rezka_bosma_waste,
            rezka_lamination_waste,
            rezka_edge_waste,
            total_waste,
            finished_goods_kg: Some(finished_goods_kg),
            bobina_kg,
            finished_goods_meter: input.finished_goods_meter,
            diameter,
            description: description.to_string(),
            payload_json,
        };
        batch.refresh_status_detail();
        batches.push(batch);
    }
    Ok(batches)
}

fn training_progress_print_request(
    batch: &OrderProgressBatch,
    input: &TrainingQueuePrintInput,
) -> crate::core::gscale::models::ProgressLabelPrintRequest {
    crate::core::gscale::models::ProgressLabelPrintRequest {
        driver_url: input.driver_url.clone(),
        qr_payload: batch.qr_payload.clone(),
        item_code: batch.label_item_code.clone(),
        item_name: batch.label_item_name.clone(),
        apparatus: batch.apparatus.clone(),
        customer_name: input.customer_name.trim().to_string(),
        executor_name: batch.executor_name.clone(),
        printer: input.printer.clone(),
        print_mode: input.print_mode.clone(),
        gross_qty: input
            .gross_qty
            .or(input.finished_goods_kg)
            .unwrap_or(batch.produced_qty),
        tare_enabled: input.bobina_kg.is_some_and(|value| value > 0.0),
        tare_kg: input.bobina_kg.unwrap_or(0.0),
        progress_qty: batch.produced_qty,
        unit: "kg".to_string(),
        progress_unit: if batch.uom.trim().is_empty() {
            "m".to_string()
        } else {
            batch.uom.clone()
        },
        label_kind: "progress".to_string(),
        print_count: input.print_count,
    }
}

fn training_progress_prints(
    state: &AppState,
    requests: Vec<crate::core::gscale::models::ProgressLabelPrintRequest>,
    print_transport: &str,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
) -> Vec<serde_json::Value> {
    if print_transport.trim().eq_ignore_ascii_case("offline") {
        return requests
            .into_iter()
            .map(
                |request| match state.gscale.prepare_progress_label(request) {
                    Ok(response) => {
                        serde_json::to_value(response).unwrap_or(serde_json::Value::Null)
                    }
                    Err(_) => training_progress_print_failure(),
                },
            )
            .collect();
    }

    let mut prints = Vec::with_capacity(requests.len());
    let mut queued_requests = Vec::with_capacity(requests.len());
    for request in requests {
        match state.gscale.prepare_progress_label(request.clone()) {
            Ok(mut response) => {
                response.status = "queued".to_string();
                response.printer_status = "server_print_queued".to_string();
                prints.push(serde_json::to_value(response).unwrap_or(serde_json::Value::Null));
                queued_requests.push(request);
            }
            Err(_) => prints.push(training_progress_print_failure()),
        }
    }

    if !queued_requests.is_empty() {
        let gscale = state.gscale.clone();
        let apparatus = apparatus.trim().to_string();
        let order_id = order_id.trim().to_string();
        tokio::spawn(async move {
            for request in queued_requests {
                if let Err(error) = gscale.print_progress_label(request).await {
                    tracing::warn!(
                        error = %error,
                        apparatus = %apparatus,
                        order_id = %order_id,
                        action = ?action,
                        "training progress label print failed"
                    );
                }
            }
        });
    }
    prints
}

fn training_progress_print_failure() -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "status": "failed",
        "code": "training_print_failed",
        "error": "training_progress_label_prepare_failed",
    })
}

fn is_training_apparatus(apparatus: &str, active_apparatuses: &[String]) -> bool {
    active_apparatuses
        .iter()
        .any(|active| canonical_apparatus_matches(active, apparatus))
}

fn training_queue_action_controls(
    apparatus: &str,
    canonical: &RuntimeApparatusConfiguration,
    sequence: &[String],
    states: &BTreeMap<String, String>,
    maps: &[ProductionMapSaved],
    input_progress_batches: &BTreeMap<String, Vec<OrderProgressBatch>>,
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
    let active_order_id = parsed_states
        .iter()
        .find_map(|(order_id, state)| state.is_active().then_some(order_id.as_str()));
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
            let previous_stage = maps
                .iter()
                .find(|saved| saved.map.id.trim() == order_id.trim())
                .and_then(|saved| training_input_stage_for_map(&saved.map, apparatus))
                .unwrap_or_default();
            let input_batches = input_progress_batches
                .get(order_id.trim())
                .map(Vec::as_slice)
                .unwrap_or_default();
            let current_input_batch_id = input_batches
                .iter()
                .find(|batch| {
                    batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && training_input_batch_matches(batch, order_id, &previous_stage, apparatus)
                        && canonical_apparatus_matches(&batch.used_by_apparatus, apparatus)
                })
                .map(|batch| batch.batch_id.as_str())
                .unwrap_or_default();
            let has_unprocessed_previous_wips = !previous_stage.is_empty()
                && training_has_unprocessed_previous_wips(
                    input_batches,
                    order_id,
                    &previous_stage,
                    apparatus,
                    current_input_batch_id,
                );
            let previous_stage_ready = previous_stage.is_empty()
                || input_batches.iter().any(|batch| {
                    training_input_batch_matches(batch, order_id, &previous_stage, apparatus)
                });
            let previous_wip_mode = if previous_stage.is_empty() {
                ApparatusQueuePreviousWipMode::NotRequired
            } else if previous_stage_ready {
                ApparatusQueuePreviousWipMode::ScanRequired
            } else {
                ApparatusQueuePreviousWipMode::Waiting
            };
            let pending_actionable =
                queue_actionable && previous_wip_mode != ApparatusQueuePreviousWipMode::Waiting;
            let allowed_actions = if !queue_actionable {
                Vec::new()
            } else {
                match state {
                    queue_state::ApparatusQueueOrderState::Pending => {
                        if pending_actionable {
                            vec![queue_state::ApparatusQueueAction::Start]
                        } else {
                            Vec::new()
                        }
                    }
                    queue_state::ApparatusQueueOrderState::InProgress => {
                        let mut actions = vec![
                            queue_state::ApparatusQueueAction::Pause,
                            queue_state::ApparatusQueueAction::DetachRoll,
                            queue_state::ApparatusQueueAction::Complete,
                        ];
                        if maps
                            .iter()
                            .find(|saved| saved.map.id.trim() == order_id)
                            .is_some_and(|saved| is_rezka_apparatus(&saved.map, apparatus))
                        {
                            actions.push(queue_state::ApparatusQueueAction::RollComplete);
                        }
                        actions
                    }
                    queue_state::ApparatusQueueOrderState::Paused => {
                        vec![queue_state::ApparatusQueueAction::Resume]
                    }
                    queue_state::ApparatusQueueOrderState::Frozen => Vec::new(),
                    queue_state::ApparatusQueueOrderState::Completed => Vec::new(),
                }
            };
            let interaction = match state {
                queue_state::ApparatusQueueOrderState::Pending if !queue_actionable => {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::FreshStartBlocked,
                        assigned_materials_display_only: true,
                        blocking_reason_code: "waiting_sequence".to_string(),
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
                queue_state::ApparatusQueueOrderState::Pending
                    if previous_wip_mode == ApparatusQueuePreviousWipMode::Waiting =>
                {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::WaitingPreviousStage,
                        assigned_materials_display_only: true,
                        previous_wip_mode,
                        blocking_reason_code: "waiting_previous_stage".to_string(),
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
                queue_state::ApparatusQueueOrderState::Pending => ApparatusQueueWorkerInteraction {
                    mode: ApparatusQueueInteractionMode::FreshStart,
                    assigned_materials_display_only: false,
                    previous_wip_mode,
                    qolip_mode: if pechat::is_pechat_apparatus(canonical) {
                        ApparatusQueueQolipMode::ScanRequired
                    } else {
                        ApparatusQueueQolipMode::NotRequired
                    },
                    ..ApparatusQueueWorkerInteraction::default()
                },
                queue_state::ApparatusQueueOrderState::InProgress => {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::InProgress,
                        material_intake_allowed: true,
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
                queue_state::ApparatusQueueOrderState::Paused => ApparatusQueueWorkerInteraction {
                    mode: ApparatusQueueInteractionMode::Paused,
                    material_intake_allowed: true,
                    ..ApparatusQueueWorkerInteraction::default()
                },
                queue_state::ApparatusQueueOrderState::Frozen => ApparatusQueueWorkerInteraction {
                    mode: ApparatusQueueInteractionMode::Frozen,
                    assigned_materials_display_only: true,
                    blocking_reason_code: "order_frozen".to_string(),
                    ..ApparatusQueueWorkerInteraction::default()
                },
                queue_state::ApparatusQueueOrderState::Completed => {
                    ApparatusQueueWorkerInteraction {
                        mode: ApparatusQueueInteractionMode::Completed,
                        assigned_materials_display_only: true,
                        ..ApparatusQueueWorkerInteraction::default()
                    }
                }
            };
            (
                order_id.clone(),
                ApparatusQueueOrderActionControl {
                    state,
                    allowed_actions,
                    interaction,
                    previous_stage,
                    previous_stage_ready,
                    complete_requires_full_report: maps
                        .iter()
                        .find(|saved| saved.map.id.trim() == order_id)
                        .is_some_and(|saved| {
                            training_complete_requires_full_report(
                                &saved.map,
                                apparatus,
                                has_unprocessed_previous_wips,
                            )
                        }),
                    freeze_request: None,
                },
            )
        })
        .collect()
}

fn training_complete_requires_full_report(
    map: &ProductionMapDefinition,
    apparatus: &str,
    has_unprocessed_previous_wips: bool,
) -> bool {
    !(is_laminatsiya_apparatus(map, apparatus) || is_rezka_apparatus(map, apparatus))
        || !has_unprocessed_previous_wips
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

pub async fn training_input_batches(
    State(state): State<AppState>,
    Query(query): Query<TrainingInputBatchesQuery>,
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
            let maps = if query.order_id.trim().is_empty() {
                store.maps().await.map_err(training_workspace_error)?
            } else {
                store
                    .map(query.order_id.trim())
                    .await
                    .map_err(training_workspace_error)?
                    .into_iter()
                    .collect()
            };
            let mut batches = Vec::new();
            for saved in maps {
                let apparatus = if query.apparatus.trim().is_empty() {
                    training_input_target_apparatus(&saved.map).unwrap_or_default()
                } else {
                    query.apparatus.trim().to_string()
                };
                if apparatus.is_empty() {
                    continue;
                }
                if training_input_stage_for_map(&saved.map, &apparatus).is_none() {
                    continue;
                }
                batches.extend(
                    training_generated_input_progress_batches_for_map(
                        store, &saved.map, &apparatus,
                    )
                    .await
                    .map_err(training_workspace_error)?
                    .into_iter()
                    .filter(|batch| {
                        query.qr_payload.trim().is_empty()
                            || batch
                                .qr_payload
                                .eq_ignore_ascii_case(query.qr_payload.trim())
                    }),
                );
            }
            Ok(json_response(serde_json::json!({"batches": batches})))
        }
        Method::POST => {
            let input: TrainingInputBatchRequest = parse_json(&body)?;
            let order_id = input.order_id.trim();
            if order_id.is_empty() || !order_id.starts_with("training-") {
                return Err(bad_request("training order id kerak"));
            }
            let saved = store
                .map(order_id)
                .await
                .map_err(training_workspace_error)?
                .ok_or_else(|| not_found("training_map_not_found"))?;
            let apparatus = if input.apparatus.trim().is_empty() {
                training_input_target_apparatus(&saved.map).unwrap_or_default()
            } else {
                input.apparatus.trim().to_string()
            };
            let Some(previous_stage) = training_input_stage_for_map(&saved.map, &apparatus) else {
                return Err(bad_request("training_input_batch_not_applicable"));
            };
            let count = input.count.unwrap_or(1);
            if count == 0 || count > 100 {
                return Err(bad_request("training_input_batch_count_invalid"));
            }
            let queue_started = store
                .queue_states()
                .await
                .map_err(training_workspace_error)?
                .iter()
                .any(|(configured_apparatus, states)| {
                    canonical_apparatus_matches(configured_apparatus, &apparatus)
                        && states
                            .get(order_id)
                            .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                            .is_some_and(|state| {
                                state != queue_state::ApparatusQueueOrderState::Pending
                            })
                });
            let input_set_started = store
                .training_input_batch_set_started(order_id, &apparatus)
                .await
                .map_err(training_workspace_error)?;
            if queue_started || input_set_started {
                return Err(bad_request("training_input_batch_set_closed"));
            }
            let identities = store
                .generate_training_input_batches(order_id, &apparatus, &previous_stage, count)
                .await
                .map_err(training_workspace_error)?;
            let batches = identities
                .iter()
                .filter_map(|identity| {
                    training_input_progress_batch(&saved.map, &apparatus, identity)
                })
                .collect::<Vec<_>>();
            if batches.len() != identities.len() {
                return Err(bad_request("training_input_batch_not_applicable"));
            }
            store
                .put_training_progress_batches(&batches)
                .await
                .map_err(training_workspace_error)?;
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "batch": batches.first(),
                "batches": batches,
            })))
        }
        Method::DELETE => {
            let order_id = query.order_id.trim();
            if order_id.is_empty() || !order_id.starts_with("training-") {
                return Err(bad_request("training order id kerak"));
            }
            let deleted = store
                .delete_training_input_batch(order_id, &query.apparatus, &query.qr_payload)
                .await
                .map_err(training_workspace_error)?;
            if deleted.is_empty() {
                return Err(not_found("training_input_batch_not_found"));
            }
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "order_id": order_id,
            })))
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
    snapshot_new_training_order_rezka_kadr_count(&mut input.map, &input.template);

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

pub async fn training_completed_orders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let events = training_store(&state)?
        .completed_queue_events_for_actor(&principal.ref_, 200)
        .await
        .map_err(training_workspace_error)?;
    let completed_orders = events
        .into_iter()
        .map(|event| {
            let status = if event.action == "complete" && event.to_state == "completed" {
                "completed"
            } else {
                "in_progress"
            };
            serde_json::json!({
                "apparatus": event.apparatus,
                "order_id": event.order_id,
                "completed_at_unix": event.created_at_unix,
                "status": status,
                "actor_ref": event.actor_ref,
                "actor_display_name": event.actor_display_name,
            })
        })
        .collect::<Vec<_>>();
    Ok(json_response(serde_json::json!({
        "completed_orders": completed_orders,
    })))
}

pub async fn training_order_statuses(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }

    let store = training_store(&state)?;
    let maps = store.maps().await.map_err(training_workspace_error)?;
    let state_records = store
        .queue_state_records()
        .await
        .map_err(training_workspace_error)?;
    let latest_events = store
        .latest_queue_events()
        .await
        .map_err(training_workspace_error)?;
    let mut statuses = BTreeMap::new();
    for saved in maps {
        let order_id = saved.map.id.trim().to_string();
        if order_id.is_empty() {
            continue;
        }
        let map_apparatus = saved
            .map
            .nodes
            .iter()
            .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .map(|node| node.apparatus_id.trim().to_string())
            .unwrap_or_default();
        let state_record = state_records
            .iter()
            .filter(|record| {
                record.order_id == order_id
                    && (map_apparatus.is_empty()
                        || canonical_apparatus_matches(&record.apparatus, &map_apparatus))
            })
            .max_by_key(|record| record.updated_at_unix);
        let event = latest_events
            .iter()
            .filter(|event| {
                event.order_id == order_id
                    && (map_apparatus.is_empty()
                        || canonical_apparatus_matches(&event.apparatus, &map_apparatus))
            })
            .max_by_key(|event| event.created_at_unix);
        let state = state_record
            .map(|record| record.state.as_str())
            .or_else(|| event.map(|event| event.to_state.as_str()))
            .unwrap_or("pending")
            .to_string();
        let apparatus = state_record
            .map(|record| record.apparatus.clone())
            .or_else(|| event.map(|event| event.apparatus.clone()))
            .unwrap_or(map_apparatus);
        let updated_at_unix = state_record
            .map(|record| record.updated_at_unix)
            .or_else(|| event.map(|event| event.created_at_unix))
            .unwrap_or_default();
        let completed_at_unix = if state == "completed" {
            event
                .filter(|event| event.action == "complete" && event.to_state == "completed")
                .map(|event| event.created_at_unix)
                .unwrap_or_default()
        } else {
            0
        };
        statuses.insert(
            order_id.clone(),
            serde_json::json!({
                "order_id": order_id,
                "apparatus": apparatus,
                "state": state.clone(),
                "status": state,
                "action": event.map(|event| event.action.clone()).unwrap_or_default(),
                "actor_ref": event.map(|event| event.actor_ref.clone()).unwrap_or_default(),
                "actor_display_name": event
                    .map(|event| event.actor_display_name.clone())
                    .unwrap_or_default(),
                "updated_at_unix": updated_at_unix,
                "completed_at_unix": completed_at_unix,
            }),
        );
    }
    Ok(json_response(serde_json::json!({"statuses": statuses})))
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
        Method::DELETE => {
            let order_id = query.order_id.trim();
            let apparatus = query.apparatus.trim();
            let barcode = query.barcode.trim();
            if order_id.is_empty() || apparatus.is_empty() || barcode.is_empty() {
                return Err(bad_request("order_id, apparatus va barcode kerak"));
            }
            let deleted = store
                .delete_raw_material_assignment(order_id, apparatus, barcode)
                .await
                .map_err(training_workspace_error)?;
            if !deleted {
                return Err(not_found("training_material_assignment_not_found"));
            }
            Ok(json_response(serde_json::json!({
                "ok": true,
                "order_id": order_id,
                "apparatus": apparatus,
                "barcode": barcode,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_order_image_upload(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method == Method::DELETE {
        let image_id = query_value(&uri, "id")
            .filter(|value| safe_image_id(value))
            .ok_or_else(|| bad_request("id kerak"))?;
        let owner = owner_key("admin", &principal.ref_);
        let deleted = training_store(&state)?
            .delete_image(&owner, &image_id)
            .await
            .map_err(training_workspace_error)?;
        if !deleted {
            return Err(not_found("rasm topilmadi"));
        }
        return Ok(json_response(serde_json::json!({"ok": true})));
    }
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
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueManage,
        ],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        Principal {
            role: PrincipalRole::Aparatchi,
            display_name: "Rezka operator".to_string(),
            legal_name: String::new(),
            ref_: "rezka-operator".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    fn node(id: &str, kind: ProductionMapNodeKind, title: &str) -> ProductionMapNode {
        let (apparatus_id, role_code) = match (kind.clone(), title) {
            (ProductionMapNodeKind::Apparatus, "Laminatsiya aparat") => (
                "apparatus:test:laminatsiya-1".to_string(),
                TRAINING_LAMINATSIYA_ROLE.to_string(),
            ),
            (ProductionMapNodeKind::Apparatus, "Rezka aparat") => (
                "apparatus:test:rezka-1".to_string(),
                TRAINING_REZKA_ROLE.to_string(),
            ),
            _ => (String::new(), String::new()),
        };
        ProductionMapNode {
            id: id.to_string(),
            kind,
            title: title.to_string(),
            apparatus_id,
            formula: None,
            role_code,
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 0.0,
            y: 0.0,
        }
    }

    fn laminatsiya_training_map() -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: "training-laminatsiya-1".to_string(),
            product_code: "TRAINING-1".to_string(),
            title: "Training laminatsiya".to_string(),
            code: String::new(),
            order_number: String::new(),
            customer_name: String::new(),
            roll_count: None,
            width_mm: None,
            order_kg: Some(12.0),
            base_length: None,
            nodes: vec![
                node("start", ProductionMapNodeKind::Start, "Boshlanish"),
                node(
                    "lam",
                    ProductionMapNodeKind::Apparatus,
                    "Laminatsiya aparat",
                ),
                node("end", ProductionMapNodeKind::End, "Tugash"),
            ],
            edges: vec![
                ProductionMapEdge {
                    from: "start".to_string(),
                    to: "lam".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "lam".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
            ],
        }
    }

    fn training_input_identity() -> TrainingInputBatchIdentity {
        TrainingInputBatchIdentity {
            order_id: "training-laminatsiya-1".to_string(),
            apparatus: "apparatus:test:laminatsiya-1".to_string(),
            batch_id: "progress-batch:1770000000000000000:training-input:bosma:training-laminatsiya-1:complete"
                .to_string(),
            session_id: "training-input-session:progress-batch:1770000000000000000:training-input:bosma:training-laminatsiya-1:complete"
                .to_string(),
            qr_payload: "400118904D9F447100000F96".to_string(),
        }
    }

    fn rezka_training_map() -> ProductionMapDefinition {
        let mut map = laminatsiya_training_map();
        map.id = "training-rezka-1".to_string();
        map.title = "Training rezka".to_string();
        map.nodes[1].id = "rezka".to_string();
        map.nodes[1].title = "Rezka aparat".to_string();
        map.nodes[1].apparatus_id = "apparatus:test:rezka-1".to_string();
        map.nodes[1].role_code = TRAINING_REZKA_ROLE.to_string();
        map.edges[0].to = "rezka".to_string();
        map.edges[1].from = "rezka".to_string();
        map
    }

    fn rezka_training_input_identity() -> TrainingInputBatchIdentity {
        TrainingInputBatchIdentity {
            order_id: "training-rezka-1".to_string(),
            apparatus: "apparatus:test:rezka-1".to_string(),
            batch_id: "progress-batch:1770000000000000000:training-input:laminatsiya:training-rezka-1:complete"
                .to_string(),
            session_id: "training-input-session:progress-batch:1770000000000000000:training-input:laminatsiya:training-rezka-1:complete"
                .to_string(),
            qr_payload: "400118904D9F447100000F97".to_string(),
        }
    }

    #[test]
    fn training_order_request_accepts_mobile_decimal_roll_count() {
        let input: TrainingMapSaveWithOrderRequest = serde_json::from_value(serde_json::json!({
            "map": {
                "id": "training-decimal-roll-count",
                "product_code": "TRAINING-7701",
                "title": "Training decimal roll count",
                "roll_count": 7.0,
                "nodes": [],
                "edges": []
            },
            "template": {
                "name": "training mahsulot",
                "product": "training mahsulot",
                "roll_count": 7.0
            }
        }))
        .expect("training order request");

        assert_eq!(input.map.roll_count, Some(7));
        assert_eq!(input.template.roll_count, Some(7));
    }

    #[test]
    fn new_training_order_snapshots_rezka_frame_count() {
        let mut map = rezka_training_map();
        let template = CalculateOrderTemplate {
            frame_count: 4.0,
            ..CalculateOrderTemplate::default()
        };

        snapshot_new_training_order_rezka_kadr_count(&mut map, &template);

        assert_eq!(map.nodes[1].rezka_kadr_count, Some(4));

        map.order_number = "T-0001".to_string();
        let edited_template = CalculateOrderTemplate {
            frame_count: 8.0,
            ..CalculateOrderTemplate::default()
        };
        snapshot_new_training_order_rezka_kadr_count(&mut map, &edited_template);

        assert_eq!(map.nodes[1].rezka_kadr_count, Some(4));
    }

    #[test]
    fn laminatsiya_training_map_gets_virtual_bosma_input() {
        let map = laminatsiya_training_map();

        assert_eq!(
            training_input_stage_for_map(&map, "apparatus:test:laminatsiya-1").as_deref(),
            Some(TRAINING_VIRTUAL_INPUT_BOSMA)
        );

        let worker_map = training_worker_map(map.clone());
        let input = worker_map
            .nodes
            .iter()
            .find(|item| is_training_input_node(item))
            .expect("virtual training input node");
        assert_eq!(input.title, TRAINING_LAMINATSIYA_INPUT_APPARATUS);
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| { edge.from == "start" && edge.to == input.id })
        );
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| { edge.from == input.id && edge.to == "lam" })
        );

        let identity = training_input_identity();
        let batch =
            training_input_progress_batch(&worker_map, "apparatus:test:laminatsiya-1", &identity)
                .expect("virtual training input batch");
        assert_eq!(batch.qr_payload, identity.qr_payload);
        assert_eq!(batch.batch_id, identity.batch_id);
        assert_eq!(
            crate::core::production_map::progress_qr_payload(&batch.batch_id),
            batch.qr_payload,
        );
        assert_eq!(batch.apparatus, "apparatus:test:laminatsiya-1");
        assert_eq!(batch.next_apparatus, "apparatus:test:laminatsiya-1");
        assert_eq!(batch.wip_status, OrderProgressBatchWipStatus::Waiting);
    }

    #[test]
    fn rezka_training_map_gets_virtual_laminatsiya_input() {
        let map = rezka_training_map();

        assert_eq!(
            training_input_stage_for_map(&map, "apparatus:test:rezka-1").as_deref(),
            Some(TRAINING_VIRTUAL_INPUT_LAMINATSIYA)
        );

        let worker_map = training_worker_map(map);
        let input = worker_map
            .nodes
            .iter()
            .find(|item| is_training_input_node(item))
            .expect("virtual rezka input node");
        assert_eq!(input.title, TRAINING_REZKA_INPUT_APPARATUS);
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| edge.from == "start" && edge.to == input.id)
        );
        assert!(
            worker_map
                .edges
                .iter()
                .any(|edge| edge.from == input.id && edge.to == "rezka")
        );

        let identity = rezka_training_input_identity();
        let batch = training_input_progress_batch(&worker_map, "apparatus:test:rezka-1", &identity)
            .expect("virtual rezka input batch");
        assert_eq!(batch.qr_payload, identity.qr_payload);
        assert_eq!(batch.batch_id, identity.batch_id);
        assert_eq!(batch.apparatus, "apparatus:test:rezka-1");
        assert_eq!(batch.next_apparatus, "apparatus:test:rezka-1");
        assert_eq!(batch.wip_status, OrderProgressBatchWipStatus::Waiting);
    }

    #[test]
    fn training_input_batch_set_uses_partial_then_full_completion() {
        let worker_map = training_worker_map(laminatsiya_training_map());
        let first_identity = training_input_identity();
        let mut second_identity = training_input_identity();
        second_identity.batch_id = "training-input-batch-2".to_string();
        second_identity.session_id = "training-input-session-2".to_string();
        second_identity.qr_payload = progress_qr_payload(&second_identity.batch_id);
        let first = training_input_progress_batch(
            &worker_map,
            "apparatus:test:laminatsiya-1",
            &first_identity,
        )
        .expect("first training input");
        let second = training_input_progress_batch(
            &worker_map,
            "apparatus:test:laminatsiya-1",
            &second_identity,
        )
        .expect("second training input");
        let claimed_first = training_claim_input_batch(
            &first,
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
        );

        assert!(training_has_unprocessed_previous_wips(
            &[claimed_first.clone(), second.clone()],
            "training-laminatsiya-1",
            TRAINING_VIRTUAL_INPUT_BOSMA,
            "apparatus:test:laminatsiya-1",
            &claimed_first.batch_id,
        ));
        assert!(!training_complete_requires_full_report(
            &laminatsiya_training_map(),
            "apparatus:test:laminatsiya-1",
            true,
        ));
        assert!(!training_complete_requires_full_report(
            &rezka_training_map(),
            "apparatus:test:rezka-1",
            true
        ));

        let processed_first = training_process_input_batch(
            &claimed_first,
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
        );
        let claimed_second = training_claim_input_batch(
            &second,
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
        );
        assert!(!training_has_unprocessed_previous_wips(
            &[processed_first, claimed_second.clone()],
            "training-laminatsiya-1",
            TRAINING_VIRTUAL_INPUT_BOSMA,
            "apparatus:test:laminatsiya-1",
            &claimed_second.batch_id,
        ));
        assert!(training_complete_requires_full_report(
            &laminatsiya_training_map(),
            "apparatus:test:laminatsiya-1",
            false,
        ));
        assert!(training_complete_requires_full_report(
            &rezka_training_map(),
            "apparatus:test:rezka-1",
            false
        ));
    }

    #[test]
    fn training_input_qr_order_id_is_case_insensitive() {
        assert_eq!(
            training_input_order_id_from_qr("TRAINING-INPUT:TRAINING-ZAKAZ-0004").as_deref(),
            Some("training-zakaz-0004"),
        );
        assert_eq!(
            training_input_order_id_from_qr("training-input:training-zakaz-0004").as_deref(),
            Some("training-zakaz-0004"),
        );
        assert_eq!(training_input_order_id_from_qr("GSP:PROGRESS-1"), None);
    }

    #[test]
    fn training_selection_is_independent_of_display_rename() {
        let mut map = laminatsiya_training_map();
        let saved = ProductionMapSaved {
            program: crate::core::production_map::ProductionMapProgram {
                map_id: map.id.clone(),
                product_code: map.product_code.clone(),
                operations: Vec::new(),
            },
            map: map.clone(),
        };
        assert!(training_map_has_apparatus(
            &saved,
            "apparatus:test:laminatsiya-1"
        ));
        map.nodes[1].title = "Renamed display only".to_string();
        let renamed = ProductionMapSaved { map, ..saved };
        assert!(training_map_has_apparatus(
            &renamed,
            "apparatus:test:laminatsiya-1"
        ));
        assert!(!training_map_has_apparatus(
            &renamed,
            "Renamed display only"
        ));
    }

    #[test]
    fn training_virtual_input_cannot_be_a_production_apparatus_id() {
        assert!(ApparatusId::new(TRAINING_VIRTUAL_INPUT_BOSMA).is_err());
        assert!(ApparatusId::new(TRAINING_VIRTUAL_INPUT_LAMINATSIYA).is_err());
        let batch = training_input_progress_batch(
            &training_worker_map(laminatsiya_training_map()),
            "apparatus:test:laminatsiya-1",
            &training_input_identity(),
        )
        .expect("training virtual input batch");
        assert_eq!(batch.apparatus, "apparatus:test:laminatsiya-1");
        assert_eq!(
            batch.payload_json["training_virtual_apparatus"],
            TRAINING_VIRTUAL_INPUT_BOSMA
        );
    }

    #[test]
    fn unsupported_training_map_does_not_get_virtual_input() {
        let mut map = laminatsiya_training_map();
        map.nodes[1].title = "Bosma aparat".to_string();
        map.nodes[1].role_code = "other".to_string();

        let worker_map = training_worker_map(map.clone());
        assert!(!worker_map.nodes.iter().any(is_training_input_node));
        assert!(
            training_input_progress_batch(
                &worker_map,
                "apparatus:test:unsupported-1",
                &training_input_identity(),
            )
            .is_none()
        );
    }

    #[test]
    fn training_output_uses_meter_when_progress_quantity_is_missing() {
        let input = TrainingQueuePrintInput {
            gross_qty: Some(250.0),
            finished_goods_kg: Some(250.0),
            finished_goods_meter: Some(6.0),
            bobina_kg: Some(2.0),
            uom: "m".to_string(),
            ..TrainingQueuePrintInput::default()
        };
        let batches = training_progress_batches(
            &laminatsiya_training_map(),
            "apparatus:test:laminatsiya-1",
            "training-laminatsiya-1",
            queue_state::ApparatusQueueAction::Complete,
            &principal(),
            &input,
            None,
            None,
            "",
        )
        .expect("training output batches");

        let batch = batches.first().expect("training output batch");
        assert_eq!(batch.produced_qty, 6.0);
        assert_eq!(batch.finished_goods_kg, Some(250.0));
        let print_request = training_progress_print_request(batch, &input);
        assert_eq!(print_request.progress_qty, 6.0);
        assert_eq!(print_request.gross_qty, 250.0);
    }

    #[test]
    fn rezka_training_output_matches_production_frame_fan_out() {
        let mut map = rezka_training_map();
        map.nodes[1].rezka_kadr_count = Some(4);
        map.nodes[1].rezka_label_length = Some(250.0);
        let input = TrainingQueuePrintInput {
            progress_qty: Some(100.0),
            gross_qty: Some(104.0),
            finished_goods_kg: Some(100.0),
            finished_goods_meter: Some(900.0),
            bobina_kg: Some(4.0),
            rezka_bosma_waste: Some(1.0),
            rezka_lamination_waste: Some(2.0),
            rezka_edge_waste: Some(3.0),
            total_waste: Some(6.0),
            diameter: Some(42.0),
            uom: "kg".to_string(),
            ..TrainingQueuePrintInput::default()
        };

        let batches = training_progress_batches(
            &map,
            "apparatus:test:rezka-1",
            "training-rezka-1",
            queue_state::ApparatusQueueAction::Complete,
            &principal(),
            &input,
            None,
            None,
            "input-batch-1",
        )
        .expect("rezka training output batches");

        assert_eq!(batches.len(), 4);
        let batch_ids = batches
            .iter()
            .map(|batch| batch.batch_id.as_str())
            .collect::<BTreeSet<_>>();
        let qr_payloads = batches
            .iter()
            .map(|batch| batch.qr_payload.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(batch_ids.len(), 4);
        assert_eq!(qr_payloads.len(), 4);
        for (index, batch) in batches.iter().enumerate() {
            assert!(batch.batch_id.ends_with(&format!(":frame:{}", index + 1)));
            assert_eq!(progress_qr_payload(&batch.batch_id), batch.qr_payload);
            assert_eq!(batch.parent_batch_id, "input-batch-1");
            assert_eq!(batch.payload_json["rezka_frame_index"], index + 1);
            assert_eq!(batch.payload_json["rezka_frame_count"], 4);
            assert_eq!(batch.payload_json["rezka_label_length"], 250.0);
            assert_eq!(batch.finished_goods_kg, Some(100.0));
            assert_eq!(batch.finished_goods_meter, Some(900.0));
            assert_eq!(
                batch.payload_json["rezka_metrics_owner"],
                serde_json::json!(index == 0),
            );
            let print_request = training_progress_print_request(batch, &input);
            assert_eq!(print_request.qr_payload, batch.qr_payload);
            assert_eq!(print_request.progress_qty, batch.produced_qty);
        }
        assert_eq!(batches[0].diameter, Some(42.0));
        assert_eq!(batches[0].total_waste, Some(6.0));
        assert_eq!(batches[0].bobina_kg, Some(4.0));
        assert_eq!(batches[1].diameter, None);
        assert_eq!(batches[1].total_waste, None);
        assert_eq!(batches[1].bobina_kg, None);
        assert_eq!(batches[1].rezka_bosma_waste, None);
        assert_eq!(batches[1].rezka_lamination_waste, None);
        assert_eq!(batches[1].rezka_edge_waste, None);

        for action in [
            queue_state::ApparatusQueueAction::Pause,
            queue_state::ApparatusQueueAction::DetachRoll,
            queue_state::ApparatusQueueAction::RollComplete,
        ] {
            let action_batches = training_progress_batches(
                &map,
                "apparatus:test:rezka-1",
                "training-rezka-1",
                action,
                &principal(),
                &input,
                None,
                None,
                "input-batch-1",
            )
            .expect("rezka action output batches");
            assert_eq!(action_batches.len(), 4);
            assert!(action_batches.iter().all(|batch| batch.action == action));
            assert!(action_batches.iter().all(|batch| {
                training_progress_print_request(batch, &input).qr_payload == batch.qr_payload
            }));
        }
    }

    #[test]
    fn rezka_training_output_requires_kadr_count_before_state_change() {
        let error = training_progress_batches(
            &rezka_training_map(),
            "apparatus:test:rezka-1",
            "training-rezka-1",
            queue_state::ApparatusQueueAction::DetachRoll,
            &principal(),
            &TrainingQueuePrintInput::default(),
            None,
            None,
            "",
        )
        .expect_err("missing rezka kadr count");

        assert!(matches!(
            error,
            TrainingWorkspaceError::InvalidInput(code)
                if code == "rezka_kadr_count_required"
        ));
    }
}
