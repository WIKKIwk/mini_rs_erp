
pub async fn checkouts(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipCheckoutsQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET => {
            if let Some(block) = query
                .block
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                let _ = accessible_qolip_block(&state, &principal, block).await?;
            }
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let checkouts = state
                .qolip
                .checkouts(
                    &principal,
                    is_admin,
                    query
                        .block
                        .as_deref()
                        .filter(|value| !value.trim().is_empty()),
                    query.status.as_deref().unwrap_or("open"),
                    query.limit.unwrap_or(50),
                )
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "checkouts": checkouts,
            })))
        }
        Method::POST => {
            let input: QolipCheckoutCreate =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let location = state
                .qolip
                .location_by_id(&input.location_id)
                .await
                .map_err(qolip_error)?
                .ok_or_else(|| bad_request("location_not_found"))?;
            let _ = accessible_qolip_block(&state, &principal, &location.block).await?;
            let worker_id = input.worker_id.trim();
            if worker_id.is_empty() {
                return Err(bad_request("worker_required"));
            }
            let workers = state
                .workers
                .workers_by_ids(&[worker_id.to_string()])
                .await
                .map_err(|_| qolip_error(QolipError::StoreFailed))?;
            let Some(worker) = workers.into_iter().next() else {
                return Err(bad_request("worker_not_found"));
            };
            let checkout = state
                .qolip
                .issue_checkout_from_location(
                    location,
                    input.quantity,
                    &worker.id,
                    &worker.name,
                    &principal,
                )
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "checkout": checkout,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn checkout_return(
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
    let input: QolipCheckoutReturn =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let checkout_id = input.checkout_id.trim();
    if checkout_id.is_empty() {
        return Err(bad_request("checkout_required"));
    }
    let checkout = state
        .qolip
        .checkout_by_id(checkout_id)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("checkout_not_found"))?;
    let _ = accessible_qolip_block(&state, &principal, &checkout.block).await?;
    let returned = state
        .qolip
        .return_checkout(input)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "checkout": returned,
    })))
}

pub async fn location_move(
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
    let mut input: QolipLocationMove =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let location = state
        .qolip
        .location_by_id(&input.location_id)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("location_not_found"))?;
    let _ = accessible_qolip_block(&state, &principal, &location.block).await?;
    let requested_block = input.block.trim();
    if requested_block.is_empty() || requested_block.eq_ignore_ascii_case(location.block.trim()) {
        input.block = location.block.clone();
        input.warehouse = location.warehouse.clone();
    } else {
        let target = match accessible_qolip_block(&state, &principal, requested_block).await? {
            Some(block) => block,
            None => state
                .qolip
                .blocks_for_principal(&principal, true)
                .await
                .map_err(qolip_error)?
                .into_iter()
                .find(|block| block.name.trim().eq_ignore_ascii_case(requested_block))
                .ok_or_else(|| bad_request("block_not_found"))?,
        };
        input.block = target.name;
        input.warehouse = target.warehouse;
    }
    let saved = state
        .qolip
        .move_location(input, &principal)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "location": saved,
    })))
}

pub async fn location_move_batch(
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
    let mut batch: QolipLocationMoveBatch =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    if batch.moves.is_empty() {
        return Err(bad_request("locations_required"));
    }
    if batch.moves.len() > 100 {
        return Err(bad_request("locations_limit_exceeded"));
    }

    for input in &mut batch.moves {
        let location = state
            .qolip
            .location_by_id(&input.location_id)
            .await
            .map_err(qolip_error)?
            .ok_or_else(|| bad_request("location_not_found"))?;
        let _ = accessible_qolip_block(&state, &principal, &location.block).await?;
        let requested_block = input.block.trim();
        if requested_block.is_empty() || requested_block.eq_ignore_ascii_case(location.block.trim())
        {
            input.block = location.block.clone();
            input.warehouse = location.warehouse.clone();
        } else {
            let target = match accessible_qolip_block(&state, &principal, requested_block).await? {
                Some(block) => block,
                None => state
                    .qolip
                    .blocks_for_principal(&principal, true)
                    .await
                    .map_err(qolip_error)?
                    .into_iter()
                    .find(|block| block.name.trim().eq_ignore_ascii_case(requested_block))
                    .ok_or_else(|| bad_request("block_not_found"))?,
            };
            input.block = target.name;
            input.warehouse = target.warehouse;
        }
    }

    let locations = state
        .qolip
        .move_locations(batch.moves, &principal)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "locations": locations,
    })))
}

include!("../qolip_print_scan.rs");
