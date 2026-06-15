use super::*;

pub async fn customers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::CustomerDirectoryRead,
            Capability::CustomerDirectoryManage,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::CustomerDirectoryRead).await?;
            state
                .admin
                .customers(500)
                .await
                .map(json_response)
                .map_err(|_| server_error("customers fetch failed"))
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::CustomerDirectoryManage).await?;
            let input: AdminCreateCustomerRequest = parse_json(&body)?;
            state
                .admin
                .create_customer(&input.name, &input.phone)
                .await
                .map(json_response)
                .map_err(|error| match error {
                    AdminPortError::InvalidInput(message) => bad_request(message),
                    _ => server_error("customer create failed"),
                })
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn customer_list(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<CustomerDirectoryEntry>>, AdminError> {
    authorize_capability(&state, &headers, Capability::CustomerDirectoryRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .admin
        .customers_page(
            query.q.as_deref().unwrap_or_default(),
            optional_search_limit(query.limit.as_deref(), 20, 50),
            optional_offset(query.offset.as_deref()),
        )
        .await
        .map(Json)
        .map_err(|_| server_error("customers page failed"))
}

pub async fn customer_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::CustomerDirectoryRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    state
        .admin
        .customer_detail(ref_)
        .await
        .map(Json)
        .map_err(|_| server_error("customer detail failed"))
}

pub async fn items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::CatalogItemRead, Capability::CatalogItemCreate],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::CatalogItemRead).await?;
            state
                .admin
                .items_page_by_group(
                    query.group.as_deref().unwrap_or(""),
                    query.q.as_deref().unwrap_or(""),
                    positive_int(query.limit.as_deref(), 50),
                    optional_offset(query.offset.as_deref()),
                )
                .await
                .map(json_response)
                .map_err(|_| server_error("admin items failed"))
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::CatalogItemCreate).await?;
            let input: AdminCreateItemRequest = parse_json(&body)?;
            match state
                .admin
                .create_item(
                    &input.code,
                    &input.name,
                    &input.uom,
                    &input.item_group,
                    &input.customer_ref,
                )
                .await
            {
                Ok(item) => Ok(json_response(item)),
                Err(AdminPortError::InvalidInput(error)) => Err(bad_request(error)),
                Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
                Err(_) => Err(server_error("admin item create failed")),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn item_groups(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::CatalogItemGroupRead,
            Capability::CatalogItemGroupManage,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST | Method::PUT) {
        return Err(method_not_allowed());
    }
    if method == Method::POST {
        require_capability(&state, &principal, Capability::CatalogItemGroupManage).await?;
        let input: AdminCreateItemGroupRequest = parse_json(&body)?;
        return match state
            .admin
            .create_item_group(&input.name, &input.parent, input.is_group)
            .await
        {
            Ok(group) => Ok(json_response(group)),
            Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
            Err(_) => Err(server_error("admin item group create failed")),
        };
    }
    if method == Method::PUT {
        require_capability(&state, &principal, Capability::CatalogItemGroupManage).await?;
        let input: AdminMoveItemGroupRequest = parse_json(&body)?;
        return match state
            .admin
            .move_item_group_parent(&input.name, &input.parent)
            .await
        {
            Ok(group) => Ok(json_response(group)),
            Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
            Err(_) => Err(server_error("admin item group move failed")),
        };
    }
    require_capability(&state, &principal, Capability::CatalogItemGroupRead).await?;
    state
        .admin
        .item_groups(query.q.as_deref().unwrap_or(""), 100)
        .await
        .map(json_response)
        .map_err(|_| server_error("admin item groups failed"))
}

pub async fn item_group_tree(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::CatalogItemGroupRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .admin
        .item_group_tree()
        .await
        .map(json_response)
        .map_err(|_| server_error("admin item group tree failed"))
}

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
