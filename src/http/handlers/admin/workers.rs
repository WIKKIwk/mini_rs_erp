use super::*;

use crate::core::admin::models::AdminWorkerDetail;
use crate::core::auth::models::PrincipalRole;
use crate::core::production_map::{OrderProgressBatch, OrderRunSession, ProductionOrderLogEntry};
use crate::core::worker_groups::{WorkerGroupError, WorkerGroupRecord, WorkerGroupUpsert};
use crate::core::workers::Worker;
use crate::core::workers::{WorkerError, WorkerUpsert};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
pub struct WorkerGroupQuery {
    apparatus: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkerIdQuery {
    id: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkerGroupResponse {
    apparatus: String,
    group_code: String,
    shift: String,
    start_time: String,
    end_time: String,
    work_days_per_week: i32,
    start_day: String,
    accounting_enabled: bool,
    worker_ids: Vec<String>,
    workers: Vec<Worker>,
}

#[derive(Debug, Serialize)]
struct WorkerProfileDetailResponse {
    worker: AdminWorkerDetail,
    assigned_groups: Vec<WorkerGroupResponse>,
    active_sessions: Vec<OrderRunSession>,
    recent_batches: Vec<OrderProgressBatch>,
    recent_logs: Vec<ProductionOrderLogEntry>,
}

pub async fn workers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if !matches!(method, Method::GET | Method::POST | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => state
            .workers
            .workers(
                query.q.as_deref().unwrap_or(""),
                optional_search_limit(query.limit.as_deref(), 50, 500),
            )
            .await
            .map(json_response)
            .map_err(worker_error),
        Method::POST => {
            let input: WorkerUpsert = parse_json(&body)?;
            state
                .workers
                .upsert_worker(input)
                .await
                .map(json_response)
                .map_err(worker_error)
        }
        Method::PUT => {
            let input: WorkerUpsert = parse_json(&body)?;
            if input.level.trim().is_empty() && !input.phone.trim().is_empty() {
                state
                    .workers
                    .update_worker_phone(input)
                    .await
                    .map(json_response)
                    .map_err(worker_error)
            } else {
                state
                    .workers
                    .update_worker_level(input)
                    .await
                    .map(json_response)
                    .map_err(worker_error)
            }
        }
        _ => Err(method_not_allowed()),
    }
}

fn worker_error(error: WorkerError) -> AdminError {
    match error {
        WorkerError::MissingName => bad_request("worker name is required"),
        WorkerError::MissingId => bad_request("worker id is required"),
        WorkerError::InvalidLevel => bad_request("worker level is invalid"),
        WorkerError::NotFound => not_found("worker not found"),
        WorkerError::StoreFailed => server_error("worker store failed"),
    }
}

pub async fn worker_profile_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<WorkerIdQuery>,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let worker = required_worker(&state, query.id.as_deref()).await?;
    let mut detail = state
        .admin
        .worker_detail(worker.clone())
        .await
        .map_err(|_| server_error("worker detail failed"))?;
    detail.avatar_url =
        with_admin_profile_avatar_proxy(&headers, detail.avatar_url, "worker", &detail.id);
    let assigned_groups = state
        .worker_groups
        .worker_groups(None)
        .await
        .map_err(worker_group_error)?
        .into_iter()
        .filter(|group| {
            group
                .worker_ids
                .iter()
                .any(|id| id.trim() == worker.id.trim())
        })
        .collect::<Vec<_>>();
    let assigned_groups = enrich_worker_groups(&state, assigned_groups).await?;
    let refs = worker_activity_refs(&worker);
    let active_sessions = state
        .production_maps
        .active_order_run_sessions_for_worker(&refs, &worker.name, 50)
        .await
        .map_err(|_| server_error("worker activity failed"))?;
    let recent_batches = state
        .production_maps
        .progress_batches_for_worker(&refs, &worker.name, 50)
        .await
        .map_err(|_| server_error("worker activity failed"))?;
    let recent_logs = state
        .production_maps
        .queue_action_logs_for_worker(&refs, &worker.name, 100)
        .await
        .map_err(|_| server_error("worker activity failed"))?;
    Ok(json_response(WorkerProfileDetailResponse {
        worker: detail,
        assigned_groups,
        active_sessions,
        recent_batches,
        recent_logs,
    }))
}

pub async fn worker_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<WorkerIdQuery>,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let worker = required_worker(&state, query.id.as_deref()).await?;
    let mut detail = state
        .admin
        .worker_detail(worker)
        .await
        .map_err(|_| server_error("worker detail failed"))?;
    detail.avatar_url =
        with_admin_profile_avatar_proxy(&headers, detail.avatar_url, "worker", &detail.id);
    Ok(json_response(detail))
}

pub async fn worker_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<WorkerIdQuery>,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let worker = required_worker(&state, query.id.as_deref()).await?;
    state
        .admin
        .regenerate_worker_code(worker)
        .await
        .map(json_response)
        .map_err(|_| server_error("worker code regenerate failed"))
}

fn worker_activity_refs(worker: &Worker) -> Vec<String> {
    [worker.id.trim(), worker.phone.trim()]
        .into_iter()
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

async fn required_worker(state: &AppState, id: Option<&str>) -> Result<Worker, AdminError> {
    let id = id.unwrap_or("").trim();
    if id.is_empty() {
        return Err(bad_request("worker id is required"));
    }
    let ids = vec![id.to_string()];
    state
        .workers
        .workers_by_ids(&ids)
        .await
        .map_err(worker_error)?
        .into_iter()
        .next()
        .ok_or_else(|| not_found("worker not found"))
}

pub async fn worker_groups(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<WorkerGroupQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }

    match method {
        Method::GET => {
            let groups = state
                .worker_groups
                .worker_groups(query.apparatus.as_deref())
                .await
                .map_err(worker_group_error)?;
            let responses = enrich_worker_groups(&state, groups).await?;
            Ok(json_response(responses))
        }
        Method::PUT => {
            let input: WorkerGroupUpsert = parse_json(&body)?;
            validate_worker_ids(&state, &input.worker_ids).await?;
            let previous_groups = state
                .worker_groups
                .worker_groups(None)
                .await
                .map_err(worker_group_error)?;
            let saved = state
                .worker_groups
                .upsert_group(input)
                .await
                .map_err(worker_group_error)?;
            let affected_worker_ids = previous_groups
                .iter()
                .filter(|group| group.group_code == saved.group_code)
                .flat_map(|group| group.worker_ids.iter())
                .chain(saved.worker_ids.iter())
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect::<BTreeSet<_>>();
            sync_worker_group_apparatchi_assignments(&state, &affected_worker_ids).await?;
            let mut responses = enrich_worker_groups(&state, vec![saved]).await?;
            Ok(json_response(responses.pop().ok_or_else(|| {
                server_error("worker group store failed")
            })?))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn sync_worker_group_apparatchi_assignments(
    state: &AppState,
    affected_worker_ids: &BTreeSet<String>,
) -> Result<(), AdminError> {
    if affected_worker_ids.is_empty() {
        return Ok(());
    }

    let groups = state
        .worker_groups
        .worker_groups(None)
        .await
        .map_err(worker_group_error)?;
    let mut apparatus_by_worker = BTreeMap::<String, BTreeSet<String>>::new();
    for group in groups {
        let apparatus = group.apparatus.trim();
        if apparatus.is_empty() {
            continue;
        }
        for worker_id in group.worker_ids.iter().map(|id| id.trim()) {
            if worker_id.is_empty() {
                continue;
            }
            apparatus_by_worker
                .entry(worker_id.to_string())
                .or_default()
                .insert(apparatus.to_string());
        }
    }

    for worker_id in affected_worker_ids {
        let assigned_apparatus = apparatus_by_worker
            .remove(worker_id)
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        if assigned_apparatus.is_empty() {
            state
                .admin
                .delete_role_assignment(&PrincipalRole::Aparatchi, worker_id)
                .await
                .map_err(|_| server_error("admin role assignment delete failed"))?;
            continue;
        }
        state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: worker_id.clone(),
                role_id: "aparatchi".to_string(),
                assigned_apparatus,
            })
            .await
            .map_err(|_| server_error("admin role assignment save failed"))?;
    }
    Ok(())
}

async fn validate_worker_ids(state: &AppState, ids: &[String]) -> Result<(), AdminError> {
    let ids = ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let workers = state
        .workers
        .workers_by_ids(&ids)
        .await
        .map_err(worker_error)?;
    let found = workers
        .into_iter()
        .map(|worker| worker.id)
        .collect::<BTreeSet<_>>();
    if ids.iter().any(|id| !found.contains(id)) {
        return Err(bad_request("worker not found"));
    }
    Ok(())
}

async fn enrich_worker_groups(
    state: &AppState,
    groups: Vec<WorkerGroupRecord>,
) -> Result<Vec<WorkerGroupResponse>, AdminError> {
    let ids = groups
        .iter()
        .flat_map(|group| group.worker_ids.iter())
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let workers = state
        .workers
        .workers_by_ids(&ids)
        .await
        .map_err(worker_error)?
        .into_iter()
        .map(|worker| (worker.id.clone(), worker))
        .collect::<BTreeMap<_, _>>();

    Ok(groups
        .into_iter()
        .map(|group| {
            let group_workers = group
                .worker_ids
                .iter()
                .filter_map(|id| workers.get(id).cloned())
                .collect::<Vec<_>>();
            WorkerGroupResponse {
                apparatus: group.apparatus,
                group_code: group.group_code,
                shift: group.shift,
                start_time: group.start_time,
                end_time: group.end_time,
                work_days_per_week: group.work_days_per_week,
                start_day: group.start_day,
                accounting_enabled: group.accounting_enabled,
                worker_ids: group.worker_ids,
                workers: group_workers,
            }
        })
        .collect())
}

fn worker_group_error(error: WorkerGroupError) -> AdminError {
    match error {
        WorkerGroupError::MissingApparatus => bad_request("apparatus is required"),
        WorkerGroupError::InvalidGroup => bad_request("worker group is invalid"),
        WorkerGroupError::InvalidShift => bad_request("worker shift is invalid"),
        WorkerGroupError::InvalidSchedule => bad_request("worker schedule is invalid"),
        WorkerGroupError::DuplicateWorker => {
            bad_request("worker is duplicated in apparatus groups")
        }
        WorkerGroupError::StoreFailed => server_error("worker group store failed"),
    }
}
