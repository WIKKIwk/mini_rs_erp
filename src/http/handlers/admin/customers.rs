use super::*;

mod material_scope;
use material_scope::{material_scoped_items, scoped_item_group_tree};

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

pub async fn material_taminotchilar(
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
                .user_list_page("", 500, 0, Some("material_taminotchi"))
                .await
                .map(json_response)
                .map_err(|_| server_error("material taminotchilar fetch failed"))
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::CustomerDirectoryManage).await?;
            let input: AdminCreateMaterialTaminotchiRequest = parse_json(&body)?;
            let detail = state
                .admin
                .create_material_taminotchi(&input.name, &input.phone, input.assigned_item_groups)
                .await
                .map_err(|error| match error {
                    AdminPortError::InvalidInput(message) => bad_request(message),
                    _ => server_error("material taminotchi create failed"),
                })?;
            let detail = hydrate_material_taminotchi_detail(&state, &headers, detail).await?;
            Ok(json_response(detail))
        }
        _ => Err(method_not_allowed()),
    }
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
    let mut detail = state
        .admin
        .customer_detail(ref_)
        .await
        .map_err(|_| server_error("customer detail failed"))?;
    detail.avatar_url =
        with_admin_profile_avatar_proxy(&headers, detail.avatar_url, "customer", ref_);
    Ok(Json(detail))
}

pub async fn material_taminotchi_detail(
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
    let detail = state
        .admin
        .material_taminotchi_detail(ref_)
        .await
        .map_err(|_| server_error("material taminotchi detail failed"))?;
    Ok(Json(
        hydrate_material_taminotchi_detail(&state, &headers, detail).await?,
    ))
}

pub async fn material_taminotchi_phone(
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
    let detail = match state
        .admin
        .update_material_taminotchi_phone(ref_, &input.phone)
        .await
    {
        Ok(detail) => detail,
        Err(AdminPortError::NotFound) => return Err(not_found("material taminotchi not found")),
        Err(_) => return Err(server_error("material taminotchi phone update failed")),
    };
    Ok(Json(
        hydrate_material_taminotchi_detail(&state, &headers, detail).await?,
    ))
}

pub async fn material_taminotchi_code_regenerate(
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
    let detail = match state.admin.regenerate_material_taminotchi_code(ref_).await {
        Ok(detail) => detail,
        Err(AdminPortError::CodeRegenCooldown) => {
            return Err(too_many_requests("code regenerate cooldown"));
        }
        Err(AdminPortError::NotFound) => return Err(not_found("material taminotchi not found")),
        Err(_) => return Err(server_error("material taminotchi code regenerate failed")),
    };
    Ok(Json(
        hydrate_material_taminotchi_detail(&state, &headers, detail).await?,
    ))
}

pub async fn material_taminotchi_item_groups(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminMaterialItemGroupsUpdateRequest = parse_json(&body)?;
    let detail = state
        .admin
        .update_material_taminotchi_item_groups(ref_, input.assigned_item_groups)
        .await
        .map_err(|error| match error {
            AdminPortError::InvalidInput(message) => bad_request(message),
            AdminPortError::NotFound => not_found("material taminotchi not found"),
            _ => server_error("material taminotchi item groups update failed"),
        })?;
    Ok(Json(
        hydrate_material_taminotchi_detail(&state, &headers, detail).await?,
    ))
}

async fn hydrate_material_taminotchi_detail(
    state: &AppState,
    headers: &HeaderMap,
    mut detail: AdminCustomerDetail,
) -> Result<AdminCustomerDetail, AdminError> {
    let principal = Principal {
        role: PrincipalRole::MaterialTaminotchi,
        display_name: detail.name.clone(),
        legal_name: detail.name.clone(),
        ref_: detail.ref_.clone(),
        phone: detail.phone.clone(),
        avatar_url: detail.avatar_url.clone(),
    };
    detail.assigned_warehouses = state
        .warehouses
        .assigned_warehouse_names(&principal)
        .await
        .map_err(|_| server_error("material taminotchi warehouses fetch failed"))?;
    detail
        .assigned_warehouses
        .sort_by_key(|value| value.to_lowercase());
    detail.avatar_url = with_admin_profile_avatar_proxy(
        headers,
        detail.avatar_url,
        "material_taminotchi",
        &detail.ref_,
    );
    Ok(detail)
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
        &[
            Capability::CatalogItemRead,
            Capability::CatalogItemCreate,
            Capability::GscaleCatalogRead,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            if principal.role == PrincipalRole::MaterialTaminotchi {
                require_capability(&state, &principal, Capability::GscaleCatalogRead).await?;
                material_scoped_items(&state, &principal, &query)
                    .await
                    .map(json_response)
            } else {
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
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::CatalogItemCreate).await?;
            let input: AdminCreateItemRequest = parse_json(&body)?;
            require_material_item_group_scope(&state, &principal, &input.item_group).await?;
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
                Err(AdminPortError::InvalidInput(error)) if error == "item code already exists" => {
                    Err(conflict(error))
                }
                Err(AdminPortError::InvalidInput(error)) => Err(bad_request(error)),
                Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
                Err(_) => Err(server_error("admin item create failed")),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

async fn require_material_item_group_scope(
    state: &AppState,
    principal: &Principal,
    item_group: &str,
) -> Result<(), AdminError> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Ok(());
    }
    let item_group = item_group.trim();
    let assigned_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await
        .map_err(|_| server_error("item group scope fetch failed"))?;
    if !item_group.is_empty()
        && assigned_groups
            .iter()
            .any(|group| group.trim().eq_ignore_ascii_case(item_group))
    {
        return Ok(());
    }
    Err(bad_request(
        "item group is not assigned to material taminotchi",
    ))
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
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::CatalogItemGroupRead,
            Capability::GscaleCatalogRead,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let groups = state
        .admin
        .item_group_tree()
        .await
        .map_err(|_| server_error("admin item group tree failed"))?;
    if principal.role == PrincipalRole::MaterialTaminotchi {
        return Ok(json_response(
            scoped_item_group_tree(&state, &principal, groups).await?,
        ));
    }
    Ok(json_response(groups))
}

include!("customers_mutations.rs");
