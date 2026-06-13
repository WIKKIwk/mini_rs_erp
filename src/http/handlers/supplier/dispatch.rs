use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};

use super::authz::{authorize, require_supplier};
use crate::app::AppState;
use crate::core::auth::models::PrincipalRole;
use crate::core::werka::models::{CreateDispatchRequest, DispatchRecord};
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::push_notify::send_dispatch_record;

pub async fn create_dispatch(
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
    require_supplier(&state, &principal).await?;

    let request: CreateDispatchRequest = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid json",
            }),
        )
    })?;

    match state
        .werka
        .create_supplier_dispatch(
            &principal.ref_,
            &principal.display_name,
            &principal.phone,
            &request.item_code,
            request.qty,
        )
        .await
    {
        Ok(Some(record)) => {
            send_dispatch_record(
                &state,
                "werka:werka".to_string(),
                &record.supplier_name,
                &format!(
                    "{} • {:.0} {} qabul kutmoqda.",
                    record.item_code, record.sent_qty, record.uom
                ),
                &record,
                PrincipalRole::Werka,
                "werka",
                "werka dispatch notify",
            )
            .await;
            Ok(Json(record))
        }
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "dispatch create failed",
            }),
        )),
    }
}
