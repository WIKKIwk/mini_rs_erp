use super::*;

use crate::core::worker_groups::{WorkerGroupError, WorkerGroupRecord, WorkerGroupUpsert};
use crate::core::workers::Worker;
use crate::core::workers::{WorkerError, WorkerUpsert};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
pub struct WorkerGroupQuery {
    apparatus: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkerGroupResponse {
    apparatus: String,
    group_code: String,
    shift: String,
    worker_ids: Vec<String>,
    workers: Vec<Worker>,
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
            state
                .workers
                .update_worker_level(input)
                .await
                .map(json_response)
                .map_err(worker_error)
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
            let saved = state
                .worker_groups
                .upsert_group(input)
                .await
                .map_err(worker_group_error)?;
            let mut responses = enrich_worker_groups(&state, vec![saved]).await?;
            Ok(json_response(responses.pop().ok_or_else(|| {
                server_error("worker group store failed")
            })?))
        }
        _ => Err(method_not_allowed()),
    }
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
        WorkerGroupError::DuplicateWorker => {
            bad_request("worker is duplicated in apparatus groups")
        }
        WorkerGroupError::StoreFailed => server_error("worker group store failed"),
    }
}
