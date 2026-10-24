use super::*;
use crate::core::auth::models::PrincipalRole;
use crate::core::qolip::QolipProductTransfer;

pub async fn product_transfer(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    if principal.role != PrincipalRole::Qolipchi {
        return Err(forbidden());
    }
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: QolipProductTransfer =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let codes = vec![input.from_item_code.clone(), input.to_item_code.clone()];
    let specs = state
        .qolip
        .transfer_product_specs(input, &principal)
        .await
        .map_err(qolip_error)?;
    let delta = catalog_delta::after_write(&state, &principal, codes).await;
    Ok(Json(serde_json::json!({"ok": true, "specs": specs, "catalog_delta": delta})))
}
