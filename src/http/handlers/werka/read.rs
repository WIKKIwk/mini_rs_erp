use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::core::werka::models::{
    DispatchRecord, WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
};
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::werka::authz::{authorize, require_werka};
use crate::http::handlers::werka::query::{StatusBreakdownQuery, StatusDetailsQuery};

pub async fn status_breakdown(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StatusBreakdownQuery>,
) -> Result<Json<Vec<WerkaStatusBreakdownEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let kind = query.kind.as_deref().unwrap_or("").trim();
    match state.werka.status_breakdown(kind).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka status breakdown failed",
            }),
        )),
    }
}

pub async fn status_details(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StatusDetailsQuery>,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let kind = query.kind.as_deref().unwrap_or("").trim();
    let supplier_ref = query.supplier_ref.as_deref().unwrap_or("").trim();
    match state.werka.status_details(kind, supplier_ref).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka status details failed",
            }),
        )),
    }
}

pub async fn pending(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    match state.werka.pending(0).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "pending fetch failed",
            }),
        )),
    }
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    match state.werka.history().await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "history fetch failed",
            }),
        )),
    }
}

pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WerkaHomeSummary>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    match state.werka.summary().await {
        Ok(Some(summary)) => Ok(Json(summary)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka summary failed",
            }),
        )),
    }
}

pub async fn home(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WerkaHomeData>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    match state.werka.home(20).await {
        Ok(Some(data)) => Ok(Json(data)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka home failed",
            }),
        )),
    }
}
