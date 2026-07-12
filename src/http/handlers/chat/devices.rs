use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Deserialize;

use super::auth::authorize;
use super::{ChatHttpError, http_error};
use crate::app::AppState;
use crate::core::push::models::PushTokenRegisterRequest;
use crate::core::push::ports::PushServiceError;

#[derive(Default, Deserialize)]
pub struct DeviceTokenQuery {
    token: Option<String>,
}

pub async fn device_token(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<DeviceTokenQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ChatHttpError> {
    let (_, principal) = authorize(&state, &headers).await?;
    match method {
        Method::POST => {
            let request: PushTokenRegisterRequest = serde_json::from_slice(&body)
                .map_err(|_| http_error(StatusCode::BAD_REQUEST, "chat_device_request_invalid"))?;
            state
                .push
                .register(&principal, &request.token, &request.platform)
                .await
                .map_err(device_error)?;
        }
        Method::DELETE => {
            let token = query.token.as_deref().unwrap_or_default();
            state
                .push
                .delete(&principal, token)
                .await
                .map_err(device_error)?;
        }
        _ => {
            return Err(http_error(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
            ));
        }
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

fn device_error(error: PushServiceError) -> ChatHttpError {
    match error {
        PushServiceError::TokenRequired => {
            http_error(StatusCode::BAD_REQUEST, "chat_device_token_required")
        }
        PushServiceError::StoreFailed => http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "chat_device_store_failed",
        ),
    }
}
