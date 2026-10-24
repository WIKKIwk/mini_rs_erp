use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::response::IntoResponse;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tower_http::services::ServeFile;

use crate::app::AppState;
use crate::core::mobile_release::MobileReleaseError;
use crate::http::ws::{Message, WebSocketUpgrade};

pub async fn android_live(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response<Body> {
    let mut releases = state.mobile_releases.subscribe_android_release();
    ws.on_upgrade(move |mut socket| async move {
        loop {
            let release = releases.borrow_and_update().clone();
            let payload = serde_json::json!({"type": "android_release_changed", "release": release});
            if !matches!(
                tokio::time::timeout(std::time::Duration::from_secs(5), socket.send(Message::Text(payload.to_string().into()))).await,
                Ok(Ok(()))
            ) { break; }
            loop {
                tokio::select! {
                    changed = releases.changed() => {
                        if changed.is_err() { return; }
                        break;
                    }
                    incoming = socket.recv() => {
                        if matches!(incoming, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) { return; }
                    }
                }
            }
        }
    }).into_response()
}

pub async fn android_metadata(State(state): State<AppState>) -> Response<Body> {
    match state.mobile_releases.android_release_info().await {
        Ok(release) => {
            let mut response = Json(release).into_response();
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store, max-age=0"),
            );
            response
        }
        Err(MobileReleaseError::NotPublished) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => release_unavailable(error),
    }
}

pub async fn android_apk(
    State(state): State<AppState>,
    Path(file_name): Path<String>,
    request: Request,
) -> Response<Body> {
    let apk_path = match state.mobile_releases.android_apk_path(&file_name).await {
        Ok(path) => path,
        Err(MobileReleaseError::ApkNotFound | MobileReleaseError::InvalidField(_)) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => return release_unavailable(error),
    };

    apk_response(apk_path, &file_name, request).await
}

async fn apk_response(
    apk_path: std::path::PathBuf,
    file_name: &str,
    request: Request,
) -> Response<Body> {
    let metadata = match tokio::fs::metadata(&apk_path).await {
        Ok(metadata) => metadata,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    // DownloadManager sends If-Match and Range when resuming. The publisher
    // gives every APK a unique name; size/mtime also distinguish legacy names.
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let etag = format!(
        "\"{:x}\"",
        Sha256::digest(format!("{file_name}:{}:{modified}", metadata.len()))
    );
    if request
        .headers()
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|expected| {
            expected != "*" && !expected.split(',').any(|value| value.trim() == etag)
        })
    {
        return StatusCode::PRECONDITION_FAILED.into_response();
    }
    let mut service = ServeFile::new(apk_path);
    match service.try_call(request).await {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
            parts.headers.insert(
                header::ETAG,
                HeaderValue::from_str(&etag).expect("hex ETag is a valid header"),
            );
            parts.headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.android.package-archive"),
            );
            parts.headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store, max-age=0"),
            );
            parts.headers.insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{}\"", file_name))
                    .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
            );
            parts.headers.insert(
                header::HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            );
            Response::from_parts(parts, Body::new(body))
        }
        Err(error) => {
            tracing::error!(%error, "failed to stream published Android APK");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn release_unavailable(error: MobileReleaseError) -> Response<Body> {
    tracing::error!(%error, "Android mobile release is unavailable");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "mobile_release_unavailable",
        }),
    )
        .into_response()
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apk_download_resumes_with_matching_etag_and_range() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("accord.apk");
        tokio::fs::write(&path, b"0123456789").await.unwrap();
        let response = apk_response(path.clone(), "accord.apk", Request::new(Body::empty())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()[header::ETAG].clone();
        let request = Request::builder()
            .header(header::RANGE, "bytes=4-")
            .header(header::IF_MATCH, etag.clone())
            .body(Body::empty())
            .unwrap();
        let resumed = apk_response(path.clone(), "accord.apk", request).await;
        assert_eq!(resumed.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resumed.headers()[header::ETAG], etag);
        assert_eq!(resumed.headers()[header::CONTENT_RANGE], "bytes 4-9/10");
        assert_eq!(
            axum::body::to_bytes(resumed.into_body(), 100)
                .await
                .unwrap(),
            "456789"
        );

        let stale = Request::builder()
            .header(header::RANGE, "bytes=4-")
            .header(header::IF_MATCH, "\"different-apk\"")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            apk_response(path, "accord.apk", stale).await.status(),
            StatusCode::PRECONDITION_FAILED
        );
    }
}
