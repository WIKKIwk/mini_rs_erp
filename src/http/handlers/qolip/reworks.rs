use super::*;

#[derive(Default, serde::Deserialize)]
pub struct ReworkQuery {
    #[serde(default)]
    pub status: String,
}

pub async fn reworks(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ReworkQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let status = if query.status.is_empty() {
        "open"
    } else {
        query.status.as_str()
    };
    if !matches!(status, "open" | "returned" | "all") {
        return Err(bad_request("invalid_status"));
    }
    let admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    let entries = state
        .qolip
        .reworks(status, &principal, admin)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({"ok":true,"reworks":entries})))
}

pub async fn send_reworks(
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
    let input: crate::core::qolip::QolipReworkSend =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    let entries = state
        .qolip
        .send_reworks(input, &principal, admin)
        .await
        .map_err(qolip_error)?;
    state.production_maps.notify_live();
    Ok(Json(serde_json::json!({"ok":true,"reworks":entries})))
}

pub async fn return_reworks(
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
    let input: crate::core::qolip::QolipReworkReturn =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let admin = state
        .admin
        .principal_has_admin_workspace_access(&principal)
        .await;
    let entries = state
        .qolip
        .return_reworks(input, &principal, admin)
        .await
        .map_err(qolip_error)?;
    state.production_maps.notify_live();
    Ok(Json(serde_json::json!({"ok":true,"reworks":entries})))
}
