use super::*;
use crate::core::qolip::{QolipStorageAction, QolipStorageError, QolipStorageQuery};

fn storage_error(e: QolipStorageError) -> (StatusCode, Json<QolipErrorResponse>) {
    match e {
        QolipStorageError::Conflict(code) => conflict(code),
        QolipStorageError::Invalid(code) => bad_request(code),
        QolipStorageError::Forbidden => forbidden(),
        QolipStorageError::NotFound => (
            StatusCode::NOT_FOUND,
            Json(QolipErrorResponse::new("qolip_storage_not_found")),
        ),
        QolipStorageError::Store => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(QolipErrorResponse::new("qolip_storage_failed")),
        ),
    }
}
async fn read(
    state: AppState,
    method: Method,
    headers: HeaderMap,
    query: QolipStorageQuery,
    kind: &str,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    Ok(Json(
        state
            .qolip
            .storage_read(kind, &query, &principal, admin)
            .await
            .map_err(storage_error)?,
    ))
}
pub async fn paddons(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipStorageQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    read(state, method, headers, query, "paddons").await
}
pub async fn paddon_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipStorageQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    read(state, method, headers, query, "detail").await
}
pub async fn paddon_molds(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipStorageQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    read(state, method, headers, query, "molds").await
}
pub async fn archive(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipStorageQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    read(state, method, headers, query, "archive").await
}
async fn action(
    state: AppState,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
    archive: bool,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: QolipStorageAction =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    let mut response = state
        .qolip
        .storage_action(input, archive, &principal, admin)
        .await
        .map_err(storage_error)?;
    // Placement changes affect every product projecting the same physical mold.
    // Invalidate rather than publish a partial delta from a historical retry snapshot.
    response["catalog_delta"] = serde_json::json!({"reset":true});
    state.production_maps.notify_live();
    Ok(Json(response))
}
pub async fn paddon_action(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    action(state, method, headers, body, false).await
}
pub async fn archive_action(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    action(state, method, headers, body, true).await
}

/// A label is read-only; printing never locks or restores the package.
pub async fn paddon_qr_print(
    State(state): State<AppState>, method: Method, headers: HeaderMap, body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::POST { return Err(method_not_allowed()); }
    let input: QolipCodeQrPrintRequest = serde_json::from_slice(&body)
        .map_err(|_| bad_request("invalid_json"))?;
    let code = input.qolip_code.trim().to_uppercase();
    if code.is_empty() { return Err(bad_request("qolip_code_required")); }
    let qr_payload = format!("qolip-paddon:{code}");
    let admin = state.admin.principal_has_admin_workspace_access(&principal).await;
    let detail = state.qolip.storage_read("detail", &QolipStorageQuery {
        code: code.clone(), ..Default::default()
    }, &principal, admin).await.map_err(storage_error)?;
    let label = ProgressLabelPrintRequest {
        driver_url: input.driver_url,
        qr_payload: qr_payload.clone(), item_code: code.clone(), item_name: format!("Qolip paddoni {code}"),
        apparatus: String::new(), apparatus_display_name: String::new(), customer_name: String::new(),
        executor_name: principal.display_name.trim().to_string(), printer: input.printer,
        print_mode: input.print_mode, label_kind: "paddon_code".to_string(),
        gross_qty: 1.0, tare_enabled: false, tare_kg: 0.0, progress_qty: 1.0,
        unit: "dona".to_string(), progress_unit: "dona".to_string(), print_count: input.print_count,
    };
    let print = if input.print_transport.trim().eq_ignore_ascii_case("offline") {
        state.gscale.prepare_progress_label(label).map_err(gscale_print_error)?
    } else {
        state.gscale.print_progress_label(label).await.map_err(gscale_print_error)?
    };
    Ok(Json(serde_json::json!({"ok":true,"paddon":detail["paddon"],"qr_payload":qr_payload,"print":print})))
}
