use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::rezka::{RezkaServiceError, RezkaSourceEntry, RezkaSplitRequest};
use crate::core::werka::models::StockEntryBarcodeEntry;
use crate::core::werka::ports::WerkaPortError;
use crate::http::handlers::auth::bearer_token;

pub async fn source(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RezkaSourceQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RezkaErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_rezka_access(&state, &principal).await?;
    let source = source_by_barcode(&state, query.barcode.as_deref().unwrap_or("")).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "source": source,
    })))
}

pub async fn split(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RezkaErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_rezka_access(&state, &principal).await?;
    let request: RezkaSplitRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let source = source_by_barcode(&state, &request.source_barcode).await?;
    let response = state
        .rezka
        .split(source, request)
        .await
        .map_err(rezka_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

#[derive(Debug, Deserialize)]
pub struct RezkaSourceQuery {
    barcode: Option<String>,
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<RezkaErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn ensure_rezka_access(
    state: &AppState,
    principal: &Principal,
) -> Result<(), (StatusCode, Json<RezkaErrorResponse>)> {
    if state
        .admin
        .principal_has_capability(principal, Capability::RezkaSplitManage)
        .await
    {
        Ok(())
    } else {
        Err(forbidden())
    }
}

async fn source_by_barcode(
    state: &AppState,
    barcode: &str,
) -> Result<RezkaSourceEntry, (StatusCode, Json<RezkaErrorResponse>)> {
    let barcode = barcode.trim();
    if barcode.is_empty() {
        return Err(bad_request("barcode_required", "barcode is required"));
    }
    let lookup = state
        .werka
        .stock_entry_lookup_by_barcode(barcode, 20)
        .await
        .map_err(lookup_error)?
        .ok_or_else(|| service_unavailable("lookup_unavailable", "lookup unavailable"))?;
    let Some(entry) = lookup
        .entries
        .into_iter()
        .find(|entry| entry.doc_status == 1)
    else {
        return Err(not_found("stock_entry_not_found", "stock entry not found"));
    };
    source_entry_from_stock_entry(entry).ok_or_else(|| {
        bad_request(
            "source_entry_invalid",
            "source entry warehouse/item/qty is invalid",
        )
    })
}

fn source_entry_from_stock_entry(entry: StockEntryBarcodeEntry) -> Option<RezkaSourceEntry> {
    let warehouse = if entry.target_warehouse.trim().is_empty() {
        entry.source_warehouse.trim().to_string()
    } else {
        entry.target_warehouse.trim().to_string()
    };
    if entry.barcode.trim().is_empty()
        || entry.item_code.trim().is_empty()
        || warehouse.is_empty()
        || entry.qty <= 0.0
    {
        return None;
    }
    Some(RezkaSourceEntry {
        barcode: entry.barcode.trim().to_string(),
        stock_entry_name: entry.stock_entry_name.trim().to_string(),
        line_index: entry.line_index,
        item_code: entry.item_code.trim().to_string(),
        item_name: entry.item_name.trim().to_string(),
        qty: entry.qty,
        uom: first_non_empty(&entry.uom, &entry.stock_uom, "Kg"),
        warehouse,
        company: entry.company.trim().to_string(),
    })
}

fn lookup_error(error: WerkaPortError) -> (StatusCode, Json<RezkaErrorResponse>) {
    match error {
        WerkaPortError::InvalidInput => bad_request("barcode_required", "barcode is required"),
        _ => service_unavailable("lookup_unavailable", "lookup unavailable"),
    }
}

fn rezka_error(error: RezkaServiceError) -> (StatusCode, Json<RezkaErrorResponse>) {
    let status = match error {
        RezkaServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        RezkaServiceError::NotConfigured(_) => StatusCode::SERVICE_UNAVAILABLE,
        RezkaServiceError::EpcGenerationFailed => StatusCode::INTERNAL_SERVER_ERROR,
        RezkaServiceError::StoreWrite(_)
        | RezkaServiceError::PrintFailed(_)
        | RezkaServiceError::SubmitFailed(_) => StatusCode::FAILED_DEPENDENCY,
    };
    (
        status,
        Json(RezkaErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn first_non_empty(value: &str, fallback: &str, default: &str) -> String {
    let value = value.trim();
    if !value.is_empty() {
        return value.to_string();
    }
    let fallback = fallback.trim();
    if !fallback.is_empty() {
        fallback.to_string()
    } else {
        default.to_string()
    }
}

fn unauthorized() -> (StatusCode, Json<RezkaErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(RezkaErrorResponse::new("unauthorized", "unauthorized")),
    )
}

fn forbidden() -> (StatusCode, Json<RezkaErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(RezkaErrorResponse::new("forbidden", "forbidden")),
    )
}

fn method_not_allowed() -> (StatusCode, Json<RezkaErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(RezkaErrorResponse::new(
            "method_not_allowed",
            "method not allowed",
        )),
    )
}

fn bad_request(
    error: &'static str,
    detail: &'static str,
) -> (StatusCode, Json<RezkaErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(RezkaErrorResponse::new(error, detail)),
    )
}

fn not_found(error: &'static str, detail: &'static str) -> (StatusCode, Json<RezkaErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(RezkaErrorResponse::new(error, detail)),
    )
}

fn service_unavailable(
    error: &'static str,
    detail: &'static str,
) -> (StatusCode, Json<RezkaErrorResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(RezkaErrorResponse::new(error, detail)),
    )
}

#[derive(Debug, Serialize)]
pub struct RezkaErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

impl RezkaErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}
