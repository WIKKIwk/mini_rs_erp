use super::*;

pub async fn supplier_status(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryManage).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierStatusUpdateRequest = parse_json(&body)?;
    match state.admin.set_supplier_blocked(ref_, input.blocked).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier status failed")),
    }
}

pub async fn supplier_phone(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryManage).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminPhoneUpdateRequest = parse_json(&body)?;
    match state.admin.update_supplier_phone(ref_, &input.phone).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier phone update failed")),
    }
}

pub async fn supplier_items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierItemAssign).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierItemsUpdateRequest = parse_json(&body)?;
    match state
        .admin
        .update_supplier_items(ref_, input.item_codes)
        .await
    {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier items update failed")),
    }
}

pub async fn supplier_item_add(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierItemAssign).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierItemMutationRequest = parse_json(&body)?;
    state
        .admin
        .assign_supplier_item(ref_, &input.item_code)
        .await
        .map(Json)
        .map_err(|_| server_error("supplier item add failed"))
}

pub async fn supplier_item_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefItemQuery>,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierItemAssign).await?;
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    let (ref_, item_code) = required_ref_item(query.ref_.as_deref(), query.item_code.as_deref())?;
    state
        .admin
        .unassign_supplier_item(ref_, item_code)
        .await
        .map(Json)
        .map_err(|_| server_error("supplier item remove failed"))
}

pub async fn supplier_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierCodeManage).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.regenerate_supplier_code(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::CodeRegenCooldown) => {
            Err(too_many_requests("code regenerate cooldown"))
        }
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier code regenerate failed")),
    }
}

pub async fn supplier_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<OkResponse>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryManage).await?;
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.remove_supplier(ref_).await {
        Ok(()) => Ok(Json(OkResponse { ok: true })),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier remove failed")),
    }
}

pub async fn supplier_restore(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryManage).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.restore_supplier(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier restore failed")),
    }
}
