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
    let mut items = Vec::new();
    for worker in workers.into_iter().skip(offset).take(limit) {
        let detail = state
            .admin
            .worker_detail(worker)
            .await
            .map_err(|_| server_error("worker detail failed"))?;
        items.push(AdminUserListEntry {
            id: format!("worker:{}", detail.id),
            source: "worker".to_string(),
            entity_ref: detail.id,
            principal_role: PrincipalRole::Aparatchi,
            name: detail.name,
            phone: detail.phone,
            avatar_url: detail.avatar_url,
            role_label: detail.level,
            blocked: false,
            status: "active".to_string(),
        });
    }
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
                assigned_item_groups: Vec::new(),
            })
            .await
            .map_err(|_| server_error("admin role assignment save failed"))?;
    }
    Ok(())
}

