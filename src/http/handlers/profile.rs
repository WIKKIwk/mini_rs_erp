use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::http::handlers::auth::{ErrorResponse, bearer_token, with_avatar_proxy};

const AVATAR_BODY_LIMIT: usize = 5 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct AvatarViewQuery {
    token: Option<String>,
}

pub async fn profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Result<Json<Principal>, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    let principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;

    match method {
        Method::GET => {
            let current = state.profiles.refresh(principal).await;
            state.sessions.update(&token, current.clone()).await;
            Ok(Json(with_avatar_proxy(&headers, current, &token)))
        }
        Method::PUT => {
            let request: ProfileUpdateRequest = serde_json::from_slice(&body).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "invalid json",
                    }),
                )
            })?;
            let current = state
                .profiles
                .update_nickname(principal, &request.nickname)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "nickname update failed",
                        }),
                    )
                })?;
            state.sessions.update(&token, current.clone()).await;
            Ok(Json(with_avatar_proxy(&headers, current, &token)))
        }
        _ => Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        )),
    }
}

pub async fn avatar_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> Result<Json<Principal>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        ));
    }
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    let principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    if body.len() > AVATAR_BODY_LIMIT {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid multipart",
            }),
        ));
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let Some(boundary) = multipart_boundary(content_type) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid multipart",
            }),
        ));
    };
    let Some(upload) = parse_avatar_multipart(&body, &boundary) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "avatar is required",
            }),
        ));
    };

    let current = state
        .profiles
        .upload_avatar(
            principal,
            &upload.filename,
            &upload.content_type,
            upload.content,
        )
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "avatar upload failed",
                }),
            )
        })?;
    state.sessions.update(&token, current.clone()).await;
    Ok(Json(with_avatar_proxy(&headers, current, &token)))
}

pub async fn avatar_view(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AvatarViewQuery>,
) -> Response {
    let token = match query
        .token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(token) => token.to_string(),
        None => match bearer_token(&headers) {
            Some(token) => token,
            None => return unauthorized().into_response(),
        },
    };

    let Ok(principal) = state.sessions.get(&token).await else {
        return unauthorized().into_response();
    };
    if !state
        .admin
        .principal_has_capability(&principal, Capability::SupplierAvatarManage)
        .await
    {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(ErrorResponse { error: "forbidden" }),
        )
            .into_response();
    }

    match state.profiles.download_avatar(principal).await {
        Ok(Some(file)) => {
            let mut response = Body::from(file.body).into_response();
            if !file.content_type.trim().is_empty() {
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    file.content_type
                        .parse()
                        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
                );
            }
            response
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(ErrorResponse {
                error: "avatar fetch failed",
            }),
        )
            .into_response(),
    }
}

fn unauthorized() -> (StatusCode, axum::Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

#[derive(Debug, Deserialize)]
pub struct ProfileUpdateRequest {
    pub nickname: String,
}

struct AvatarUpload {
    filename: String,
    content_type: String,
    content: Vec<u8>,
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))
        .map(|value| value.trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
}

fn parse_avatar_multipart(body: &[u8], boundary: &str) -> Option<AvatarUpload> {
    let marker = format!("--{boundary}").into_bytes();
    let mut offset = 0;
    while let Some(start) = find_bytes(&body[offset..], &marker) {
        let section_start = offset + start + marker.len();
        let Some(next) = find_bytes(&body[section_start..], &marker) else {
            break;
        };
        let mut section = &body[section_start..section_start + next];
        offset = section_start + next;
        section = trim_prefix(section, b"\r\n");
        section = trim_suffix(section, b"\r\n");
        section = trim_suffix(section, b"--");

        let Some(headers_end) = find_bytes(section, b"\r\n\r\n") else {
            continue;
        };
        let raw_headers = String::from_utf8_lossy(&section[..headers_end]);
        if !raw_headers.contains("name=\"avatar\"") {
            continue;
        }
        let raw_content = trim_suffix(&section[headers_end + 4..], b"\r\n");
        let mut filename = "avatar.png".to_string();
        let mut content_type = "image/png".to_string();
        for line in raw_headers.lines() {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("content-disposition:") {
                if let Some(value) = disposition_param(line, "filename") {
                    filename = value;
                }
            } else if lower.starts_with("content-type:") {
                content_type = line
                    .split_once(':')
                    .map(|(_, value)| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or(content_type);
            }
        }
        return Some(AvatarUpload {
            filename,
            content_type,
            content: raw_content.to_vec(),
        });
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn trim_prefix<'a>(value: &'a [u8], prefix: &[u8]) -> &'a [u8] {
    value.strip_prefix(prefix).unwrap_or(value)
}

fn trim_suffix<'a>(value: &'a [u8], suffix: &[u8]) -> &'a [u8] {
    value.strip_suffix(suffix).unwrap_or(value)
}

fn disposition_param(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=\"");
    line.split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(&prefix))
        .and_then(|rest| rest.split('"').next())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

#[allow(dead_code)]
fn _profile_update_request_contract(_request: ProfileUpdateRequest) {}
