use super::*;

pub async fn material_stock(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MaterialStockQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate().map_err(error)?;
    let scopes = warehouse_material_scopes(&state, &actor).await?;
    store(&state)?
        .material_stock(&actor.ref_, &scopes, &query)
        .await
        .map(Json)
        .map_err(error)
}
