
pub async fn blocks(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET
        && method != Method::POST
        && method != Method::PUT
        && method != Method::DELETE
    {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET => {
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let blocks = state
                .qolip
                .blocks_for_principal(&principal, is_admin)
                .await
                .map_err(qolip_error)?;
            let warehouses = state
                .qolip
                .warehouses_for_principal(&principal, is_admin)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "supports_cross_block_move": true,
                "warehouses": warehouses,
                "blocks": blocks,
            })))
        }
        Method::POST => {
            let input: QolipBlockUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let block = input.block.trim();
            if block.is_empty() {
                return Err(bad_request("block_required"));
            }
            let parent = accessible_qolip_warehouse(&state, &principal, &input.warehouse).await?;
            let saved = state
                .warehouses
                .upsert_warehouse(WarehouseUpsert {
                    warehouse: block.to_string(),
                    company: String::new(),
                    is_group: false,
                    parent_warehouse: parent.clone(),
                })
                .await
                .map_err(|_| qolip_error(QolipError::StoreFailed))?;
            let block = QolipBlock {
                name: saved.warehouse,
                warehouse: saved.parent_warehouse,
            };
            Ok(Json(serde_json::json!({
                "ok": true,
                "block": block,
            })))
        }
        Method::PUT => {
            let input: QolipBlockUpdate =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let new_block = input.new_block.trim();
            if new_block.is_empty() {
                return Err(bad_request("block_required"));
            }
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let current = managed_qolip_block(&state, &principal, &input.block, is_admin).await?;
            let warehouse =
                accessible_qolip_warehouse(&state, &principal, &input.warehouse).await?;
            if !current
                .warehouse
                .trim()
                .eq_ignore_ascii_case(warehouse.trim())
            {
                return Err(forbidden());
            }

            if !current.name.trim().eq_ignore_ascii_case(new_block) {
                let already_exists = state
                    .warehouses
                    .warehouses(new_block, "", 200)
                    .await
                    .map_err(|_| qolip_error(QolipError::StoreFailed))?
                    .into_iter()
                    .any(|item| item.warehouse.trim().eq_ignore_ascii_case(new_block));
                if already_exists {
                    return Err(conflict("block_exists"));
                }
            }

            let saved = state
                .qolip
                .rename_block(&current.name, new_block, &warehouse)
                .await
                .map_err(|_| qolip_error(QolipError::StoreFailed))?;

            Ok(Json(serde_json::json!({
                "ok": true,
                "block": saved,
            })))
        }
        Method::DELETE => {
            let input: QolipBlockUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let current = managed_qolip_block(&state, &principal, &input.block, is_admin).await?;
            ensure_qolip_block_is_empty(&state, &principal, &current, is_admin).await?;
            state
                .warehouses
                .delete_warehouse(WarehouseDeleteRequest {
                    warehouse: current.name,
                    delete_products: false,
                })
                .await
                .map_err(qolip_block_delete_error)?;
            Ok(Json(serde_json::json!({"ok": true})))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn managed_qolip_block(
    state: &AppState,
    principal: &crate::core::auth::models::Principal,
    block: &str,
    is_admin: bool,
) -> Result<QolipBlock, (StatusCode, Json<QolipErrorResponse>)> {
    let block = block.trim();
    if block.is_empty() {
        return Err(bad_request("block_required"));
    }
    state
        .qolip
        .blocks_for_principal(principal, is_admin)
        .await
        .map_err(qolip_error)?
        .into_iter()
        .find(|item| item.name.trim().eq_ignore_ascii_case(block))
        .ok_or_else(forbidden)
}

async fn ensure_qolip_block_is_empty(
    state: &AppState,
    principal: &crate::core::auth::models::Principal,
    block: &QolipBlock,
    is_admin: bool,
) -> Result<(), (StatusCode, Json<QolipErrorResponse>)> {
    let locations = state
        .qolip
        .locations(&block.name)
        .await
        .map_err(qolip_error)?;
    if !locations.is_empty() {
        return Err(conflict("block_in_use"));
    }
    let open_checkouts = state
        .qolip
        .checkouts(principal, is_admin, Some(&block.name), "open", 1)
        .await
        .map_err(qolip_error)?;
    if open_checkouts.is_empty() {
        Ok(())
    } else {
        Err(conflict("block_in_use"))
    }
}

pub async fn products(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipSearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let products = state
        .qolip
        .products(
            query.q.as_deref().unwrap_or(""),
            query.limit.unwrap_or(50),
            query.with_qolip.unwrap_or(false) || query.with_qolip_only.unwrap_or(false),
        )
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "products": products,
    })))
}

pub async fn product_specs(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::POST && method != Method::DELETE {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::POST => {
            let input: QolipProductSpecUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let spec = state
                .qolip
                .upsert_product_spec(input, &principal)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "product": {
                    "code": spec.item_code,
                    "name": spec.item_name,
                    "item_group": spec.item_group,
                    "qolip_code": spec.qolip_code,
                    "size": spec.size,
                    "color": spec.color,
                    "has_qolip_spec": true,
                    "is_in_use": false,
                },
            })))
        }
        Method::DELETE => {
            let input: QolipProductSpecDelete =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let deleted_count = state
                .qolip
                .delete_product_specs(input.qolip_codes)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "deleted_count": deleted_count,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn product_specs_batch(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let input: QolipProductSpecBatchUpsert =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let specs = state
        .qolip
        .upsert_product_specs(input.specs, &principal)
        .await
        .map_err(qolip_error)?;
    let products = specs
        .into_iter()
        .map(|spec| {
            serde_json::json!({
                "code": spec.item_code,
                "name": spec.item_name,
                "item_group": spec.item_group,
                "qolip_code": spec.qolip_code,
                "size": spec.size,
                "color": spec.color,
                "has_qolip_spec": true,
                "is_in_use": false,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::json!({
        "ok": true,
        "products": products,
    })))
}

pub async fn locations(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipSearchQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET => {
            let mut block_query = query.block.as_deref().unwrap_or("").trim().to_string();
            if block_query.is_empty() {
                let assigned = state
                    .qolip
                    .assigned_blocks(&principal)
                    .await
                    .map_err(qolip_error)?;
                if assigned.len() == 1 {
                    block_query = assigned[0].name.clone();
                } else if assigned.is_empty()
                    && !state
                        .admin
                        .principal_has_capability(&principal, Capability::AdminAccess)
                        .await
                {
                    return Err(forbidden());
                }
            }
            let block = match accessible_qolip_block(&state, &principal, &block_query).await? {
                Some(block) => block.name,
                None => block_query,
            };
            let locations = state.qolip.locations(&block).await.map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "locations": locations,
            })))
        }
        Method::POST => {
            let mut input: QolipLocationUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            if let Some(block) = accessible_qolip_block(&state, &principal, &input.block).await? {
                input.block = block.name;
                input.warehouse = block.warehouse;
            }
            let saved = state
                .qolip
                .upsert_location(input, &principal)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "location": saved,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn workers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipSearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let workers = state
        .workers
        .workers(query.q.as_deref().unwrap_or(""), query.limit.unwrap_or(100))
        .await
        .map_err(|_| qolip_error(QolipError::StoreFailed))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "workers": workers,
    })))
}
