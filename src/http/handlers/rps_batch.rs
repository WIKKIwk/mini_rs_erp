use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::gscale::{GscaleService, GscaleServiceError};
use crate::core::rps_batch::{
    RpsBatchClientPrintConfirmRequest, RpsBatchPrintRequest, RpsBatchServiceError,
    RpsBatchStartRequest,
};
use crate::http::handlers::auth::bearer_token;

pub async fn start(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let request: RpsBatchStartRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    require_material_warehouse_access(&state, &principal, &request.warehouse).await?;
    let response = state
        .rps_batch
        .start(&principal, request)
        .await
        .map_err(batch_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn state(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let response = state
        .rps_batch
        .state(&principal)
        .await
        .map_err(batch_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn history(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RpsBatchHistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let response = state
        .rps_batch
        .history(&principal, query.limit.unwrap_or(50))
        .await
        .map_err(batch_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn stop(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let response = state
        .rps_batch
        .stop(&principal)
        .await
        .map_err(batch_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn print(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let request: RpsBatchPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let (batch_id, mut material_request) = state
        .rps_batch
        .material_receipt_request(&principal, request)
        .await
        .map_err(batch_error)?;
    material_request.actor_role = principal_role_code(&principal.role).to_string();
    material_request.actor_ref = principal.ref_.trim().to_string();
    material_request.actor_display_name = principal.display_name.trim().to_string();
    let batch_service = state.rps_batch.clone();
    let batch_principal = principal.clone();
    let late_error_batch_id = batch_id.clone();
    let late_error = Arc::new(move |detail: String| {
        let batch_service = batch_service.clone();
        let batch_principal = batch_principal.clone();
        let batch_id = late_error_batch_id.clone();
        tokio::spawn(async move {
            match batch_service
                .record_late_error(&batch_principal, &batch_id, detail)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(batch_id, "RPS batch changed before late error was recorded");
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        batch_id,
                        "failed to record late RPS material receipt error"
                    );
                }
            }
        });
    });
    let print_count = GscaleService::material_receipt_print_count(&material_request)
        .map_err(gscale_error)?;
    material_request.print_count = 1;
    let mut last_response = None;
    for completed in 0..print_count {
        let response = match state
            .gscale
            .print_material_receipt_driver_once_with_late_error(
                material_request.clone(),
                Some(late_error.clone()),
            )
            .await
        {
            Ok(response) => response,
            Err(error) if completed > 0 => {
                let (status, Json(mut body)) = gscale_error(error);
                body.detail = format!(
                    "{completed}/{print_count} ta mahsulot chop etildi; keyingi print to'xtadi: {}",
                    body.detail
                );
                return Err((status, Json(body)));
            }
            Err(error) => return Err(gscale_error(error)),
        };
        record_batch_print(&state, &principal, &batch_id, &response).await;
        last_response = Some(response);
    }
    let mut response = last_response.ok_or_else(|| {
        bad_request("print_count_required", "print_count must be at least one")
    })?;
    response.print_count = print_count;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn client_print_prepare(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let request: RpsBatchPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let (_, mut material_request) = state
        .rps_batch
        .material_receipt_request(&principal, request)
        .await
        .map_err(batch_error)?;
    attach_actor(&mut material_request, &principal);
    let response = state
        .gscale
        .prepare_material_receipt_client_print(material_request)
        .map_err(gscale_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn client_print_confirm(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<RpsBatchErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let request: RpsBatchClientPrintConfirmRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let (batch_id, mut material_request) = state
        .rps_batch
        .material_receipt_request(&principal, request.print)
        .await
        .map_err(batch_error)?;
    attach_actor(&mut material_request, &principal);
    let response = state
        .gscale
        .confirm_material_receipt_client_print(material_request, &request.epc)
        .await
        .map_err(gscale_error)?;
    record_batch_print(&state, &principal, &batch_id, &response).await;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

async fn record_batch_print(
    state: &AppState,
    principal: &Principal,
    batch_id: &str,
    response: &crate::core::gscale::models::MaterialReceiptPrintResponse,
) {
    match state
        .rps_batch
        .record_print(principal, batch_id, response)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(batch_id, "RPS batch changed before print could be recorded");
        }
        Err(error) => {
            tracing::warn!(%error, batch_id, "failed to record RPS batch print");
        }
    }
}

fn attach_actor(
    request: &mut crate::core::gscale::models::MaterialReceiptPrintRequest,
    principal: &Principal,
) {
    request.actor_role = principal_role_code(&principal.role).to_string();
    request.actor_ref = principal.ref_.trim().to_string();
    request.actor_display_name = principal.display_name.trim().to_string();
}

async fn require_material_warehouse_access(
    state: &AppState,
    principal: &Principal,
    warehouse: &str,
) -> Result<(), (StatusCode, Json<RpsBatchErrorResponse>)> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Ok(());
    }
    let assigned = state
        .warehouses
        .assigned_warehouse_names(principal)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RpsBatchErrorResponse::new(
                    "warehouse_scope_failed",
                    "warehouse scope failed",
                )),
            )
        })?;
    if assigned
        .iter()
        .any(|assigned| assigned.trim().eq_ignore_ascii_case(warehouse.trim()))
    {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(RpsBatchErrorResponse::new(
            "warehouse_not_assigned",
            "warehouse is not assigned to material taminotchi",
        )),
    ))
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<RpsBatchErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    let principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    if !state
        .admin
        .principal_has_capability(&principal, Capability::RpsBatchManage)
        .await
    {
        return Err(forbidden());
    }
    Ok(principal)
}

fn batch_error(error: RpsBatchServiceError) -> (StatusCode, Json<RpsBatchErrorResponse>) {
    let status = match error {
        RpsBatchServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        RpsBatchServiceError::BatchNotActive | RpsBatchServiceError::BatchAlreadyActive => {
            StatusCode::CONFLICT
        }
        RpsBatchServiceError::StoreFailed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(RpsBatchErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn gscale_error(error: GscaleServiceError) -> (StatusCode, Json<RpsBatchErrorResponse>) {
    let status = match error {
        GscaleServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        GscaleServiceError::NotConfigured(_) => StatusCode::SERVICE_UNAVAILABLE,
        GscaleServiceError::EpcGenerationFailed => StatusCode::INTERNAL_SERVER_ERROR,
        GscaleServiceError::StoreWrite(_)
        | GscaleServiceError::PrintFailed { .. }
        | GscaleServiceError::SubmitFailed(_) => StatusCode::FAILED_DEPENDENCY,
    };
    (
        status,
        Json(RpsBatchErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn unauthorized() -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(RpsBatchErrorResponse::new("unauthorized", "unauthorized")),
    )
}

fn forbidden() -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(RpsBatchErrorResponse::new("forbidden", "forbidden")),
    )
}

fn bad_request(
    error: &'static str,
    detail: &'static str,
) -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(RpsBatchErrorResponse::new(error, detail)),
    )
}

fn method_not_allowed() -> (StatusCode, Json<RpsBatchErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(RpsBatchErrorResponse::new(
            "method_not_allowed",
            "method not allowed",
        )),
    )
}

fn principal_role_code(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

#[derive(Debug, Serialize)]
pub struct RpsBatchErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct RpsBatchHistoryQuery {
    pub limit: Option<usize>,
}

impl RpsBatchErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}

impl RpsBatchServiceError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::BatchNotActive => "batch_not_active",
            Self::BatchAlreadyActive => "batch_already_active",
            Self::StoreFailed => "batch_store_failed",
        }
    }
}
