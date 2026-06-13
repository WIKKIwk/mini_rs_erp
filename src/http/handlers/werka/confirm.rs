use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};

use crate::app::AppState;
use crate::core::auth::models::PrincipalRole;
use crate::core::werka::models::{ConfirmReceiptRequest, DispatchRecord};
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::push_notify::send_dispatch_record;
use crate::http::handlers::werka::authz::{authorize, require_werka};

pub async fn confirm(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<DispatchRecord>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        ));
    }
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let request: ConfirmReceiptRequest = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid json",
            }),
        )
    })?;

    match state
        .werka
        .confirm_receipt(
            &request.receipt_id,
            request.accepted_qty,
            request.returned_qty,
            &request.return_reason,
            &request.return_comment,
        )
        .await
    {
        Ok(Some(record)) => {
            send_dispatch_record(
                &state,
                format!("supplier:{}", record.supplier_ref.trim()),
                &record.item_code,
                &format!("Status: {}", record.status.trim()),
                &record,
                PrincipalRole::Supplier,
                &record.supplier_ref,
                "supplier receipt notify",
            )
            .await;
            Ok(Json(record))
        }
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "receipt confirm failed",
            }),
        )),
    }
}
