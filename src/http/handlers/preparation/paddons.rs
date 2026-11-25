use super::*;
use crate::core::inventory_movements::InventoryActor;
use crate::db::postgres_preparation::paddons::{
    PaddonCreate, PaddonIdentity, PaddonIssueCreate, PaddonLabelPrint, PaddonQuery,
    PaddonRollAttachment, PaddonRollOptions,
};

async fn paddon_actor(
    state: &AppState,
    headers: &HeaderMap,
    write: bool,
) -> Result<Principal, ApiError> {
    let token = super::super::auth::bearer_token(headers).unwrap_or_default();
    let actor = state.sessions.get(&token).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
    })?;
    let required = match actor.role {
        PrincipalRole::TayyorlovMasteri => Capability::PreparationAccess,
        PrincipalRole::MaterialTaminotchi if write => Capability::GscalePrint,
        PrincipalRole::MaterialTaminotchi => Capability::GscaleCatalogRead,
        _ => return Err(error(PreparationError::Forbidden)),
    };
    if !state.admin.principal_has_capability(&actor, required).await {
        return Err(error(PreparationError::Forbidden));
    }
    Ok(actor)
}

pub async fn raw_material_paddons(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PaddonQuery>,
) -> Result<([(axum::http::HeaderName, &'static str); 1], Json<Value>), ApiError> {
    let actor = paddon_actor(&state, &headers, false).await?;
    let paddons = store(&state)?
        .raw_material_paddons(&actor, query)
        .await
        .map_err(error)?;
    let paddons = if actor.role == PrincipalRole::TayyorlovMasteri {
        super::production_journal_sync::with_checkpoint(paddons, "paddons")
    } else {
        paddons
    };
    Ok((
        [(axum::http::header::CACHE_CONTROL, "private, no-store")],
        Json(paddons),
    ))
}

pub async fn receive_raw_material_paddon(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PaddonCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = paddon_actor(&state, &headers, true).await?;
    let groups = if actor.role == PrincipalRole::MaterialTaminotchi {
        state
            .admin
            .principal_assigned_item_group_scope(&actor)
            .await
            .map_err(material_scope_admin_error)?
            .into_iter()
            .map(|group| group.trim().to_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    let result = store(&state)?
        .receive_raw_material_paddon(&actor, input, &groups)
        .await
        .map_err(error)?;
    if let Some(warehouse) = result["warehouse"].as_str() {
        state
            .warehouse_events
            .notify_updated(warehouse, "raw_material_paddon");
    }
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct PaddonOptionsQuery {
    #[serde(default)]
    preview_partial: bool,
}

pub async fn production_paddon_options(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PaddonOptionsQuery>,
    Json(input): Json<PaddonRollOptions>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    store(&state)?
        .production_paddon_options(&actor, input, query.preview_partial)
        .await
        .map(Json)
        .map_err(error)
}

pub async fn attach_prepared_rolls(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PaddonRollAttachment>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    let result = store(&state)?
        .attach_prepared_rolls(&actor, input)
        .await
        .map_err(error)?;
    if let Some(warehouse) = result["warehouse"].as_str() {
        state.warehouse_events.notify_updated(warehouse, "raw_material_paddon");
    }
    state.production_maps.notify_live();
    Ok(Json(result))
}

pub async fn raw_material_paddon_issues(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PaddonIdentity>,
) -> Result<([(axum::http::HeaderName, &'static str); 1], Json<Value>), ApiError> {
    let actor = paddon_actor(&state, &headers, false).await?;
    let issues = store(&state)?
        .raw_material_paddon_issues(&actor, &query.code)
        .await
        .map_err(error)?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "private, no-store")],
        Json(json!({"issues": issues})),
    ))
}

pub async fn issue_raw_material_paddon(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PaddonIssueCreate>,
) -> Result<Json<Value>, ApiError> {
    let actor = paddon_actor(&state, &headers, true).await?;
    if input.moves_stock()
        && actor.role == PrincipalRole::MaterialTaminotchi
        && !state
            .admin
            .principal_has_capability(&actor, Capability::InventoryMovementManage)
            .await
    {
        return Err(error(PreparationError::Forbidden));
    }
    let warehouses = state
        .warehouses
        .assigned_warehouse_names(&actor)
        .await
        .map_err(|_| error(PreparationError::StoreFailed))?;
    let actor = InventoryActor::new(actor, false, warehouses);
    let result = store(&state)?
        .issue_raw_material_paddon(&actor, input)
        .await
        .map_err(error)?;
    if let Some(warehouse) = result["warehouse"].as_str() {
        state
            .warehouse_events
            .notify_updated(warehouse, "raw_material_paddon");
    }
    if result["transfer_status"].is_string()
        && let Some(warehouse) = result["destination"].as_str()
    {
        state
            .warehouse_events
            .notify_updated(warehouse, "raw_material_paddon");
    }
    state.production_maps.notify_live();
    Ok(Json(result))
}

pub async fn track_paddon_print(
    State(state): State<AppState>, headers: HeaderMap, Json(input): Json<PaddonLabelPrint>,
) -> Result<Json<Value>, ApiError> {
    let actor = paddon_actor(&state, &headers, true).await?;
    Ok(Json(store(&state)?.track_paddon_print(&actor, input).await.map_err(error)?))
}
