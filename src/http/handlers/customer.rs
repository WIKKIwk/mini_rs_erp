use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Deserialize;

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::customer::models::{
    CustomerDeliveryDetail, CustomerDeliveryResponseRequest, CustomerHomeSummary,
};
use crate::core::customer::ports::CustomerServiceError;
use crate::core::werka::models::DispatchRecord;
use crate::http::handlers::auth::{ErrorResponse, bearer_token};
use crate::http::handlers::push_notify::send_dispatch_record;

pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CustomerHomeSummary>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_customer(&state, &principal).await?;

    match state.customer.summary(&principal).await {
        Ok(Some(summary)) => Ok(Json(summary)),
        Ok(None) | Err(_) => Err(server_error("customer summary failed")),
    }
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_customer(&state, &principal).await?;

    match state.customer.history(&principal).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err(server_error("customer history failed")),
    }
}

pub async fn status_details(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CustomerStatusDetailsQuery>,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_customer(&state, &principal).await?;

    let kind = query.kind.as_deref().unwrap_or("").trim();
    match state.customer.status_details(&principal, kind).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err(server_error("customer status details failed")),
    }
}

pub async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CustomerDetailQuery>,
) -> Result<Json<CustomerDeliveryDetail>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_customer(&state, &principal).await?;

    let delivery_note_id = query.delivery_note_id.as_deref().unwrap_or("").trim();
    if delivery_note_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "delivery_note_id is required",
            }),
        ));
    }

    match state.customer.detail(&principal, delivery_note_id).await {
        Ok(Some(detail)) => Ok(Json(detail)),
        Ok(None) => Err(server_error("customer detail failed")),
        Err(CustomerServiceError::Unauthorized) => Err(forbidden()),
        Err(_) => Err(server_error("customer detail failed")),
    }
}

pub async fn respond(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Result<Json<CustomerDeliveryDetail>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        ));
    }
    let principal = authorize(&state, &headers).await?;
    require_customer(&state, &principal).await?;

    let request: CustomerDeliveryResponseRequest = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid json",
            }),
        )
    })?;

    match state.customer.respond(&principal, request).await {
        Ok(Some(detail)) => {
            send_dispatch_record(
                &state,
                "werka:werka".to_string(),
                "Customer javob berdi",
                &detail.record.note,
                &detail.record,
                PrincipalRole::Werka,
                "werka",
                "werka customer response",
            )
            .await;
            send_dispatch_record(
                &state,
                "admin:admin".to_string(),
                "Customer javob berdi",
                &detail.record.note,
                &detail.record,
                PrincipalRole::Admin,
                "admin",
                "admin customer response",
            )
            .await;
            Ok(Json(detail))
        }
        Ok(None) => Err(server_error("customer respond failed")),
        Err(CustomerServiceError::Unauthorized) => Err(forbidden()),
        Err(CustomerServiceError::InvalidInput) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid input",
            }),
        )),
        Err(_) => Err(server_error("customer respond failed")),
    }
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn require_customer(
    state: &AppState,
    principal: &Principal,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if state
        .admin
        .principal_has_capability(principal, Capability::CustomerAccess)
        .await
    {
        Ok(())
    } else {
        Err(forbidden())
    }
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

fn forbidden() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse { error: "forbidden" }),
    )
}

fn server_error(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error }),
    )
}

#[derive(Debug, Deserialize)]
pub struct CustomerStatusDetailsQuery {
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CustomerDetailQuery {
    pub delivery_note_id: Option<String>,
}
