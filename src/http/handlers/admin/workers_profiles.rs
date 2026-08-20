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
    let assigned_apparatus = state
        .admin
        .role_assignments()
        .await
        .map_err(|_| server_error("admin role assignments failed"))?
        .into_iter()
        .filter(|assignment| {
            assignment.principal_role == PrincipalRole::Aparatchi
                && assignment
                    .principal_ref
                    .trim()
                    .eq_ignore_ascii_case(worker.id.trim())
        })
        .flat_map(|assignment| assignment.assigned_apparatus)
        .map(|apparatus| apparatus.trim().to_string())
        .filter(|apparatus| !apparatus.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
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
        assigned_apparatus,
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

pub(super) async fn worker_user_list_page(
    state: &AppState,
    query: &PageQuery,
) -> Result<AdminUserListPage, AdminError> {
    let limit = optional_search_limit(query.limit.as_deref(), 20, 50);
    let offset = optional_offset(query.offset.as_deref());
    let workers = state
        .workers
        .workers(
            query.q.as_deref().unwrap_or_default(),
            offset.saturating_add(limit).saturating_add(1),
        )
        .await
        .map_err(worker_error)?;
    let has_more = workers.len() > offset.saturating_add(limit);
    let items = state
        .admin
        .worker_user_list_entries(workers.into_iter().skip(offset).take(limit).collect())
        .await
        .map_err(|_| server_error("worker detail failed"))?;
    Ok(AdminUserListPage { items, has_more })
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
    let id = worker.id.trim();
    (!id.is_empty())
        .then(|| id.to_string())
        .into_iter()
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
            let apparatus_id = parse_apparatus_id(query.apparatus_id.as_deref())?;
            let groups = state
                .worker_groups
                .worker_groups(apparatus_id.as_ref())
                .await
                .map_err(worker_group_error)?;
            let responses = enrich_worker_groups(&state, groups).await?;
            Ok(json_response(responses))
        }
        Method::PUT => {
            let input: WorkerGroupUpsert = parse_json(&body)?;
            validate_worker_ids(&state, &input.worker_ids).await?;
            // Apparatus access belongs to the worker role assignment. Group writes
            // use the canonical apparatus ID; the optional apparatus field is a
            // display snapshot only.
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
