pub async fn scan(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipCellQrLookupQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let qr = query.qr.as_deref().unwrap_or("").trim();
    if qr.is_empty() {
        return Err(bad_request("qr_required"));
    }

    if let Some(cell_qr) = state
        .qolip
        .resolve_cell_qr(qr, &principal)
        .await
        .map_err(qolip_error)?
    {
        let _ = accessible_qolip_block(&state, &principal, &cell_qr.block).await?;
        return Ok(Json(serde_json::json!({
            "ok": true,
            "kind": "cell",
            "cell_qr": cell_qr,
        })));
    }

    let spec = state
        .qolip
        .product_spec_by_qolip_code(qr)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("qolip_code_not_found"))?;
    let location = state
        .qolip
        .location_by_qolip_code(qr)
        .await
        .map_err(qolip_error)?;
    if let Some(location) = &location {
        let _ = accessible_qolip_block(&state, &principal, &location.block).await?;
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "kind": "qolip",
        "product": {
            "code": spec.item_code,
            "name": spec.item_name,
            "item_group": spec.item_group,
            "qolip_code": spec.qolip_code,
            "size": spec.size,
            "has_qolip_spec": true,
        },
        "location": location,
    })))
}

pub async fn cell_qr(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipCellQrLookupQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let qr = query.qr.as_deref().unwrap_or("").trim();
    if qr.is_empty() {
        return Err(bad_request("qr_required"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    if !is_admin
        && state
            .qolip
            .assigned_blocks(&principal)
            .await
            .map_err(qolip_error)?
            .is_empty()
    {
        return Err(forbidden());
    }
    let cell_qr = state
        .qolip
        .resolve_cell_qr(qr, &principal)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("cell_qr_not_found"))?;
    let _ = accessible_qolip_block(&state, &principal, &cell_qr.block).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "cell_qr": cell_qr,
    })))
}

pub async fn cell_qr_print(
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
    let input: QolipCellQrPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let mut cell_input = QolipCellQrInput {
        block: input.block.clone(),
        warehouse: input.warehouse.clone(),
        row_letter: input.row_letter.clone(),
        column_number: input.column_number,
    };
    if let Some(block) = accessible_qolip_block(&state, &principal, &input.block).await? {
        cell_input.block = block.name;
        cell_input.warehouse = block.warehouse;
    }
    let cell_qr = state
        .qolip
        .cell_qr(cell_input, &principal)
        .await
        .map_err(qolip_error)?;
    let client_print = input.print_transport.trim().eq_ignore_ascii_case("offline");
    let print_request = ProgressLabelPrintRequest {
        driver_url: input.driver_url,
        qr_payload: cell_qr.qr_payload.clone(),
        item_code: cell_qr.qr_payload.clone(),
        item_name: cell_qr.location_label.clone(),
        executor_name: principal.display_name.trim().to_string(),
        printer: input.printer,
        print_mode: input.print_mode,
        label_kind: "qolip_cell".to_string(),
        gross_qty: 1.0,
        progress_qty: 1.0,
        unit: "dona".to_string(),
        progress_unit: "dona".to_string(),
        print_count: input.print_count,
    };
    let print = if client_print {
        state
            .gscale
            .prepare_progress_label(print_request)
            .map_err(gscale_print_error)?
    } else {
        state
            .gscale
            .print_progress_label(print_request)
            .await
            .map_err(gscale_print_error)?
    };
    Ok(Json(serde_json::json!({
        "ok": true,
        "cell_qr": cell_qr,
        "print": print,
    })))
}

pub async fn code_qr_print(
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
    let input: QolipCodeQrPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let qolip_code = input.qolip_code.trim();
    if qolip_code.is_empty() {
        return Err(bad_request("qolip_code_required"));
    }
    let spec = state
        .qolip
        .product_spec_by_qolip_code(qolip_code)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("qolip_code_not_found"))?;
    let client_print = input.print_transport.trim().eq_ignore_ascii_case("offline");
    let print_request = ProgressLabelPrintRequest {
        driver_url: input.driver_url,
        qr_payload: spec.qolip_code.clone(),
        item_code: spec.qolip_code.clone(),
        item_name: spec.item_name.clone(),
        executor_name: principal.display_name.trim().to_string(),
        printer: input.printer,
        print_mode: input.print_mode,
        label_kind: "qolip_code".to_string(),
        gross_qty: 1.0,
        progress_qty: 1.0,
        unit: "dona".to_string(),
        progress_unit: "dona".to_string(),
        print_count: input.print_count,
    };
    let print = if client_print {
        state
            .gscale
            .prepare_progress_label(print_request)
            .map_err(gscale_print_error)?
    } else {
        state
            .gscale
            .print_progress_label(print_request)
            .await
            .map_err(gscale_print_error)?
    };
    Ok(Json(serde_json::json!({
        "ok": true,
        "qolip_qr": {
            "qolip_code": spec.qolip_code,
            "qr_payload": spec.qolip_code,
            "item_code": spec.item_code,
            "item_name": spec.item_name,
            "item_group": spec.item_group,
            "size": spec.size,
        },
        "print": print,
    })))
}
