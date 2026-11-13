use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    app::AppState,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::Capability,
        gscale::{models::RawMaterialStockEntry, service::GscaleServiceError},
    },
};

type ApiError = (StatusCode, Json<serde_json::Value>);

fn error(status: StatusCode, code: &str, message: &str) -> ApiError {
    (status, Json(json!({"code": code, "error": message})))
}

async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let token = super::auth::bearer_token(headers).unwrap_or_default();
    let actor = state.sessions.get(&token).await.map_err(|_| {
        error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Avval akkauntga kiring",
        )
    })?;
    if actor.role != PrincipalRole::Metalayzer
        || !state
            .admin
            .principal_has_capability(&actor, Capability::MetalayzerAccess)
            .await
    {
        return Err(error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Metalayzer huquqi kerak",
        ));
    }
    Ok(actor)
}

async fn warehouses(state: &AppState, actor: &Principal) -> Result<Vec<String>, ApiError> {
    let names = state
        .warehouses
        .assigned_warehouse_names(actor)
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "metalayzer_scope_failed",
                "Ombor ruxsati olinmadi",
            )
        })?;
    if names.is_empty() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "metalayzer_warehouse_forbidden",
            "Sizga ombor biriktirilmagan",
        ));
    }
    Ok(names)
}

fn stock_error(err: GscaleServiceError) -> ApiError {
    match err {
        GscaleServiceError::InvalidInput(code) => {
            let (status, message) = match code.as_str() {
                "raw_material_stock_not_found" => (StatusCode::NOT_FOUND, "Xomashyo topilmadi"),
                "metalayzer_warehouse_forbidden" => {
                    (StatusCode::FORBIDDEN, "Bu omborga ruxsat yo‘q")
                }
                "metalayzer_material_unavailable" => {
                    (StatusCode::CONFLICT, "Bu xomashyo sarflangan")
                }
                _ => (StatusCode::BAD_REQUEST, "Xomashyo kodini kiriting"),
            };
            error(status, &code, message)
        }
        GscaleServiceError::NotConfigured(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "metalayzer_unavailable",
            "Xomashyo ombori hozir mavjud emas",
        ),
        other => {
            tracing::error!(%other, "metalayzer stock operation failed");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "metalayzer_store_failed",
                "Natija saqlanmadi. Qayta tekshiring",
            )
        }
    }
}

fn response(stock: RawMaterialStockEntry) -> Response {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "barcode": stock.barcode,
            "item_code": stock.item_code,
            "item_name": stock.item_name,
            "warehouse": stock.warehouse,
            "qty": stock.qty,
            "uom": stock.uom,
            "is_metalayzer": stock.is_metalayzer,
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialInput {
    pub barcode: String,
}

pub async fn material(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<MaterialInput>,
) -> Result<Response, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let allowed = warehouses(&state, &actor).await?;
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(&input.barcode)
        .await
        .map_err(stock_error)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                "raw_material_stock_not_found",
                "Xomashyo topilmadi",
            )
        })?;
    if !allowed
        .iter()
        .any(|name| name.trim().eq_ignore_ascii_case(stock.warehouse.trim()))
    {
        return Err(error(
            StatusCode::FORBIDDEN,
            "metalayzer_warehouse_forbidden",
            "Bu omborga ruxsat yo‘q",
        ));
    }
    if stock.status == "consumed" || stock.status == "deleted" || stock.qty <= 0.0 {
        return Err(error(
            StatusCode::CONFLICT,
            "metalayzer_material_unavailable",
            "Bu xomashyo sarflangan",
        ));
    }
    Ok(response(stock))
}

pub async fn complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MaterialInput>,
) -> Result<Response, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let allowed = warehouses(&state, &actor).await?;
    let stock = state
        .gscale
        .mark_raw_material_metalayzer(&input.barcode, &allowed)
        .await
        .map_err(stock_error)?;
    Ok(response(stock))
}
