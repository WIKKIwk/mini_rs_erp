use std::time::Duration;
use std::time::Instant;

use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::time::{interval, timeout};

use super::auth::authorize;
use super::{ChatHttpError, http_error};
use crate::app::AppState;
use crate::core::auth::models::Principal;

const CHAT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const CHAT_SEND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default, Deserialize)]
pub struct LiveQuery {
    ticket: String,
}

#[derive(Default, Deserialize)]
pub struct SyncQuery {
    cursor: Option<i64>,
    limit: Option<usize>,
}

pub async fn sync(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<SyncQuery>,
) -> Result<Json<crate::core::chat::ChatSyncPage>, ChatHttpError> {
    if method != Method::GET {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (_, principal) = authorize(&state, &headers).await?;
    state
        .chat
        .sync(
            &principal,
            query.cursor.unwrap_or(0),
            query.limit.unwrap_or(200),
        )
        .await
        .map(Json)
        .map_err(super::map_chat_error)
}

pub async fn socket_ticket(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ChatHttpError> {
    if method != Method::POST {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (_, principal) = authorize(&state, &headers).await?;
    let (ticket, expires_at_unix) = state
        .chat
        .issue_socket_ticket(principal)
        .await
        .map_err(super::map_chat_error)?;
    Ok(Json(serde_json::json!({
        "ticket": ticket,
        "expires_in_seconds": 30,
        "expires_at_unix": expires_at_unix,
    })))
}

pub async fn live(
    State(state): State<AppState>,
    method: Method,
    Query(query): Query<LiveQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ChatHttpError> {
    if method != Method::GET {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let principal = state
        .chat
        .consume_socket_ticket(&query.ticket)
        .await
        .map_err(|_| http_error(axum::http::StatusCode::UNAUTHORIZED, "chat_ticket_invalid"))?;
    Ok(ws
        .on_upgrade(move |socket| chat_socket(state, socket, principal))
        .into_response())
}

async fn chat_socket(state: AppState, mut socket: WebSocket, principal: Principal) {
    let mut receiver = state.chat.hub().subscribe(&principal).await;
    if !send_json(&mut socket, serde_json::json!({"event": "chat.ready"})).await {
        return;
    }
    let mut heartbeat = interval(CHAT_HEARTBEAT_INTERVAL);
    let mut last_inbound = Instant::now();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Pong(_))) | Some(Ok(Message::Ping(_))) => {
                        last_inbound = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {
                        last_inbound = Instant::now();
                    }
                }
            }
            event = receiver.recv() => {
                match event {
                    Ok(event) => {
                        let Ok(value) = serde_json::to_value(event) else { continue; };
                        if !send_json(&mut socket, value).await { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if !send_json(
                            &mut socket,
                            serde_json::json!({"event": "chat.resync_required"}),
                        ).await { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = heartbeat.tick() => {
                if last_inbound.elapsed() > CHAT_HEARTBEAT_INTERVAL * 3 {
                    break;
                }
                if !matches!(
                    timeout(CHAT_SEND_TIMEOUT, socket.send(Message::Ping(Vec::new().into()))).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        }
    }
}

async fn send_json(socket: &mut WebSocket, value: serde_json::Value) -> bool {
    let payload = value.to_string();
    matches!(
        timeout(
            CHAT_SEND_TIMEOUT,
            socket.send(Message::Text(payload.into()))
        )
        .await,
        Ok(Ok(()))
    )
}
