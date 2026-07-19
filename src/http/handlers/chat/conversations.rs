use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method};
use serde::Deserialize;

use super::auth::authorize;
use super::directory::{proxied_avatar_url, resolve_target};
use super::{ChatHttpError, http_error, map_chat_error};
use crate::app::AppState;
use crate::core::auth::models::PrincipalRole;
use crate::core::chat::{
    ChatConversation, ChatConversationPage, ChatMessagePage, ChatPrincipalInput, ChatSendResult,
};

#[derive(Default, Deserialize)]
pub struct PageQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Default, Deserialize)]
pub struct MessagesQuery {
    limit: Option<usize>,
    before_sequence: Option<i64>,
    after_sequence: Option<i64>,
}

#[derive(Deserialize)]
struct CreateDmRequest {
    role: PrincipalRole,
    #[serde(rename = "ref")]
    ref_: String,
}

#[derive(Deserialize)]
struct SendMessageRequest {
    client_message_id: String,
    #[serde(default)]
    body: String,
    media_id: Option<String>,
}

#[derive(Deserialize)]
struct MarkReadRequest {
    sequence: i64,
    device_id: String,
}

pub async fn conversations(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<ChatConversationPage>, ChatHttpError> {
    if method != Method::GET {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (token, principal) = authorize(&state, &headers).await?;
    ensure_actor(&state, &principal).await?;
    let mut page = state
        .chat
        .conversations(
            &principal,
            query.limit.unwrap_or(30),
            query.offset.unwrap_or(0),
        )
        .await
        .map_err(map_chat_error)?;
    proxy_conversation_avatars(&headers, &token, &mut page.items);
    Ok(Json(page))
}

pub async fn create_dm(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ChatConversation>, ChatHttpError> {
    if method != Method::POST {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (token, principal) = authorize(&state, &headers).await?;
    let request: CreateDmRequest = serde_json::from_slice(&body)
        .map_err(|_| http_error(axum::http::StatusCode::BAD_REQUEST, "chat_request_invalid"))?;
    let actor = actor_input(&state, &principal).await;
    let target = resolve_target(&state, &request.role, &request.ref_).await?;
    let mut conversation = state
        .chat
        .create_or_get_dm(actor, target)
        .await
        .map_err(map_chat_error)?;
    proxy_conversation_avatars(&headers, &token, std::slice::from_mut(&mut conversation));
    Ok(Json(conversation))
}

pub async fn conversation_messages(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<MessagesQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ChatHttpError> {
    let (_, principal) = authorize(&state, &headers).await?;
    match method {
        Method::GET => {
            let page: ChatMessagePage = state
                .chat
                .messages(
                    &principal,
                    &conversation_id,
                    query.before_sequence,
                    query.after_sequence,
                    query.limit.unwrap_or(50),
                )
                .await
                .map_err(map_chat_error)?;
            Ok(Json(serde_json::to_value(page).map_err(|_| {
                http_error(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "chat_serialize_failed",
                )
            })?))
        }
        Method::POST => {
            let request: SendMessageRequest = serde_json::from_slice(&body).map_err(|_| {
                http_error(axum::http::StatusCode::BAD_REQUEST, "chat_request_invalid")
            })?;
            let result: ChatSendResult = if let Some(media_id) = request.media_id {
                state
                    .chat
                    .send_media_message(
                        &principal,
                        &conversation_id,
                        &request.client_message_id,
                        &request.body,
                        &media_id,
                    )
                    .await
            } else {
                state
                    .chat
                    .send_message(
                        &principal,
                        &conversation_id,
                        &request.client_message_id,
                        &request.body,
                    )
                    .await
            }
            .map_err(map_chat_error)?;
            Ok(Json(serde_json::json!({
                "message": result.message,
                "created": result.created,
            })))
        }
        _ => Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        )),
    }
}

pub async fn mark_read(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ChatHttpError> {
    if method != Method::POST {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (_, principal) = authorize(&state, &headers).await?;
    let request: MarkReadRequest = serde_json::from_slice(&body)
        .map_err(|_| http_error(axum::http::StatusCode::BAD_REQUEST, "chat_request_invalid"))?;
    state
        .chat
        .mark_read(
            &principal,
            &conversation_id,
            request.sequence,
            &request.device_id,
        )
        .await
        .map_err(map_chat_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn mark_delivered(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ChatHttpError> {
    if method != Method::POST {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (_, principal) = authorize(&state, &headers).await?;
    let request: MarkReadRequest = serde_json::from_slice(&body)
        .map_err(|_| http_error(axum::http::StatusCode::BAD_REQUEST, "chat_request_invalid"))?;
    state
        .chat
        .mark_delivered(
            &principal,
            &conversation_id,
            request.sequence,
            &request.device_id,
        )
        .await
        .map_err(map_chat_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn ensure_actor(
    state: &AppState,
    principal: &crate::core::auth::models::Principal,
) -> Result<(), ChatHttpError> {
    state
        .chat
        .ensure_principal(actor_input(state, principal).await)
        .await
        .map(|_| ())
        .map_err(map_chat_error)
}

async fn actor_input(
    state: &AppState,
    principal: &crate::core::auth::models::Principal,
) -> ChatPrincipalInput {
    let refreshed = state.profiles.refresh(principal.clone()).await;
    ChatPrincipalInput {
        role: refreshed.role,
        ref_: refreshed.ref_,
        display_name: refreshed.display_name,
        avatar_url: refreshed.avatar_url,
    }
}

fn proxy_conversation_avatars(
    headers: &HeaderMap,
    token: &str,
    conversations: &mut [ChatConversation],
) {
    for conversation in conversations {
        if let Some(peer) = &mut conversation.peer {
            peer.avatar_url =
                proxied_avatar_url(headers, &peer.avatar_url, &peer.role, &peer.ref_, token);
        }
    }
}
