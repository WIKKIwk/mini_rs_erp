use super::*;

#[derive(serde::Deserialize)]
pub struct MileageQuery {
    #[serde(default)]
    qolip_code: String,
}

pub async fn mileage(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<MileageQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let spec = state
        .qolip
        .product_spec_by_qolip_code(&query.qolip_code)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| qolip_error(QolipError::QolipCodeNotFound))?;
    ensure_qolip_owner_access(&state, &principal, &spec).await?;
    let mileage = state
        .qolip
        .mileage(&spec.qolip_code)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({"ok": true, "mileage": mileage})))
}
