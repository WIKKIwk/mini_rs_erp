use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Deserialize;

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::models::{NotificationCommentCreateRequest, NotificationDetail};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};
use crate::http::handlers::push_notify::send_dispatch_record;

#[derive(Debug, Deserialize)]
pub struct NotificationDetailQuery {
    receipt_id: Option<String>,
}

pub async fn detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<NotificationDetailQuery>,
) -> Result<Json<NotificationDetail>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        ));
    }
    let principal = authorize(&state, &headers).await?;
    require_notification_role(&principal)?;
    let receipt_id = query.receipt_id.unwrap_or_default();
    if receipt_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "receipt_id is required",
            }),
        ));
    }
    match state
        .werka
        .notification_detail(
            principal.role,
            &principal.ref_,
            &principal.display_name,
            &receipt_id,
        )
        .await
    {
        Ok(Some(detail)) => Ok(Json(detail)),
        Ok(None) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "notification detail failed",
            }),
        )),
        Err(error) if error.to_string().contains("unauthorized") => Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "forbidden" }),
        )),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "notification detail failed",
            }),
        )),
    }
}

pub async fn comment(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<NotificationDetailQuery>,
    body: Bytes,
) -> Result<Json<NotificationDetail>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        ));
    }
    let principal = authorize(&state, &headers).await?;
    require_notification_role(&principal)?;
    let receipt_id = query.receipt_id.unwrap_or_default();
    if receipt_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "receipt_id is required",
            }),
        ));
    }
    let request: NotificationCommentCreateRequest =
        serde_json::from_slice(&body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid json",
                }),
            )
        })?;

    match state
        .werka
        .add_notification_comment(
            principal.role.clone(),
            &principal.ref_,
            &principal.display_name,
            &receipt_id,
            &request.message,
        )
        .await
    {
        Ok(Some(detail)) => {
            if principal.role == PrincipalRole::Supplier
                && request
                    .message
                    .trim()
                    .to_lowercase()
                    .starts_with("tasdiqlayman")
            {
                let mut record = detail.record.clone();
                record.id = format!(
                    "supplier_ack:{}:{}",
                    record.id.trim(),
                    time::OffsetDateTime::now_utc().unix_timestamp()
                );
                record.event_type = "supplier_ack".to_string();
                record.highlight = "Supplier mahsulotni qaytarganingizni tasdiqladi".to_string();
                record.note.clear();
                send_dispatch_record(
                    &state,
                    "werka:werka".to_string(),
                    "Supplier tasdiqladi",
                    &record.highlight,
                    &record,
                    PrincipalRole::Werka,
                    "werka",
                    "werka acknowledgment event",
                )
                .await;
            }
            Ok(Json(detail))
        }
        Ok(None) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "notification comment failed",
            }),
        )),
        Err(error) if error.to_string().contains("unauthorized") => Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "forbidden" }),
        )),
        Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "notification comment failed",
            }),
        )),
    }
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

fn require_notification_role(
    principal: &Principal,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if matches!(
        principal.role,
        PrincipalRole::Supplier | PrincipalRole::Werka | PrincipalRole::Customer
    ) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "forbidden" }),
        ))
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
