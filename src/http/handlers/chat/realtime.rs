use std::time::Duration;
use std::time::Instant;

use axum::Json;
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::time::{interval, timeout};

use super::auth::authorize;
use super::{ChatHttpError, http_error};
use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::session::scoped_ticket::{issue_ticket, TicketScope};
use crate::http::router::LiveSession;

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
    let (session_token, principal) = authorize(&state, &headers).await?;
    let (ticket, expires_at_unix) = state
        .chat
        .issue_socket_ticket(principal)
        .await
        .map_err(super::map_chat_error)?;
    // Bind the existing single-use database ticket to the issuing ERP
    // session. The public live-ticket issuer accepts only exact route paths.
    let purpose = format!("chat-socket/{ticket}");
    let proof = issue_ticket(&session_token, TicketScope::Live { path: &purpose });
    Ok(Json(serde_json::json!({
        "ticket": format!("{ticket}.{proof}"),
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
    let invalid = || http_error(axum::http::StatusCode::UNAUTHORIZED, "chat_ticket_invalid");
    let (ticket, proof) = query.ticket.split_once('.').ok_or_else(invalid)?;
    // Short, URL-safe, random tickets are generated from exactly 24 bytes.
    if ticket.len() != 32 {
        return Err(invalid());
    }
    let purpose = format!("chat-socket/{ticket}");
    let (digest, principal) = state.sessions.resolve_ticket(proof, TicketScope::Live { path: &purpose })
        .await.map_err(|_| invalid())?;
    let bound = state.chat.consume_socket_ticket(ticket).await.map_err(|_| invalid())?;
    if bound.role != principal.role || bound.ref_ != principal.ref_
        || !matches!(state.admin.principal_is_active(&principal).await, Ok(true))
    {
        return Err(invalid());
    }
    let session = LiveSession { digest, principal };
    let ws = session.protect(ws, &state, |state, actor| async move {
        crate::core::chat::can_participate_in_chat(&actor.role)
            && matches!(state.admin.principal_is_active(&actor).await, Ok(true))
    });
    let principal = session.principal;
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
