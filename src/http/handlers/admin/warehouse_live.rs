use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};

#[derive(Default, Deserialize)]
pub struct WarehouseLiveQuery {
    #[serde(default)]
    token: String,
}

pub async fn warehouse_live(
    State(state): State<AppState>,
    Query(query): Query<WarehouseLiveQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    let principal = authenticated_principal_for_live(&state, &headers, &query.token).await?;
    require_capability(&state, &principal, Capability::CatalogItemRead).await?;
    Ok(ws
        .on_upgrade(move |socket| warehouse_live_socket(state, socket))
        .into_response())
}

async fn authenticated_principal_for_live(
    state: &AppState,
    headers: &HeaderMap,
    query_token: &str,
) -> Result<crate::core::auth::models::Principal, AdminError> {
    let token = query_token.trim().to_string();
    let token = if token.is_empty() {
        bearer_token(headers).ok_or_else(unauthorized)?
    } else {
        token
    };
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn warehouse_live_socket(state: AppState, mut socket: WebSocket) {
    let mut rx = state.warehouse_events.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => match serde_json::to_string(&event) {
                Ok(payload) => {
                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "warehouse live event serialization failed");
                }
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
