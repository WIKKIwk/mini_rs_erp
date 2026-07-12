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
            let mut settings = state
                .admin
                .settings()
                .await
                .map_err(|_| server_error("settings fetch failed"))?;
            settings.werka_avatar_url = with_admin_profile_avatar_proxy(
                &headers,
                settings.werka_avatar_url,
                "werka",
                "werka",
            );
            Ok(Json(settings))
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
            let mut settings = state
                .admin
                .settings()
                .await
                .map_err(|_| server_error("suppliers fetch failed"))?;
            settings.werka_avatar_url = with_admin_profile_avatar_proxy(
                &headers,
                settings.werka_avatar_url,
                "werka",
                "werka",
            );
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

pub async fn user_list(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<AdminUserListPage>, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::SupplierDirectoryRead,
            Capability::CustomerDirectoryRead,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    require_capability(&state, &principal, Capability::SupplierDirectoryRead).await?;
    require_capability(&state, &principal, Capability::CustomerDirectoryRead).await?;
    let role = query
        .role
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    let mut page = match role.as_deref() {
        Some("qolipchi") => system_users::system_user_list_page(&state, &query).await?,
        Some("worker" | "ishchi" | "aparatchi" | "apparatchi") => {
            workers::worker_user_list_page(&state, &query).await?
        }
        _ => state
            .admin
            .user_list_page(
                query.q.as_deref().unwrap_or_default(),
                optional_search_limit(query.limit.as_deref(), 20, 50),
                optional_offset(query.offset.as_deref()),
                query.role.as_deref(),
            )
            .await
            .map_err(|_| server_error("admin users page failed"))?,
    };
    proxy_user_list_avatars(&headers, &mut page);
    Ok(Json(page))
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
        Ok(mut detail) => {
            detail.avatar_url =
                with_admin_profile_avatar_proxy(&headers, detail.avatar_url, "supplier", ref_);
            Ok(Json(detail))
        }
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier detail failed")),
    }
}

pub async fn admin_profile_avatar_view(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<AdminProfileAvatarQuery>,
) -> Response {
    if method != Method::GET {
        return method_not_allowed().into_response();
    }
    let token = query
        .token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| bearer_token(&headers));
    let Some(token) = token else {
        return unauthorized().into_response();
    };
    let Ok(principal) = state.sessions.get(&token).await else {
        return unauthorized().into_response();
    };
    if require_capability(&state, &principal, Capability::AdminAccess)
        .await
        .is_err()
    {
        return forbidden().into_response();
    }
    let Some(role) = query
        .role
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return bad_request("role is required").into_response();
    };
    let Some(ref_) = query
        .ref_
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return bad_request("ref is required").into_response();
    };
    let role_key = match role {
        "supplier"
        | "customer"
        | "worker"
        | "werka"
        | "admin"
        | "aparatchi"
        | "qolipchi"
        | "material_taminotchi" => role,
        _ => return bad_request("invalid role").into_response(),
    };
    match state
        .profiles
        .download_avatar_for_profile(role_key, ref_)
        .await
    {
        Ok(Some(file)) => {
            let mut response = axum::body::Body::from(file.body).into_response();
            if !file.content_type.trim().is_empty() {
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    file.content_type
                        .parse()
                        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
                );
            }
            response
        }
        Ok(None) => not_found("avatar not found").into_response(),
        Err(_) => server_error("avatar fetch failed").into_response(),
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
