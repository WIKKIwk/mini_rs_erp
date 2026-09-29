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
                client_project_id: Option<String>,
            }
            let input: Input = parse_json(&body)?;
            if let Some(project) = input
                .client_project_id
                .filter(|value| !value.trim().is_empty())
            {
                if input.service_account["project_id"].as_str() != Some(project.trim()) {
                    return Err(bad_request("push_config_project_mismatch"));
                }
            }
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
