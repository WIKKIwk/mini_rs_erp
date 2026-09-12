use crate::{
    app::AppState,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::Capability,
        preparation::*,
    },
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};

type ApiError = (StatusCode, Json<Value>);

async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let token = super::auth::bearer_token(headers).unwrap_or_default();
    let principal = state.sessions.get(&token).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )
    })?;
    if principal.role != PrincipalRole::TayyorlovMasteri
        || !state
            .admin
            .principal_has_capability(&principal, Capability::PreparationAccess)
            .await
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"}))));
    }
    Ok(principal)
}

fn store(
    state: &AppState,
) -> Result<&crate::db::postgres_preparation::PostgresPreparationStore, ApiError> {
    state.preparation.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"Tayyorlov ombori hozir mavjud emas"})),
    ))
}

fn error(error: PreparationError) -> ApiError {
    let code = match &error {
        PreparationError::Invalid(_) => "preparation_invalid",
        PreparationError::Conflict(_) => "preparation_conflict",
        PreparationError::Forbidden => "preparation_scope",
        PreparationError::Insufficient => "preparation_insufficient_stock",
        PreparationError::StoreFailed => "preparation_store",
    };
    let status = match &error {
        PreparationError::Invalid(_) => StatusCode::BAD_REQUEST,
        PreparationError::Conflict(_) | PreparationError::Insufficient => StatusCode::CONFLICT,
        PreparationError::Forbidden => StatusCode::FORBIDDEN,
        PreparationError::StoreFailed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"code":code,"error":error.to_string()})))
}

pub async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .snapshot(&actor.ref_)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn material(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MaterialCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .create_material(&actor, input)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn receipt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReceiptCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?.receive(&actor, input).await.map_err(error)?;
    state.warehouse_events.notify_updated(
        result["warehouse"].as_str().unwrap_or_default(),
        "raw_material_stock",
    );
    Ok(Json(result))
}

pub async fn consumption(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ConsumptionCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?.consume(&actor, input).await.map_err(error)?;
    state.warehouse_events.notify_updated(
        result["warehouse"].as_str().unwrap_or_default(),
        "raw_material_stock",
    );
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct FormulaQuery {
    pub product_code: String,
}

#[derive(Debug, Deserialize)]
pub struct FormulaDeleteQuery {
    pub product_code: String,
    pub name: String,
}

pub async fn formula_upsert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<FormulaUpsert>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .upsert_formula(&actor, input)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn formula_show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .list_formulas(&actor.ref_, &query.product_code)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn formula_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaDeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .delete_formula(&actor.ref_, &query.product_code, &query.name)
        .await
        .map(Json)
        .map_err(error)
}
