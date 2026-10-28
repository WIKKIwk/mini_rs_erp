use super::*;

#[derive(Debug, Deserialize)]
pub struct AdminItemDetailQuery {
    pub code: Option<String>,
    #[serde(default)]
    pub include_image: bool,
}

pub async fn item_uoms(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::CatalogItemRead],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .admin
        .item_uoms()
        .await
        .map(json_response)
        .map_err(|_| server_error("admin item UOMs failed"))
}

pub async fn item_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<AdminItemDetailQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::CatalogItemRead],
    )
    .await?;
    if method != Method::GET {
        require_capability(&state, &principal, Capability::AdminAccess).await?;
    }
    match method {
        Method::GET => {
            let code = query.code.as_deref().unwrap_or_default().trim();
            if code.is_empty() {
                return Err(bad_request("item code is required"));
            }
            let detail = state
                .admin
                .item_detail(code)
                .await
                .map_err(|error| match error {
                    AdminPortError::NotFound => not_found("item not found"),
                    AdminPortError::InvalidInput(message) => bad_request(message),
                    _ => server_error("admin item detail failed"),
                })?;
            if principal.role == PrincipalRole::SalesManager && !detail.is_finished_goods {
                return Err(forbidden());
            }
            if query.include_image
                && detail.is_finished_goods
                && !detail.item_group.trim().eq_ignore_ascii_case("kombo")
            {
                let image_order_id = match state
                    .qolip
                    .order_image_order_ids(&[detail.code.clone()])
                    .await
                {
                    Ok(images) => item_image_order_id(&detail.code, images),
                    Err(error) => {
                        tracing::warn!(?error, "failed to resolve item image order");
                        String::new()
                    }
                };
                let mut payload = serde_json::to_value(detail)
                    .map_err(|_| server_error("admin item detail failed"))?;
                payload["order_image_order_id"] = image_order_id.into();
                Ok(json_response(payload))
            } else {
                Ok(json_response(detail))
            }
        }
        Method::PUT => {
            let input: AdminUpdateItemRequest = parse_json(&body)?;
            state
                .admin
                .update_item(&input.original_code, &input.code, &input.name)
                .await
                .map(json_response)
                .map_err(|error| match error {
                    AdminPortError::NotFound => not_found("item not found"),
                    AdminPortError::InvalidInput(message)
                        if message == "item code already exists" =>
                    {
                        conflict(message)
                    }
                    AdminPortError::InvalidInput(message) => bad_request(message),
                    _ => server_error("admin item update failed"),
                })
        }
        Method::DELETE => {
            let code = query.code.as_deref().unwrap_or_default().trim();
            if code.is_empty() {
                return Err(bad_request("item code is required"));
            }
            state
                .admin
                .delete_item(code)
                .await
                .map(|()| json_response(serde_json::json!({"ok": true})))
                .map_err(|error| match error {
                    AdminPortError::NotFound => not_found("item not found"),
                    AdminPortError::InvalidInput(message) => conflict(message),
                    _ => server_error("admin item delete failed"),
                })
        }
        _ => Err(method_not_allowed()),
    }
}

fn item_image_order_id(code: &str, images: Vec<(String, String)>) -> String {
    images
        .into_iter()
        .find(|(item, _)| item.trim().eq_ignore_ascii_case(code.trim()))
        .map(|(_, order_id)| order_id)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::item_image_order_id;

    #[test]
    fn item_image_reference_requires_matching_product_code() {
        let rows = vec![(" product-a ".into(), "order-a".into())];
        assert_eq!(item_image_order_id("PRODUCT-A", rows.clone()), "order-a");
        assert_eq!(item_image_order_id("PRODUCT-B", rows), "");
    }
}
