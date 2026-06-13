use super::*;

pub async fn settings(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AdminSettings>, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminSettingsRead,
            Capability::AdminSettingsManage,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::AdminSettingsRead).await?;
            state
                .admin
                .settings()
                .await
                .map(Json)
                .map_err(|_| server_error("settings fetch failed"))
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::AdminSettingsManage).await?;
            let input: AdminSettings = parse_json(&body)?;
            state
                .admin
                .update_settings(input)
                .await
                .map(Json)
                .map_err(|_| server_error("settings update failed"))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn suppliers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::SupplierDirectoryRead,
            Capability::SupplierDirectoryManage,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::SupplierDirectoryRead).await?;
            let summary = state
                .admin
                .supplier_summary(300)
                .await
                .map_err(|_| server_error("supplier summary failed"))?;
            let suppliers = state
                .admin
                .suppliers(100)
                .await
                .map_err(|_| server_error("suppliers fetch failed"))?;
            let customers = state.admin.customers(500).await.unwrap_or_default();
            let settings = state
                .admin
                .settings()
                .await
                .map_err(|_| server_error("suppliers fetch failed"))?;
            Ok(json_response(AdminSuppliersPage {
                summary,
                suppliers,
                customers,
                settings,
            }))
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::SupplierDirectoryManage).await?;
            let input: AdminCreateSupplierRequest = parse_json(&body)?;
            state
                .admin
                .create_supplier(&input.name, &input.phone)
                .await
                .map(json_response)
                .map_err(|_| server_error("supplier create failed"))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn supplier_list(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<AdminSupplier>>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .admin
        .suppliers_page(
            optional_search_limit(query.limit.as_deref(), 20, 50),
            optional_offset(query.offset.as_deref()),
        )
        .await
        .map(Json)
        .map_err(|_| server_error("suppliers page failed"))
}

pub async fn supplier_summary(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<AdminSupplierSummary>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .admin
        .supplier_summary(300)
        .await
        .map(Json)
        .map_err(|_| server_error("supplier summary failed"))
}

pub async fn supplier_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.supplier_detail(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier detail failed")),
    }
}

pub async fn inactive_suppliers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<Vec<AdminSupplier>>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .admin
        .inactive_suppliers(300)
        .await
        .map(Json)
        .map_err(|_| server_error("inactive suppliers failed"))
}

pub async fn assigned_supplier_items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<Vec<SupplierItem>>, AdminError> {
    authorize_capability(&state, &headers, Capability::SupplierDirectoryRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let ref_ = required_ref(query.ref_.as_deref())?;
    state
        .admin
        .assigned_supplier_items(ref_, 200)
        .await
        .map(Json)
        .map_err(|_| server_error("assigned items fetch failed"))
}
