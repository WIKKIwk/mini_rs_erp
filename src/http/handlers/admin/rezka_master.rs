use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::core::{
    apparatus_standard::LifecycleState,
    authz::{RoleAssignment, RoleAssignmentUpsert},
    production_map::{ProductionMapDefinition, chain},
};

async fn authorize_master(state: &AppState, headers: &HeaderMap) -> Result<Principal, AdminError> {
    let principal = authorize_capability(state, headers, Capability::RezkaMasterAccess).await?;
    if principal.role != PrincipalRole::RezkaMaster {
        return Err(forbidden());
    }
    Ok(principal)
}

pub(super) async fn require_order_scope(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
) -> Result<ProductionMapDefinition, AdminError> {
    let id = order_id.trim();
    if id.is_empty() || id.starts_with("training-") || id.starts_with("template-") {
        return Err(not_found("map_not_found"));
    }
    let map = state
        .production_maps
        .raw_map(id)
        .await
        .map_err(production_maps::production_map_error)?
        .ok_or_else(|| not_found("map_not_found"))?;
    let allowed = state.principal_assigned_apparatus(principal).await;
    if !chain::linear_work_stages(&map).iter().any(|stage| {
        stage
            .apparatus_id
            .as_ref()
            .is_some_and(|id| allowed.contains(id))
    }) {
        return Err(not_found("map_not_found"));
    }
    Ok(map)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderSettings {
    order_id: String,
    duplicate_rezka_labels: bool,
    include_bobina_in_net: bool,
    previous_duplicate_rezka_labels: bool,
    previous_include_bobina_in_net: bool,
}

pub async fn order_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<OrderSettings>,
) -> Result<Response, AdminError> {
    let principal = authorize_master(&state, &headers).await?;
    let _guard = state.production_maps.queue_action_guard().await;
    let mut map = require_order_scope(&state, &principal, &input.order_id).await?;
    let allowed = state.principal_assigned_apparatus(&principal).await;
    let final_cut_stages: Vec<_> = chain::linear_work_stages(&map)
        .into_iter()
        .filter(|stage| {
            stage
                .apparatus_id
                .as_ref()
                .is_some_and(|id| allowed.contains(id))
                && chain::is_final_work_stage_node(&map, &stage.node_id)
        })
        .collect();
    if final_cut_stages.is_empty() {
        return Err(bad_request("final_rezka_stage_required"));
    }
    let duplicate = map.duplicate_rezka_labels.unwrap_or(false);
    let include_bobina = map.include_bobina_in_net.unwrap_or(false);
    if (duplicate, include_bobina) != (input.duplicate_rezka_labels, input.include_bobina_in_net)
        && (duplicate, include_bobina)
            != (
                input.previous_duplicate_rezka_labels,
                input.previous_include_bobina_in_net,
            )
    {
        return Err(conflict("rezka_order_settings_changed"));
    }
    if include_bobina != input.include_bobina_in_net {
        let sessions = state
            .production_maps
            .order_run_sessions_for_order(&map.id)
            .await
            .map_err(production_maps::production_map_error)?;
        if sessions.iter().any(|run| {
            run.status == crate::core::production_map::OrderRunStatus::Active
                && final_cut_stages.iter().any(|stage| {
                    stage.apparatus_id.as_deref() == Some(&run.apparatus)
                        && (run.stage_node_id.is_empty() || run.stage_node_id == stage.node_id)
                })
        }) {
            return Err(conflict("rezka_weight_mode_run_active"));
        }
    }
    map.duplicate_rezka_labels = Some(input.duplicate_rezka_labels);
    map.include_bobina_in_net = Some(input.include_bobina_in_net);
    let saved = state
        .production_maps
        .upsert_map_under_queue_guard(map)
        .await
        .map_err(production_maps::production_map_error)?;
    tracing::info!(actor_ref = %principal.ref_, order_id = %saved.map.id, "updated Rezka order settings");
    Ok(json_response(saved))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerApparatus {
    worker_id: String,
    apparatus_ids: Vec<String>,
    previous_apparatus_ids: Vec<String>,
}

async fn active_cut_ids(state: &AppState) -> Result<BTreeSet<String>, AdminError> {
    let apparatus = state
        .rezka_apparatus()
        .await
        .map_err(|_| server_error("rezka apparatus lookup failed"))?;
    Ok(apparatus
        .into_iter()
        .filter(|row| row.lifecycle.state == LifecycleState::Active)
        .map(|row| row.apparatus_id.to_string())
        .collect())
}

fn assigned_cut_ids(
    assignment: &RoleAssignment,
    active_cut: &BTreeSet<String>,
) -> BTreeSet<String> {
    assignment
        .assigned_apparatus
        .iter()
        .filter(|id| active_cut.contains(*id))
        .cloned()
        .collect()
}

pub async fn workers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_master(&state, &headers).await?;
    let active_cut = active_cut_ids(&state).await?;
    let assignments = state
        .admin
        .role_assignments()
        .await
        .map_err(|_| server_error("worker assignment lookup failed"))?;
    let mut by_worker = BTreeMap::new();
    // Apply legacy assignments first so Aparatchi assignments take precedence.
    for role in [PrincipalRole::Customer, PrincipalRole::Aparatchi] {
        for assignment in assignments.iter().filter(|row| row.principal_role == role) {
            by_worker.insert(assignment.principal_ref.trim().to_string(), assignment);
        }
    }
    let by_worker: BTreeMap<_, _> = by_worker
        .into_iter()
        .filter_map(|(id, assignment)| {
            let cut = assigned_cut_ids(assignment, &active_cut);
            (!cut.is_empty()).then_some((id, (assignment, cut)))
        })
        .collect();
    let ids: Vec<_> = by_worker.keys().cloned().collect();
    let mut workers = state
        .workers
        .workers_by_ids(&ids)
        .await
        .map_err(|_| server_error("worker lookup failed"))?;
    workers.sort_by_cached_key(|worker| (worker.name.to_lowercase(), worker.id.clone()));
    let visible_assignments: Vec<_> = workers
        .iter()
        .map(|worker| {
            let (assignment, cut) = &by_worker[&worker.id];
            serde_json::json!({
                "principal_role": "aparatchi",
                "principal_ref": worker.id,
                "role_id": assignment.role_id,
                "assigned_apparatus": cut,
            })
        })
        .collect();
    Ok(json_response(serde_json::json!({
        "workers": workers, "assignments": visible_assignments,
    })))
}

pub async fn assign_worker(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<WorkerApparatus>,
) -> Result<Response, AdminError> {
    let master = authorize_master(&state, &headers).await?;
    let _guard = state.production_maps.queue_action_guard().await;
    let worker_id = input.worker_id.trim();
    let worker = state
        .workers
        .workers_by_ids(&[worker_id.to_string()])
        .await
        .map_err(|_| server_error("worker lookup failed"))?
        .into_iter()
        .next()
        .ok_or_else(|| not_found("worker_not_found"))?;
    let principal = Principal {
        role: PrincipalRole::Aparatchi,
        ref_: worker.id.clone(),
        display_name: worker.name.clone(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let active_cut = active_cut_ids(&state).await?;
    let current = state
        .admin
        .principal_assignment(&principal)
        .await
        .map_err(|_| server_error("worker assignment lookup failed"))?
        .ok_or_else(|| not_found("worker_not_found"))?;
    let current_cut = assigned_cut_ids(&current, &active_cut);
    if current_cut.is_empty() {
        return Err(not_found("worker_not_found"));
    }
    let selected: BTreeSet<_> = input.apparatus_ids.into_iter().collect();
    if selected.is_empty() || !selected.is_subset(&active_cut) {
        return Err(bad_request("rezka_apparatus_required"));
    }
    let previous: BTreeSet<_> = input.previous_apparatus_ids.into_iter().collect();
    if current_cut != previous {
        return Err(conflict("rezka_assignment_changed"));
    }
    let mut next: BTreeSet<_> = current
        .assigned_apparatus
        .into_iter()
        .filter(|id| !active_cut.contains(id))
        .collect();
    next.extend(selected);
    let removed: BTreeSet<_> = previous.difference(&next).collect();
    if !removed.is_empty() {
        let sessions = state
            .production_maps
            .active_order_run_sessions_for_worker(std::slice::from_ref(&worker.id), "", 500)
            .await
            .map_err(production_maps::production_map_error)?;
        let blocked_apparatus_ids: BTreeSet<_> = sessions
            .into_iter()
            .filter(|run| removed.contains(&run.apparatus))
            .map(|run| run.apparatus)
            .collect();
        if !blocked_apparatus_ids.is_empty() {
            let (status, Json(mut error)) = conflict("rezka_worker_has_open_run");
            error.blocked_apparatus_ids = Some(blocked_apparatus_ids);
            return Err((status, Json(error)));
        }
    }
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: worker.id.clone(),
            role_id: current.role_id,
            assigned_apparatus: next.into_iter().collect(),
            assigned_item_groups: current.assigned_item_groups,
        })
        .await
        .map_err(|_| server_error("worker assignment save failed"))?;
    state.warehouse_events.invalidate_inventory();
    state.production_maps.notify_live();
    tracing::info!(actor_ref = %master.ref_, worker_ref = %worker.id, "updated Rezka worker apparatus");
    Ok(json_response(serde_json::json!({ "ok": true })))
}
