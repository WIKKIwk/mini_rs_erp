use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Serialize;

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::gscale::GscaleServiceError;
use crate::core::rps_batch::{RpsBatchPrintRequest, RpsBatchServiceError, RpsBatchStartRequest};
use crate::http::handlers::auth::bearer_token;

pub async fn start(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let request: RpsBatchStartRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let response = state
        .rps_batch
        .start(&principal, request)
        .await
        .map_err(batch_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn state(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let response = state
        .rps_batch
        .state(&principal)
        .await
        .map_err(batch_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn stop(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let response = state
        .rps_batch
        .stop(&principal)
        .await
        .map_err(batch_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn print(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let request: RpsBatchPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let material_request = state
        .rps_batch
        .material_receipt_request(&principal, request)
        .await
        .map_err(batch_error)?;
    let batch_service = state.rps_batch.clone();
    let batch_principal = principal.clone();
    let late_error = Arc::new(move |detail: String| {
        let batch_service = batch_service.clone();
        let batch_principal = batch_principal.clone();
        tokio::spawn(async move {
            if let Err(error) = batch_service
                .record_late_error(&batch_principal, detail)
                .await
            {
                tracing::warn!(%error, "failed to record late RPS ERP error");
            }
        });
    });
    let response = state
        .gscale
        .print_material_receipt_driver_first_with_late_error(material_request, Some(late_error))
        .await
        .map_err(gscale_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<RpsBatchErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    let principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    if !state
        .admin
        .principal_has_capability(&principal, Capability::RpsBatchManage)
        .await
    {
        return Err(forbidden());
    }
    Ok(principal)
}

fn batch_error(error: RpsBatchServiceError) -> (StatusCode, Json<RpsBatchErrorResponse>) {
    let status = match error {
        RpsBatchServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        RpsBatchServiceError::BatchNotActive => StatusCode::CONFLICT,
        RpsBatchServiceError::StoreFailed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(RpsBatchErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn gscale_error(error: GscaleServiceError) -> (StatusCode, Json<RpsBatchErrorResponse>) {
    let status = match error {
        GscaleServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        GscaleServiceError::NotConfigured(_) => StatusCode::SERVICE_UNAVAILABLE,
        GscaleServiceError::EpcGenerationFailed => StatusCode::INTERNAL_SERVER_ERROR,
        GscaleServiceError::ErpWrite(_)
        | GscaleServiceError::PrintFailed { .. }
        | GscaleServiceError::SubmitFailed(_) => StatusCode::FAILED_DEPENDENCY,
    };
    (
        status,
        Json(RpsBatchErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn unauthorized() -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(RpsBatchErrorResponse::new("unauthorized", "unauthorized")),
    )
}

fn forbidden() -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(RpsBatchErrorResponse::new("forbidden", "forbidden")),
    )
}

fn bad_request(
    error: &'static str,
    detail: &'static str,
) -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(RpsBatchErrorResponse::new(error, detail)),
    )
}

fn method_not_allowed() -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(RpsBatchErrorResponse::new(
            "method_not_allowed",
            "method not allowed",
        )),
    )
}

#[derive(Debug, Serialize)]
pub struct RpsBatchErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

impl RpsBatchErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}

impl RpsBatchServiceError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::BatchNotActive => "batch_not_active",
            Self::StoreFailed => "batch_store_failed",
        }
    }
}
