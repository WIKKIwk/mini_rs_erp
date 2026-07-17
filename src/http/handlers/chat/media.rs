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
    ChatMediaAccessVariant, ChatMediaInitialization, ChatMediaInitializeInput,
    ChatMediaKind, ChatMediaRangeRequest, ChatMediaStorageError,
    ChatMediaStoredStream, ChatMediaStreamAccess, ChatMediaUploadView,
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

pub async fn media_upload_chunk(
    State(state): State<AppState>,
    Path((conversation_id, upload_id, chunk_index)): Path<(String, String, i32)>,
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
    let content_range = headers
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok());
    let stream = Box::pin(UploadBodyStream {
        inner: body.into_data_stream(),
    });
    let result = state
        .chat_media
        .upload_chunk(
            &principal,
            &conversation_id,
            &upload_id,
            chunk_index,
            content_length,
            content_range,
            stream,
        )
        .await
        .map_err(map_chat_media_error)?;
    Ok(Json(serde_json::json!({
        "media": result.media,
        "chunk": result.chunk,
    })))
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
    let range = media_range_request(&headers);
    match state
        .chat_media
        .media_stream_access(&principal, &media_id, variant, range)
        .await
        .map_err(map_chat_media_error)?
    {
        ChatMediaStreamAccess::Redirect { url, .. } => Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header(header::LOCATION, url)
            .header(header::CACHE_CONTROL, "private, max-age=60")
            .body(Body::empty())
            .map_err(|_| http_error(StatusCode::INTERNAL_SERVER_ERROR, "chat_media_response_failed")),
        ChatMediaStreamAccess::Local { content } => local_media_stream_response(content),
    }
}

fn media_range_request(headers: &HeaderMap) -> ChatMediaRangeRequest {
    let Some(value) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
    else {
        return ChatMediaRangeRequest::Full;
    };
    let Some(value) = value.strip_prefix("bytes=") else {
        return ChatMediaRangeRequest::Full;
    };
    if value.contains(',') {
        return ChatMediaRangeRequest::Full;
    }
    let Some((start, end)) = value.split_once('-') else {
        return ChatMediaRangeRequest::Full;
    };
    if start.is_empty() {
        return end
            .parse::<i64>()
            .ok()
            .filter(|length| *length > 0)
            .map(|length_bytes| ChatMediaRangeRequest::Suffix { length_bytes })
            .unwrap_or(ChatMediaRangeRequest::Full);
    }
    let Some(start_byte) = start.parse::<i64>().ok().filter(|value| *value >= 0) else {
        return ChatMediaRangeRequest::Full;
    };
    let end_byte_inclusive = if end.is_empty() {
        None
    } else {
        match end.parse::<i64>().ok().filter(|value| *value >= 0) {
            Some(value) => Some(value),
            None => return ChatMediaRangeRequest::Full,
        }
    };
    ChatMediaRangeRequest::From {
        start_byte,
        end_byte_inclusive,
    }
}

fn local_media_stream_response(
    content: ChatMediaStoredStream,
) -> Result<Response<Body>, ChatHttpError> {
    let status = if content.partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content.content_type.as_deref().unwrap_or("application/octet-stream"))
        .header(header::CONTENT_LENGTH, content.content_length().to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, max-age=300");
    if content.partial {
        response = response.header(
            header::CONTENT_RANGE,
            format!(
                "bytes {}-{}/{}",
                content.start_byte, content.end_byte_inclusive, content.total_size_bytes
            ),
        );
    }
    if let Some(etag) = content.etag.as_deref() {
        response = response.header(header::ETAG, etag);
    }
    response
        .body(Body::from_stream(content.stream))
        .map_err(|_| http_error(StatusCode::INTERNAL_SERVER_ERROR, "chat_media_response_failed"))
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
