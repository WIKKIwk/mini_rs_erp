use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Deserialize;

use crate::app::AppState;
use crate::core::werka::models::StockEntryBarcodeLookup;
use crate::core::werka::ports::WerkaPortError;
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

pub async fn lookup(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<StockEntryLookupQuery>,
) -> Result<Json<StockEntryBarcodeLookup>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;

    let barcode = query.barcode.as_deref().unwrap_or("").trim().to_string();
    let barcode = if barcode.is_empty() {
        query.epc.as_deref().unwrap_or("").trim()
    } else {
        barcode.as_str()
    };
    if barcode.is_empty() {
        return Err(bad_request("barcode is required"));
    }
    let limit = parse_positive_int(query.limit.as_deref(), 20);

    match state
        .werka
        .stock_entry_lookup_by_barcode(barcode, limit)
        .await
    {
        Ok(Some(lookup)) => Ok(Json(lookup)),
        Ok(None) | Err(WerkaPortError::DirectDbLookupUnavailable) => {
            Err(service_unavailable("direct db lookup unavailable"))
        }
        Err(WerkaPortError::InvalidInput) => Err(bad_request("barcode is required")),
        Err(WerkaPortError::NotFound) => Err(not_found("stock entry not found")),
        Err(_) => Err(server_error("stock entry lookup failed")),
    }
}

#[derive(Debug, Deserialize)]
pub struct StockEntryLookupQuery {
    barcode: Option<String>,
    epc: Option<String>,
    limit: Option<String>,
}

fn parse_positive_int(raw: Option<&str>, fallback: usize) -> usize {
    raw.unwrap_or("")
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

fn bad_request(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::BAD_REQUEST, Json(ErrorResponse { error }))
}

fn not_found(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::NOT_FOUND, Json(ErrorResponse { error }))
}

fn service_unavailable(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse { error }),
    )
}

fn server_error(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error }),
    )
}

fn method_not_allowed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ErrorResponse {
            error: "method not allowed",
        }),
    )
}
