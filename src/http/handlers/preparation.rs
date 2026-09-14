use crate::{
    app::AppState,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::Capability,
        preparation::*,
        warehouses::{WarehouseAssignmentUpsert, WarehouseError, WarehouseUpsert},
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

async fn authorize_admin(state: &AppState, headers: &HeaderMap) -> Result<Principal, ApiError> {
    let token = super::auth::bearer_token(headers).unwrap_or_default();
    let principal = state.sessions.get(&token).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )
    })?;
    if !state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
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

fn warehouse_api_error(error: WarehouseError) -> ApiError {
    match error {
        WarehouseError::MissingWarehouse
        | WarehouseError::MissingPrincipalRef
        | WarehouseError::InvalidApparatus
        | WarehouseError::NotFound
        | WarehouseError::AssignmentNotFound => (
            StatusCode::BAD_REQUEST,
            Json(json!({"code":"preparation_invalid","error":error.to_string()})),
        ),
        WarehouseError::NotEmpty(_)
        | WarehouseError::HasActiveReservations(_)
        | WarehouseError::HasChildren => (
            StatusCode::CONFLICT,
            Json(json!({"code":"preparation_conflict","error":error.to_string()})),
        ),
        WarehouseError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code":"preparation_store","error":error.to_string()})),
        ),
    }
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
    pub material_id: String,
}

#[derive(Debug, Deserialize)]
pub struct FormulaDeleteQuery {
    pub product_code: String,
    pub name: String,
    pub material_id: String,
}

#[derive(Debug, Deserialize)]
pub struct OrderMaterialsQuery {
    pub order_id: String,
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
        .list_formulas(&actor.ref_, &query.product_code, &query.material_id)
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
        .delete_formula(
            &actor.ref_,
            &query.product_code,
            &query.name,
            &query.material_id,
        )
        .await
        .map(Json)
        .map_err(error)
}

pub async fn order_materials(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OrderMaterialsQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?
        .order_materials(&query.order_id)
        .await
        .map_err(error)?;
    // Faqat o'ziga biriktirilgan homashyolar qaytadi (fail-closed).
    let assigned: std::collections::BTreeSet<String> = store(&state)?
        .list_responsibilities(&actor.ref_)
        .await
        .map_err(error)?
        .get("materials")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|m| m.get("material_id")?.as_str())
                .map(|s| s.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let filtered: Vec<Value> = result
        .get("materials")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|m| {
                    m.get("material_id")
                        .and_then(|s| s.as_str())
                        .is_some_and(|s| assigned.contains(&s.to_lowercase()))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    Ok(Json(
        serde_json::json!({"order_id": result["order_id"], "materials": filtered}),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ResponsibilityQuery {
    pub principal_ref: String,
}

pub async fn responsibilities_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ResponsibilityQuery>,
) -> Result<Json<Value>, ApiError> {
    let _admin = authorize_admin(&state, &headers).await?;
    store(&state)?
        .list_responsibilities(&query.principal_ref)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn responsibilities_assign(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MaterialResponsibilityAssign>,
) -> Result<Json<Value>, ApiError> {
    let _admin = authorize_admin(&state, &headers).await?;
    store(&state)?
        .assign_responsibility(input)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn responsibilities_unassign(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ResponsibilityDeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let _admin = authorize_admin(&state, &headers).await?;
    store(&state)?
        .unassign_responsibility(MaterialResponsibilityDelete {
            principal_ref: query.principal_ref,
            material_id: query.material_id,
        })
        .await
        .map(Json)
        .map_err(error)
}

#[derive(Debug, Deserialize)]
pub struct ResponsibilityDeleteQuery {
    pub principal_ref: String,
    pub material_id: String,
}

/// Bola ombor ochish: faqat o'ziga biriktirilgan ota ombor ostiga.
/// Yaratilgan bola avtomatik shu masterga biriktiriladi — snapshot,
/// kirim/sarf uni darhol ko'radi.
pub async fn create_warehouse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreparationWarehouseCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let preparation = store(&state)?;
    let parent = preparation
        .owned_warehouse_name(&actor.ref_, &input.parent_warehouse)
        .await
        .map_err(error)?;
    let name = input.warehouse_name().map_err(error)?;
    if preparation
        .warehouse_name_exists(&name)
        .await
        .map_err(error)?
    {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"code":"preparation_conflict","error":"Bunday ombor nomi mavjud"})),
        ));
    }
    let created = state
        .warehouses
        .upsert_warehouse(WarehouseUpsert {
            warehouse: name.clone(),
            company: String::new(),
            is_group: false,
            parent_warehouse: parent.clone(),
        })
        .await
        .map_err(warehouse_api_error)?;
    state
        .warehouses
        .assign_warehouse(WarehouseAssignmentUpsert {
            assignment_kind: "warehouse".to_string(),
            warehouse: created.warehouse.clone(),
            warehouse_name: None,
            apparatus_id: None,
            principal_role: PrincipalRole::TayyorlovMasteri,
            principal_ref: actor.ref_.clone(),
            display_name: actor.display_name.clone(),
        })
        .await
        .map_err(warehouse_api_error)?;
    state.warehouse_events.notify_updated(
        &created.warehouse,
        "warehouse_assignment",
    );
    Ok(Json(
        json!({"warehouse": created.warehouse, "parent_warehouse": parent}),
    ))
}
