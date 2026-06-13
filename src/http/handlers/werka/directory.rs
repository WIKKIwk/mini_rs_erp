use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::core::werka::models::{CustomerDirectoryEntry, SupplierDirectoryEntry};
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::werka::authz::{authorize, require_werka};
use crate::http::handlers::werka::query::{
    DirectoryQuery, optional_search_limit, optional_search_offset,
};

pub async fn suppliers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<SupplierDirectoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let q = query.q.as_deref().unwrap_or("").trim();
    let limit = optional_search_limit(query.limit.as_deref(), 200, 200);
    let offset = optional_search_offset(query.offset.as_deref());
    match state.werka.suppliers(q, limit, offset).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka suppliers failed",
            }),
        )),
    }
}

pub async fn customers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<CustomerDirectoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let q = query.q.as_deref().unwrap_or("").trim();
    let limit = optional_search_limit(query.limit.as_deref(), 200, 200);
    let offset = optional_search_offset(query.offset.as_deref());
    match state.werka.customers(q, limit, offset).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka customers failed",
            }),
        )),
    }
}
