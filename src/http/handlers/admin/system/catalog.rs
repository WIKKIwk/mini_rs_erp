use super::*;

pub async fn items_bulk_move_group(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AdminItemGroupBulkMoveResult>, AdminError> {
    authorize_capability(&state, &headers, Capability::CatalogItemBulkMove).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: AdminBulkMoveItemsRequest = parse_json(&body)?;
    match state
        .admin
        .move_items_to_group(input.item_codes, &input.item_group)
        .await
    {
        Ok(result) => Ok(Json(result)),
        Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
        Err(_) => Err(server_error("admin item bulk move failed")),
    }
}
