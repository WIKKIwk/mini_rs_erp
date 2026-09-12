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
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(25));
    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = heartbeat.tick() => {
                if !send_warehouse_live_message(&mut socket, Message::Ping(Vec::new().into())).await {
                    break;
                }
            }
            received = rx.recv() => {
                match received {
                    Ok(event) => match serde_json::to_string(&event) {
                        Ok(payload) => {
                            if !send_warehouse_live_message(&mut socket, Message::Text(payload.into())).await {
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
    }
}

async fn send_warehouse_live_message(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(15), socket.send(message)).await,
        Ok(Ok(()))
    )
}
