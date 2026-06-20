use super::*;

pub async fn werka_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<AdminSettings>, AdminError> {
    authorize_capability(&state, &headers, Capability::WerkaCodeManage).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    match state.admin.regenerate_werka_code().await {
        Ok(settings) => Ok(Json(settings)),
        Err(AdminPortError::CodeRegenCooldown) => {
            Err(too_many_requests("code regenerate cooldown"))
        }
        Err(_) => Err(server_error("werka code regenerate failed")),
    }
}
