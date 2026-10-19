use super::*;
use crate::core::mobile_release::MobileUpdateSettings;

pub async fn app_update_settings(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(&state, &headers, &[Capability::AdminAccess]).await?;
    if principal.role != PrincipalRole::Admin {
        return Err(forbidden());
    }
    let settings = match method {
        Method::GET => state.mobile_releases.update_settings().await,
        Method::PUT => {
            let input: MobileUpdateSettings = parse_json(&body)?;
            let saved = state.mobile_releases.save_update_settings(input).await;
            if saved.is_ok() {
                tracing::info!(
                    admin_ref = %principal.ref_,
                    mandatory_updates_enabled = input.mandatory_updates_enabled,
                    "admin updated mobile update settings"
                );
            }
            saved
        }
        _ => return Err(method_not_allowed()),
    }
    .map_err(|error| {
        tracing::error!(%error, "mobile update settings failed");
        server_error("app_update_settings_failed")
    })?;
    let mut response = json_response(serde_json::json!({"ok": true, "settings": settings}));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
