pub async fn activity(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminActivityRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    match state.werka.history().await {
        Ok(Some(history)) => state
            .admin
            .activity(history)
            .await
            .map(Json)
            .map_err(|_| server_error("admin activity failed")),
        Ok(None) => Ok(Json(Vec::new())),
        Err(_) => Err(server_error("admin activity failed")),
    }
}

pub async fn customer_phone(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::CustomerDirectoryManage).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminPhoneUpdateRequest = parse_json(&body)?;
    state
        .admin
        .update_customer_phone(ref_, &input.phone)
        .await
        .map(Json)
        .map_err(|_| server_error("customer phone update failed"))
}

pub async fn customer_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::CustomerCodeManage).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    state
        .admin
        .regenerate_customer_code(ref_)
        .await
        .map(Json)
        .map_err(|_| server_error("customer code regenerate failed"))
}

pub async fn customer_item_add(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::CustomerItemAssign).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierItemMutationRequest = parse_json(&body)?;
    match state
        .admin
        .assign_customer_item(ref_, &input.item_code)
        .await
    {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(_) => Err(server_error("customer item add failed")),
    }
}

pub async fn customer_item_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefItemQuery>,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::CustomerItemAssign).await?;
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    let (ref_, item_code) = required_ref_item(query.ref_.as_deref(), query.item_code.as_deref())?;
    match state.admin.unassign_customer_item(ref_, item_code).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
        Err(_) => Err(server_error("customer item remove failed")),
    }
}

pub async fn customer_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<OkResponse>, AdminError> {
    authorize_capability(&state, &headers, Capability::CustomerDirectoryManage).await?;
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.remove_customer(ref_).await {
        Ok(()) => Ok(Json(OkResponse { ok: true })),
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(_) => Err(server_error("customer remove failed")),
    }
}
