use super::*;

#[derive(serde::Deserialize)]
struct Request {
    order_ids: Vec<String>,
}

pub async fn order_products(
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
    let request: Request =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    if request.order_ids.len() > 50 || request.order_ids.iter().any(|id| id.trim().is_empty()) {
        return Err(bad_request("invalid_order_ids"));
    }
    let (snapshot, _) = state
        .production_maps
        .live_snapshot_shared_with_revision()
        .await
        .map_err(|_| qolip_error(QolipError::StoreFailed))?;
    let maps = request
        .order_ids
        .iter()
        .map(|id| {
            let map = snapshot
                .maps
                .iter()
                .find(|saved| saved.map.id.trim() == id.trim());
            (
                id.trim(),
                map.map(|saved| saved.map.product_code.trim().to_string()),
            )
        })
        .collect::<Vec<_>>();
    let codes = maps
        .iter()
        .filter_map(|(_, code)| code.clone())
        .collect::<Vec<_>>();
    let products = state
        .qolip
        .order_products(&codes)
        .await
        .map_err(qolip_error)?;
    let orders = maps
        .into_iter()
        .map(|(id, code)| {
            let product = code.as_ref().and_then(|code| {
                products
                    .iter()
                    .find(|product| product.code.trim().eq_ignore_ascii_case(code))
            });
            serde_json::json!({"order_id": id, "product": product})
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::json!({"ok": true, "orders": orders})))
}
