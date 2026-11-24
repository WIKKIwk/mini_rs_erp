use super::*;
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};

pub async fn warehouse_live(
    State(state): State<AppState>,
    axum::Extension(session): axum::Extension<crate::http::router::LiveSession>,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    let ws = session.protect(ws, &state, |state, actor| async move {
        require_capability(&state, &actor, Capability::CatalogItemRead).await.is_ok()
    });
    let principal = session.principal;
    require_capability(&state, &principal, Capability::CatalogItemRead).await?;
    Ok(ws
        .on_upgrade(move |socket| warehouse_live_socket(state, socket))
        .into_response())
}

async fn warehouse_live_socket(
    state: AppState,
    mut socket: WebSocket,
) {
    let mut rx = state.warehouse_events.subscribe();
    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
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
