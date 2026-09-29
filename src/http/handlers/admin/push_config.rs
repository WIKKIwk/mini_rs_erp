use super::*;
use crate::fcm::config::FcmConfigError;

async fn authorize_config(
    state: &AppState,
    headers: &HeaderMap,
    write: bool,
) -> Result<Principal, AdminError> {
    let capability = if write {
        Capability::AdminSettingsManage
    } else {
        Capability::AdminSettingsRead
    };
    let principal = authorize_capability(state, headers, capability).await?;
    if principal.role != PrincipalRole::Admin {
        return Err(forbidden());
    }
    Ok(principal)
}

pub async fn push_config(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_config(&state, &headers, method != Method::GET).await?;
    let status = match method {
        Method::GET => state.fcm_config.status(),
        Method::PUT => {
            if body.len() > 34 * 1024 {
                return Err(bad_request("push_config_invalid_credentials"));
            }
            #[derive(serde::Deserialize)]
            struct Input {
                service_account: serde_json::Value,
            }
            let input: Input = parse_json(&body)?;
            let status = state
                .fcm_config
                .save(input.service_account)
                .await
                .map_err(config_error)?;
            tracing::info!(admin_ref = %principal.ref_, project_id = %status.project_id, "admin updated FCM credentials");
            status
        }
        _ => return Err(method_not_allowed()),
    };
    let mut response = json_response(status);
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn push_config_check(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_config(&state, &headers, true).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    Ok(json_response(
        state.fcm_config.check().await.map_err(config_error)?,
    ))
}

pub async fn push_config_test(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_config(&state, &headers, true).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    #[derive(serde::Deserialize)]
    struct Input {
        token: String,
    }
    let input: Input = parse_json(&body)?;
    state
        .fcm_config
        .test_device(&principal, &input.token)
        .await
        .map_err(config_error)?;
    Ok(json_response(
        serde_json::json!({"ok":true,"accepted_by_fcm":true}),
    ))
}

fn config_error(error: FcmConfigError) -> AdminError {
    match error {
        FcmConfigError::InvalidCredentials
        | FcmConfigError::InvalidClientConfig
        | FcmConfigError::ProjectMismatch
        | FcmConfigError::NotConfigured
        | FcmConfigError::DeviceNotRegistered => bad_request(&error.to_string()),
        FcmConfigError::SaveFailed => server_error(&error.to_string()),
        _ => {
            let mut response = server_error(&error.to_string());
            response.0 = StatusCode::BAD_GATEWAY;
            response
        }
    }
}

pub async fn push_mobile_config(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_config(&state, &headers, true).await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    if body.len() > 8192 {
        return Err(bad_request("push_config_invalid_client_config"));
    }
    #[derive(serde::Deserialize)]
    struct Input {
        platform: String,
        config: crate::fcm::mobile_config::FirebaseClientConfig,
    }
    let input: Input = parse_json(&body)?;
    state
        .fcm_config
        .save_mobile_config(&input.platform, input.config)
        .await
        .map_err(config_error)?;
    Ok(json_response(state.fcm_config.status()))
}

// Only public SDK options are returned. Every authenticated employee needs these.
pub async fn push_client_config(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let mut response = json_response(state.fcm_config.mobile_configs());
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
