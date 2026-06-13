use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Serialize;

use crate::app::AppState;
use crate::core::werka::models::{
    WerkaCustomerIssueBatchCreateRequest, WerkaCustomerIssueBatchResult,
    WerkaCustomerIssueCreateInput, WerkaCustomerIssueCreateRequest, WerkaCustomerIssueRecord,
    WerkaCustomerIssueSource,
};
use crate::core::werka::ports::WerkaPortError;
use crate::http::handlers::auth::ErrorResponse;
use crate::http::handlers::push_notify::send_customer_issue;
use crate::http::handlers::werka::authz::{authorize, require_werka};

#[derive(Serialize)]
pub struct IssueErrorResponse {
    pub error: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<&'static str>,
}

pub async fn customer_issue_create(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WerkaCustomerIssueRecord>, (StatusCode, Json<IssueErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(IssueErrorResponse {
                error: "method not allowed",
                error_code: None,
            }),
        ));
    }
    let principal = authorize(&state, &headers)
        .await
        .map_err(issue_auth_error)?;
    require_werka(&state, &principal)
        .await
        .map_err(issue_auth_error)?;

    let request: WerkaCustomerIssueCreateRequest = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(IssueErrorResponse {
                error: "invalid json",
                error_code: None,
            }),
        )
    })?;

    match state
        .werka
        .create_customer_issue(customer_issue_input_from_request(request))
        .await
    {
        Ok(Some(record)) => {
            send_customer_issue(&state, &record, "customer delivery note").await;
            Ok(Json(record))
        }
        Ok(None) | Err(WerkaPortError::WriteFailed(_)) | Err(WerkaPortError::LookupFailed) => {
            Err(customer_issue_create_failed())
        }
        Err(WerkaPortError::InsufficientStock) => Err((
            StatusCode::CONFLICT,
            Json(IssueErrorResponse {
                error: "insufficient stock",
                error_code: Some("insufficient_stock"),
            }),
        )),
        Err(WerkaPortError::DuplicateCustomerIssueSource) => Err((
            StatusCode::CONFLICT,
            Json(IssueErrorResponse {
                error: "duplicate customer issue source",
                error_code: Some("duplicate_customer_issue_source"),
            }),
        )),
        Err(_) => Err(customer_issue_create_failed()),
    }
}

pub async fn customer_issue_batch_create(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WerkaCustomerIssueBatchResult>, (StatusCode, Json<IssueErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(IssueErrorResponse {
                error: "method not allowed",
                error_code: None,
            }),
        ));
    }
    let principal = authorize(&state, &headers)
        .await
        .map_err(issue_auth_error)?;
    require_werka(&state, &principal)
        .await
        .map_err(issue_auth_error)?;

    let request: WerkaCustomerIssueBatchCreateRequest =
        serde_json::from_slice(&body).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(IssueErrorResponse {
                    error: "invalid json",
                    error_code: None,
                }),
            )
        })?;
    if request.lines.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(IssueErrorResponse {
                error: "lines are required",
                error_code: None,
            }),
        ));
    }

    let client_batch_id = request.client_batch_id;
    let lines = request
        .lines
        .into_iter()
        .map(customer_issue_batch_input_from_request)
        .collect();
    match state
        .werka
        .create_customer_issue_batch(&client_batch_id, lines)
        .await
    {
        Ok(Some(result)) => {
            for created in &result.created {
                if let Some(record) = &created.record {
                    send_customer_issue(&state, record, "customer delivery note batch line").await;
                }
            }
            Ok(Json(result))
        }
        Ok(None) | Err(_) => Err(customer_issue_create_failed()),
    }
}

fn customer_issue_input_from_request(
    request: WerkaCustomerIssueCreateRequest,
) -> WerkaCustomerIssueCreateInput {
    WerkaCustomerIssueCreateInput {
        customer_ref: request.customer_ref,
        item_code: request.item_code,
        qty: request.qty,
        source: WerkaCustomerIssueSource {
            barcode: request.source_barcode,
            stock_entry_name: request.source_stock_entry,
            line_index: request.source_line_index,
        },
    }
}

fn customer_issue_batch_input_from_request(
    request: WerkaCustomerIssueCreateRequest,
) -> WerkaCustomerIssueCreateInput {
    WerkaCustomerIssueCreateInput {
        customer_ref: request.customer_ref,
        item_code: request.item_code,
        qty: request.qty,
        source: WerkaCustomerIssueSource::default(),
    }
}

fn issue_auth_error(
    error: (StatusCode, Json<ErrorResponse>),
) -> (StatusCode, Json<IssueErrorResponse>) {
    (
        error.0,
        Json(IssueErrorResponse {
            error: error.1.error,
            error_code: None,
        }),
    )
}

fn customer_issue_create_failed() -> (StatusCode, Json<IssueErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(IssueErrorResponse {
            error: "werka customer issue create failed",
            error_code: None,
        }),
    )
}
