use std::collections::BTreeSet;

use super::*;
use crate::core::{
    apparatus_standard::LifecycleState,
    authz::{RoleAssignment, RoleAssignmentUpsert},
    production_map::{ProductionMapDefinition, chain},
    workers::Worker,
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

struct WorkerScope {
    known_cut: BTreeSet<String>,
    active_cut: BTreeSet<String>,
    cut_group: BTreeSet<String>,
    other_group: BTreeSet<String>,
}

impl WorkerScope {
    async fn read(state: &AppState) -> Result<Self, AdminError> {
        let apparatus = state
            .rezka_apparatus()
            .await
            .map_err(|_| server_error("rezka apparatus lookup failed"))?;
        let known_cut: BTreeSet<_> = apparatus
            .iter()
            .map(|row| row.apparatus_id.to_string())
            .collect();
        let active_cut = apparatus
            .iter()
            .filter(|row| row.lifecycle.state == LifecycleState::Active)
            .map(|row| row.apparatus_id.to_string())
            .collect();
        let mut cut_group = BTreeSet::new();
        let mut other_group = BTreeSet::new();
        for group in state
            .worker_groups
            .worker_groups(None)
            .await
            .map_err(|_| server_error("worker group lookup failed"))?
        {
            let members = if known_cut.contains(group.apparatus_id.as_str()) {
                &mut cut_group
            } else {
                &mut other_group
            };
            members.extend(group.worker_ids);
        }
        Ok(Self {
            known_cut,
            active_cut,
            cut_group,
            other_group,
        })
    }

    fn allows(&self, worker: &Worker, assignment: Option<&RoleAssignment>) -> bool {
        let assigned = assignment
            .map(|row| row.assigned_apparatus.as_slice())
            .unwrap_or_default();
        self.cut_group.contains(&worker.id)
            || assigned.iter().any(|id| self.known_cut.contains(id))
            || (assigned.is_empty() && !self.other_group.contains(&worker.id))
    }

    fn visible_apparatus(&self, assignment: Option<&RoleAssignment>) -> BTreeSet<String> {
        assignment
            .into_iter()
            .flat_map(|row| &row.assigned_apparatus)
            .filter(|id| self.active_cut.contains(*id))
            .cloned()
            .collect()
    }
}

pub async fn workers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_master(&state, &headers).await?;
    let scope = WorkerScope::read(&state).await?;
    let assignments = state
        .admin
        .role_assignments()
        .await
        .map_err(|_| server_error("worker assignment lookup failed"))?;
    let by_worker = |worker: &Worker| {
        assignments
            .iter()
            .find(|row| {
                row.principal_role == PrincipalRole::Aparatchi && row.principal_ref == worker.id
            })
            .or_else(|| {
                assignments.iter().find(|row| {
                    row.principal_role == PrincipalRole::Customer && row.principal_ref == worker.id
                })
            })
    };
    let workers = state
        .workers
        .workers("", 500)
        .await
        .map_err(|_| server_error("worker lookup failed"))?;
    let mut visible_workers = Vec::new();
    let mut visible_assignments = Vec::new();
    for worker in workers {
        let assignment = by_worker(&worker);
        if scope.allows(&worker, assignment) {
            visible_assignments.push(serde_json::json!({
                "principal_role": "aparatchi",
                "principal_ref": worker.id,
                "role_id": assignment.map(|row| row.role_id.as_str()).unwrap_or("aparatchi"),
                "assigned_apparatus": scope.visible_apparatus(assignment),
            }));
            visible_workers.push(worker);
        }
    }
    Ok(json_response(serde_json::json!({
        "workers": visible_workers, "assignments": visible_assignments,
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
    let scope = WorkerScope::read(&state).await?;
    let current = state
        .admin
        .principal_assignment(&principal)
        .await
        .map_err(|_| server_error("worker assignment lookup failed"))?;
    if !scope.allows(&worker, current.as_ref()) {
        return Err(not_found("worker_not_found"));
    }
    let selected: BTreeSet<_> = input.apparatus_ids.into_iter().collect();
    if !selected.is_subset(&scope.active_cut) {
        return Err(bad_request("rezka_apparatus_required"));
    }
    let previous: BTreeSet<_> = input.previous_apparatus_ids.into_iter().collect();
    if scope.visible_apparatus(current.as_ref()) != previous {
        return Err(conflict("rezka_assignment_changed"));
    }
    let mut next: BTreeSet<_> = current
        .iter()
        .flat_map(|row| &row.assigned_apparatus)
        .filter(|id| !scope.active_cut.contains(*id))
        .cloned()
        .collect();
    next.extend(selected);
    if previous.iter().any(|id| !next.contains(id)) {
        let sessions = state
            .production_maps
            .active_order_run_sessions_for_worker(std::slice::from_ref(&worker.id), "", 500)
            .await
            .map_err(production_maps::production_map_error)?;
        if sessions
            .iter()
            .any(|run| previous.contains(&run.apparatus) && !next.contains(&run.apparatus))
        {
            return Err(conflict("rezka_worker_has_open_run"));
        }
    }
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: worker.id.clone(),
            role_id: current
                .as_ref()
                .map(|row| row.role_id.clone())
                .unwrap_or_else(|| "aparatchi".into()),
            assigned_apparatus: next.into_iter().collect(),
            assigned_item_groups: current
                .map(|row| row.assigned_item_groups)
                .unwrap_or_default(),
        })
        .await
        .map_err(|_| server_error("worker assignment save failed"))?;
    state.warehouse_events.invalidate_inventory();
    state.production_maps.notify_live();
    tracing::info!(actor_ref = %master.ref_, worker_ref = %worker.id, "updated Rezka worker apparatus");
    Ok(json_response(serde_json::json!({ "ok": true })))
}
