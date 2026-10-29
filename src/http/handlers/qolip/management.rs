use super::*;

pub async fn rename_set(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let is_admin = state.admin
        .principal_has_admin_workspace_access(&principal).await;
    let input: crate::core::qolip::QolipSetRename =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let set = state.qolip.rename_set(&input, &principal, is_admin)
        .await.map_err(qolip_error)?;
    Ok(Json(serde_json::json!({"ok": true, "set": set})))
}

pub async fn create_set(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let is_admin = state.admin
        .principal_has_admin_workspace_access(&principal).await;
    let input: crate::core::qolip::QolipSetCreate =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let set = state.qolip.create_set(&input, &principal, is_admin)
        .await.map_err(qolip_error)?;
    Ok(Json(serde_json::json!({"ok": true, "set": set})))
}

pub async fn sets(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let is_admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    match method {
        Method::GET => {
            let sets = state
                .qolip
                .managed_sets(&principal, is_admin)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({"ok": true, "sets": sets})))
        }
        Method::POST => {
            let input: crate::core::qolip::QolipSetEdit =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let mut codes = state.qolip.resolve_shared_set(&input.set_id).await.map_err(qolip_error)?
                .map(|set| set.products.into_iter().map(|product| product.code).collect::<Vec<_>>())
                .unwrap_or_default();
            let set = state
                .qolip
                .edit_set(&input, &principal, is_admin)
                .await
                .map_err(qolip_error)?;
            codes.extend(set.snapshot.products.iter().map(|product| product.code.clone()));
            let delta = catalog_delta::after_write(&state, &principal, codes).await;
            Ok(Json(serde_json::json!({"ok": true, "set": set, "catalog_delta": delta})))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn product_molds(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let is_admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    let input: crate::core::qolip::QolipProductMoldEdits =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    state
        .qolip
        .edit_product_molds(&input, &principal, is_admin)
        .await
        .map_err(qolip_error)?;
    let codes = input.changes.iter().map(|change| change.item_code.clone()).collect();
    let delta = catalog_delta::after_write(&state, &principal, codes).await;
    Ok(Json(serde_json::json!({"ok": true, "catalog_delta": delta})))
}
