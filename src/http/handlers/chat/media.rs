use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Json;
use axum::body::{Body, BodyDataStream, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Response, StatusCode, header};
use futures_core::Stream;
use serde::Deserialize;

use super::auth::authorize;
use super::{ChatHttpError, http_error, map_chat_media_error};
use crate::app::AppState;
use crate::core::chat_media::{
    ChatMediaAccess, ChatMediaAccessVariant, ChatMediaInitialization, ChatMediaInitializeInput,
    ChatMediaKind, ChatMediaStorageError, ChatMediaUploadView,
};

#[derive(Deserialize)]
struct InitializeMediaUploadRequest {
    client_upload_id: String,
    kind: ChatMediaKind,
    filename: String,
    content_type: String,
    size_bytes: i64,
    duration_ms: Option<i64>,
}

pub async fn media_uploads(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ChatMediaInitialization>, ChatHttpError> {
    if method != Method::POST {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (_, principal) = authorize(&state, &headers).await?;
    let request: InitializeMediaUploadRequest = serde_json::from_slice(&body).map_err(|_| {
        http_error(
            axum::http::StatusCode::BAD_REQUEST,
            "chat_media_request_invalid",
        )
    })?;
    state
        .chat_media
        .initialize_upload(
            &principal,
            &conversation_id,
            ChatMediaInitializeInput {
                client_upload_id: request.client_upload_id,
                kind: request.kind,
                filename: request.filename,
                content_type: request.content_type,
                size_bytes: request.size_bytes,
                duration_ms: request.duration_ms,
            },
        )
        .await
        .map(Json)
        .map_err(map_chat_media_error)
}

pub async fn media_upload(
    State(state): State<AppState>,
    Path((conversation_id, upload_id)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ChatHttpError> {
    let (_, principal) = authorize(&state, &headers).await?;
    let media = match method {
        Method::GET => {
            state
                .chat_media
                .upload_status(&principal, &conversation_id, &upload_id)
                .await
        }
        Method::DELETE => {
            state
                .chat_media
                .cancel_upload(&principal, &conversation_id, &upload_id)
                .await
        }
        _ => {
            return Err(http_error(
                axum::http::StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
            ));
        }
    }
    .map_err(map_chat_media_error)?;
    media_response(media)
}

pub async fn media_upload_content(
    State(state): State<AppState>,
    Path((conversation_id, upload_id)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<serde_json::Value>, ChatHttpError> {
    if method != Method::PUT {
        return Err(http_error(
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ));
    }
    let (_, principal) = authorize(&state, &headers).await?;
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok());
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let stream = Box::pin(UploadBodyStream {
        inner: body.into_data_stream(),
    });
    let media = state
        .chat_media
        .upload_content(
            &principal,
            &conversation_id,
            &upload_id,
            content_length,
            content_type,
            stream,
        )
        .await
        .map_err(map_chat_media_error)?;
    media_response(media)
}

pub async fn media_upload_complete(
    State(state): State<AppState>,
    Path((conversation_id, upload_id)): Path<(String, String)>,
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
    let media = state
        .chat_media
        .complete_upload(&principal, &conversation_id, &upload_id)
        .await
        .map_err(map_chat_media_error)?;
    media_response(media)
}

pub async fn media_access(
    State(state): State<AppState>,
    Path((media_id, variant)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response<Body>, ChatHttpError> {
    if method != Method::GET {
        return Err(http_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"));
    }
    let (_, principal) = authorize(&state, &headers).await?;
    let variant = match variant.as_str() {
        "content" => ChatMediaAccessVariant::Content,
        "thumbnail" => ChatMediaAccessVariant::Thumbnail,
        _ => return Err(http_error(StatusCode::NOT_FOUND, "chat_media_not_found")),
    };
    match state
        .chat_media
        .media_access(&principal, &media_id, variant)
        .await
        .map_err(map_chat_media_error)?
    {
        ChatMediaAccess::Redirect { url, .. } => Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header(header::LOCATION, url)
            .header(header::CACHE_CONTROL, "private, max-age=60")
            .body(Body::empty())
            .map_err(|_| http_error(StatusCode::INTERNAL_SERVER_ERROR, "chat_media_response_failed")),
        ChatMediaAccess::Local { content } => local_media_response(content, &headers),
    }
}

fn local_media_response(
    content: crate::core::chat_media::ChatMediaStoredContent,
    headers: &HeaderMap,
) -> Result<Response<Body>, ChatHttpError> {
    let total = content.bytes.len();
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| byte_range(value, total));
    let (status, start, end) = range
        .map(|(start, end)| (StatusCode::PARTIAL_CONTENT, start, end))
        .unwrap_or((StatusCode::OK, 0, total.saturating_sub(1)));
    let bytes = if total == 0 {
        content.bytes
    } else {
        content.bytes.slice(start..=end)
    };
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content.content_type.as_deref().unwrap_or("application/octet-stream"))
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, max-age=300");
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"));
    }
    response
        .body(Body::from(bytes))
        .map_err(|_| http_error(StatusCode::INTERNAL_SERVER_ERROR, "chat_media_response_failed"))
}

fn byte_range(value: &str, total: usize) -> Option<(usize, usize)> {
    if total == 0 {
        return None;
    }
    let value = value.trim().strip_prefix("bytes=")?;
    if value.contains(',') {
        return None;
    }
    let (start, end) = value.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<usize>().ok()?.clamp(1, total);
        return Some((total - suffix, total - 1));
    }
    let start = start.parse::<usize>().ok()?;
    if start >= total {
        return None;
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<usize>().ok()?.min(total - 1)
    };
    (end >= start).then_some((start, end))
}

fn media_response(media: ChatMediaUploadView) -> Result<Json<serde_json::Value>, ChatHttpError> {
    Ok(Json(serde_json::json!({"media": media})))
}

struct UploadBodyStream {
    inner: BodyDataStream,
}

impl Stream for UploadBodyStream {
    type Item = Result<Bytes, ChatMediaStorageError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_next(context) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(Some(Err(_))) => Poll::Ready(Some(Err(
                ChatMediaStorageError::OperationFailed,
            ))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
