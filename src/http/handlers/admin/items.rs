use super::*;

#[derive(Debug, Deserialize)]
pub struct AdminItemDetailQuery {
    pub code: Option<String>,
}

pub async fn item_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<AdminItemDetailQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    match method {
        Method::GET => {
            let code = query.code.as_deref().unwrap_or_default().trim();
            if code.is_empty() {
                return Err(bad_request("item code is required"));
            }
            state
                .admin
                .item_detail(code)
                .await
                .map(json_response)
                .map_err(|error| match error {
                    AdminPortError::NotFound => not_found("item not found"),
                    AdminPortError::InvalidInput(message) => bad_request(message),
                    _ => server_error("admin item detail failed"),
                })
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
