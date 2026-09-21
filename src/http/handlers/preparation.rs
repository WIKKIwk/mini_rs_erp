use crate::{
    app::AppState,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::Capability,
        preparation::*,
        warehouses::WarehouseError,
    },
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
        PreparationError::WarehouseNotExclusive => "preparation_warehouse_not_exclusive",
        PreparationError::WarehouseNotOwned => "preparation_warehouse_not_owned",
        PreparationError::WarehouseNameTaken => "preparation_warehouse_name_taken",
        PreparationError::WarehouseNotEmpty => "preparation_warehouse_not_empty",
        PreparationError::WarehouseHasMaterials => "preparation_warehouse_has_materials",
        PreparationError::WarehouseHasChildren => "preparation_warehouse_has_children",
        PreparationError::WarehouseInUse => "preparation_warehouse_in_use",
        PreparationError::MaterialNotInWarehouse => "preparation_material_not_in_warehouse",
        PreparationError::MaterialNotOwned => "preparation_material_not_owned",
        PreparationError::MaterialNameTaken => "preparation_material_name_taken",
        PreparationError::MaterialInUse => "preparation_material_in_use",
        PreparationError::MaterialWarehouseInUse => "preparation_material_warehouse_in_use",
        PreparationError::ReceiptRequiresQr => "preparation_receipt_requires_qr",
        PreparationError::Insufficient => "preparation_insufficient_stock",
        PreparationError::StoreFailed => "preparation_store",
    };
    let status = match &error {
        PreparationError::Invalid(_) => StatusCode::BAD_REQUEST,
        PreparationError::Conflict(_)
        | PreparationError::Insufficient
        | PreparationError::WarehouseNameTaken
        | PreparationError::WarehouseNotEmpty
        | PreparationError::WarehouseHasMaterials
        | PreparationError::WarehouseHasChildren
        | PreparationError::MaterialNameTaken
        | PreparationError::MaterialInUse
        | PreparationError::MaterialWarehouseInUse
        | PreparationError::WarehouseInUse => StatusCode::CONFLICT,
        PreparationError::Forbidden
        | PreparationError::WarehouseNotOwned
        | PreparationError::WarehouseNotExclusive
        | PreparationError::MaterialNotInWarehouse
        | PreparationError::MaterialNotOwned
        | PreparationError::ReceiptRequiresQr => {
            StatusCode::FORBIDDEN
        }
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

fn material_scope_admin_error(error: crate::core::admin::ports::AdminPortError) -> ApiError {
    tracing::error!(%error, "preparation warehouse material scope lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "code": "preparation_store",
            "error": "Tayyorlov ombori ko‘rinishini aniqlab bo‘lmadi"
        })),
    )
}

async fn warehouse_material_scopes(
    state: &AppState,
    actor: &Principal,
) -> Result<PreparationWarehouseMaterialScopes, ApiError> {
    let role_assignments = state
        .admin
        .role_assignments()
        .await
        .map_err(material_scope_admin_error)?;
    let assigned_warehouses = state
        .warehouses
        .assigned_warehouse_names(actor)
        .await
        .map_err(warehouse_api_error)?;
    let mut scopes = BTreeMap::new();

    for warehouse in assigned_warehouses {
        let assignments = state
            .warehouses
            .warehouse_assignments(&warehouse)
            .await
            .map_err(warehouse_api_error)?;
        let is_current_owner = assignments.iter().any(|assignment| {
            assignment.principal_role == actor.role
                && assignment
                    .principal_ref
                    .trim()
                    .eq_ignore_ascii_case(actor.ref_.trim())
        });
        if !is_current_owner {
            continue;
        }

        let other_assignments = assignments.into_iter().filter(|assignment| {
            assignment.principal_role != actor.role
                || !assignment
                    .principal_ref
                    .trim()
                    .eq_ignore_ascii_case(actor.ref_.trim())
        });
        let mut other_item_groups = Vec::new();
        let mut shared = false;
        for assignment in other_assignments {
            shared = true;
            if let Some(role_assignment) = role_assignments.iter().find(|candidate| {
                candidate.principal_role == assignment.principal_role
                    && candidate
                        .principal_ref
                        .trim()
                        .eq_ignore_ascii_case(assignment.principal_ref.trim())
            }) {
                other_item_groups.extend(role_assignment.assigned_item_groups.iter().cloned());
            }
        }

        let scope = if !shared {
            PreparationWarehouseMaterialScope::OwnSeriyo
        } else {
            PreparationWarehouseMaterialScope::AssignedItemGroups(
                state
                    .admin
                    .item_group_scope(other_item_groups)
                    .await
                    .map_err(material_scope_admin_error)?,
            )
        };
        scopes.insert(warehouse, scope);
    }

    Ok(scopes)
}

pub async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let preparation = store(&state)?;
    let scopes = warehouse_material_scopes(&state, &actor).await?;
    preparation
        .snapshot_with_warehouse_material_scopes(&actor.ref_, &scopes)
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

pub async fn list_materials(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .list_owned_materials(&actor.ref_)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn rename_material(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MaterialRename>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .rename_owned_material(&actor.ref_, input)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn delete_material(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<MaterialDelete>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .delete_owned_material(&actor.ref_, &input.item_code)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn update_material_warehouses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MaterialWarehousesUpdate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .update_owned_material_warehouses(&actor.ref_, input)
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

pub async fn receipt_reversal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReceiptReversalCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?
        .reverse_receipt(&actor, input)
        .await
        .map_err(error)?;
    state.warehouse_events.notify_updated(
        result["warehouse"].as_str().unwrap_or_default(),
        "raw_material_stock",
    );
    Ok(Json(result))
}

/// GScale/Tarozi sahifasidagi Tayyorlov masteri uchun QR'siz sodda kirim.
/// Oddiy preparation/receipts oqimi Seriyo materiallari uchun o'z qoidasi bilan
/// qoladi; bu endpoint faqat Tarozi sahifasining Rulon scope'ini ishlatadi.
pub async fn gscale_simple_receipt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReceiptCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    if !state
        .admin
        .principal_has_capability(&actor, Capability::GscalePrint)
        .await
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"}))));
    }
    let result = store(&state)?
        .receive_gscale_simple(&actor, input)
        .await
        .map_err(error)?;
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

pub async fn formula_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .formula_orders(&actor.ref_)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn saved_formula_show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.list_saved_formulas(&actor.ref_, &query.product_code, &query.material_id)
        .await.map(Json).map_err(error)
}

pub async fn saved_formula_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<FormulaUpsert>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.update_saved_formula(&actor, input).await.map(Json).map_err(error)
}

pub async fn saved_formula_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FormulaDeleteQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?.delete_saved_formula(&actor.ref_, &query.product_code, &query.name, &query.material_id)
        .await.map(Json).map_err(error)
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

/// Bola ombor ochish: faqat masterga eksklyuziv biriktirilgan ota ombor ostiga.
/// Yaratilgan bola avtomatik shu masterga biriktiriladi — snapshot,
/// kirim/sarf uni darhol ko'radi.
pub async fn create_warehouse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreparationWarehouseCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let created = store(&state)?
        .create_child_warehouse(&actor, input)
        .await
        .map_err(error)?;
    state.warehouse_events.notify_updated(
        created["warehouse"].as_str().unwrap_or_default(),
        "warehouse_assignment",
    );
    Ok(Json(created))
}

pub async fn rename_warehouse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PreparationWarehouseRename>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let old = input.warehouse.clone();
    let result = store(&state)?
        .rename_child_warehouse(&actor.ref_, input)
        .await
        .map_err(error)?;
    state.warehouse_events.notify_updated(&old, "warehouse_renamed");
    state.warehouse_events.notify_updated(
        result["warehouse"].as_str().unwrap_or_default(),
        "warehouse_renamed",
    );
    Ok(Json(result))
}

pub async fn delete_warehouse(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<PreparationWarehouseDelete>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?
        .delete_child_warehouse(&actor.ref_, &input.warehouse)
        .await
        .map_err(error)?;
    state.warehouse_events.notify_updated(
        &input.warehouse,
        "warehouse_deleted",
    );
    Ok(Json(result))
}
