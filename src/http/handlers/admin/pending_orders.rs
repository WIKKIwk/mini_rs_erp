use super::*;
use crate::core::pending_orders::{PendingOrderError, PendingOrderStore};

#[derive(Default, Deserialize)]
pub struct PendingOrderQuery {
    #[serde(default)]
    pub id: String,
}

pub(super) fn pending_store(state: &AppState) -> Result<&dyn PendingOrderStore, AdminError> {
    state
        .pending_orders
        .as_deref()
        .ok_or_else(|| server_error("pending order store is unavailable"))
}

pub(super) fn pending_error(error: PendingOrderError) -> AdminError {
    match error {
        PendingOrderError::NotFound => not_found("pending_order_not_found"),
        PendingOrderError::Invalid(message) => bad_request(&message),
        PendingOrderError::Conflict => conflict("pending_order_conflict"),
        PendingOrderError::Store => server_error("pending order store failed"),
    }
}

pub async fn pending_orders(
    State(state): State<AppState>,
    Query(query): Query<PendingOrderQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let store = pending_store(&state)?;
    if query.id.is_empty() {
        Ok(json_response(store.list().await.map_err(pending_error)?))
    } else {
        Ok(json_response(
            store.get(&query.id).await.map_err(pending_error)?,
        ))
    }
}

pub async fn pending_order_image(
    State(state): State<AppState>,
    Query(query): Query<PendingOrderQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let image = pending_store(&state)?
        .image(&query.id)
        .await
        .map_err(pending_error)?;
    Ok((
        [
            (header::CONTENT_TYPE, image.image_mime),
            (header::CACHE_CONTROL, "private, no-cache".into()),
        ],
        image.body,
    )
        .into_response())
}
