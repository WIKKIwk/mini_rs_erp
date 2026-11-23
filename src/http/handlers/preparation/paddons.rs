use super::*;
use crate::db::postgres_preparation::paddons::{PaddonCreate, PaddonQuery};

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
