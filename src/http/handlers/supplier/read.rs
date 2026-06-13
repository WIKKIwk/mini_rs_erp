use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use super::authz::{authorize, require_supplier};
use crate::app::AppState;
use crate::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierItem, SupplierStatusBreakdownEntry,
};
use crate::http::handlers::auth::ErrorResponse;

pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SupplierHomeSummary>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_supplier(&state, &principal).await?;

    match state
        .werka
        .supplier_summary(&principal.ref_, &principal.display_name)
        .await
    {
        Ok(Some(summary)) => Ok(Json(summary)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier summary failed",
            }),
        )),
    }
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_supplier(&state, &principal).await?;

    match state
        .werka
        .supplier_history(&principal.ref_, &principal.display_name)
        .await
    {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier history failed",
            }),
        )),
    }
}

pub async fn status_breakdown(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SupplierStatusBreakdownQuery>,
) -> Result<Json<Vec<SupplierStatusBreakdownEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_supplier(&state, &principal).await?;

    let kind = query.kind.as_deref().unwrap_or("").trim();
    match state
        .werka
        .supplier_status_breakdown(&principal.ref_, &principal.display_name, kind)
        .await
    {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier status breakdown failed",
            }),
        )),
    }
}

pub async fn status_details(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SupplierStatusDetailsQuery>,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_supplier(&state, &principal).await?;

    let kind = query.kind.as_deref().unwrap_or("").trim();
    let item_code = query.item_code.as_deref().unwrap_or("").trim();
    match state
        .werka
        .supplier_status_details(&principal.ref_, &principal.display_name, kind, item_code)
        .await
    {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier status details failed",
            }),
        )),
    }
}

pub async fn items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SupplierItemsQuery>,
) -> Result<Json<Vec<SupplierItem>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_supplier(&state, &principal).await?;

    let q = query.q.as_deref().unwrap_or("").trim();
    match state
        .werka
        .supplier_mobile_items(&principal.ref_, q, 20)
        .await
    {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier items failed",
            }),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct SupplierStatusBreakdownQuery {
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SupplierStatusDetailsQuery {
    pub kind: Option<String>,
    pub item_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SupplierItemsQuery {
    pub q: Option<String>,
}
