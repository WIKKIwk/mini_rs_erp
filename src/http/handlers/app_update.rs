use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::response::IntoResponse;
use serde::Serialize;
use tower_http::services::ServeFile;

use crate::app::AppState;
use crate::core::mobile_release::MobileReleaseError;

pub async fn android_metadata(State(state): State<AppState>) -> Response<Body> {
    match state.mobile_releases.android_release().await {
        Ok(release) => {
            let mut response = Json(release.public_info()).into_response();
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

    let mut service = ServeFile::new(apk_path);
    match service.try_call(request).await {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
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
