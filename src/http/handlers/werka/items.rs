use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::core::werka::models::{CustomerItemOption, SupplierItem};
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::werka::authz::{authorize, require_werka};
use crate::http::handlers::werka::query::{
    CustomerItemsQuery, DirectoryQuery, SupplierItemsQuery, optional_search_limit,
    optional_search_offset,
};

pub async fn supplier_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SupplierItemsQuery>,
) -> Result<Json<Vec<SupplierItem>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let supplier_ref = query.supplier_ref.as_deref().unwrap_or("").trim();
    let q = query.q.as_deref().unwrap_or("").trim();
    let limit = optional_search_limit(query.limit.as_deref(), 100, 200);
    let offset = optional_search_offset(query.offset.as_deref());
    match state
        .werka
        .supplier_items(supplier_ref, q, limit, offset)
        .await
    {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka supplier items failed",
            }),
        )),
    }
}

pub async fn customer_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CustomerItemsQuery>,
) -> Result<Json<Vec<SupplierItem>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let customer_ref = query.customer_ref.as_deref().unwrap_or("").trim();
    let q = query.q.as_deref().unwrap_or("").trim();
    let limit = optional_search_limit(query.limit.as_deref(), 100, 200);
    let offset = optional_search_offset(query.offset.as_deref());
    match state
        .werka
        .customer_items(customer_ref, q, limit, offset)
        .await
    {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka customer items failed",
            }),
        )),
    }
}

pub async fn customer_item_options(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<CustomerItemOption>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&state, &principal).await?;

    let q = query.q.as_deref().unwrap_or("").trim();
    let limit = optional_search_limit(query.limit.as_deref(), 200, 200);
    let offset = optional_search_offset(query.offset.as_deref());
    match state.werka.customer_item_options(q, limit, offset).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka customer item options failed",
            }),
        )),
    }
}
