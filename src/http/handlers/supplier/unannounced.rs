use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};

use super::authz::{authorize, require_supplier};
use crate::app::AppState;
use crate::core::auth::models::PrincipalRole;
use crate::core::werka::models::{NotificationDetail, SupplierUnannouncedResponseRequest};
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::push_notify::send_dispatch_record;

pub async fn unannounced_respond(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
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
    require_supplier(&state, &principal).await?;

    let request: SupplierUnannouncedResponseRequest =
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
        .respond_supplier_unannounced(
            &principal.ref_,
            &principal.display_name,
            &request.receipt_id,
            request.approve,
            &request.reason,
        )
        .await
    {
        Ok(Some(detail)) => {
            send_dispatch_record(
                &state,
                "werka:werka".to_string(),
                "Supplier javob berdi",
                &detail.record.note,
                &detail.record,
                PrincipalRole::Werka,
                "werka",
                "werka unannounced response",
            )
            .await;
            Ok(Json(detail))
        }
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier unannounced response failed",
            }),
        )),
    }
}
