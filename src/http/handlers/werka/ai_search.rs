use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use serde_json::{Value, json};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::werka::ports::{WerkaAiSearchError, WerkaAiSearchImage};
use crate::http::handlers::auth::bearer_token;

const MAX_UPLOAD_SIZE: usize = 8 << 20;

pub async fn ai_search_suggestion(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if method != Method::POST {
        return Err(error_with_code(
            StatusCode::METHOD_NOT_ALLOWED,
            "method not allowed",
            "method_not_allowed",
        ));
    }
    let principal = authorize(&state, &headers).await?;
    if !state
        .admin
        .principal_has_capability(&principal, Capability::WerkaAccess)
        .await
    {
        return Err(error(StatusCode::FORBIDDEN, "forbidden"));
    }
    if !state.werka.ai_search_configured() {
        return Err(error_with_code(
            StatusCode::SERVICE_UNAVAILABLE,
            "werka ai search is not configured",
            "not_configured",
        ));
    }

    let upload = match parse_image_upload(&headers, &body) {
        Ok(upload) => upload,
        Err(ImageUploadError::InvalidUpload) => {
            return Err(error_with_code(
                StatusCode::BAD_REQUEST,
                "invalid image upload",
                "invalid_image",
            ));
        }
        Err(ImageUploadError::MissingImage) => {
            return Err(error_with_code(
                StatusCode::BAD_REQUEST,
                "image is required",
                "invalid_image",
            ));
        }
    };

    match state
        .werka
        .ai_search_suggestion(WerkaAiSearchImage {
            bytes: upload.bytes,
            mime_type: detect_image_mime_type(
                upload.filename.as_deref().unwrap_or(""),
                upload.content_type.as_deref().unwrap_or(""),
                &upload.bytes_for_mime,
            ),
        })
        .await
    {
        Ok(suggestion) => Ok(Json(
            serde_json::to_value(suggestion).unwrap_or_else(|_| json!({})),
        )),
        Err(error) if error.code == "no_result" => Ok(Json(json!({
            "display_query": "",
            "background_queries": null,
            "visible_text": "",
        }))),
        Err(error) => Err(ai_error(error)),
    }
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<Value>)> {
    let token =
        bearer_token(headers).ok_or_else(|| error(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    state
        .sessions
        .get(&token)
        .await
        .map_err(|_| error(StatusCode::UNAUTHORIZED, "unauthorized"))
}

fn parse_image_upload(headers: &HeaderMap, body: &[u8]) -> Result<ImageUpload, ImageUploadError> {
    if body.len() > MAX_UPLOAD_SIZE {
        return Err(ImageUploadError::InvalidUpload);
    }
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let boundary = multipart_boundary(content_type).ok_or(ImageUploadError::InvalidUpload)?;
    let part =
        find_multipart_field(body, &boundary, "image").ok_or(ImageUploadError::MissingImage)?;
    if part.bytes.is_empty() {
        return Err(ImageUploadError::MissingImage);
    }
    Ok(part)
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))
        .map(|boundary| boundary.trim_matches('"').to_string())
        .filter(|boundary| !boundary.is_empty())
}

fn find_multipart_field(body: &[u8], boundary: &str, field: &str) -> Option<ImageUpload> {
    let marker = format!("--{boundary}").into_bytes();
    let mut search_from = 0;
    while let Some(relative) = find_bytes(&body[search_from..], &marker) {
        let start = search_from + relative + marker.len();
        if body.get(start..start + 2) == Some(b"--") {
            return None;
        }
        let part_start = if body.get(start..start + 2) == Some(b"\r\n") {
            start + 2
        } else if body.get(start..start + 1) == Some(b"\n") {
            start + 1
        } else {
            start
        };
        let next = find_bytes(&body[part_start..], &marker)
            .map(|index| part_start + index)
            .unwrap_or(body.len());
        let mut part = &body[part_start..next];
        if part.ends_with(b"\r\n") {
            part = &part[..part.len() - 2];
        } else if part.ends_with(b"\n") {
            part = &part[..part.len() - 1];
        }
        if let Some(upload) = parse_part(part, field) {
            return Some(upload);
        }
        search_from = next;
    }
    None
}

fn parse_part(part: &[u8], field: &str) -> Option<ImageUpload> {
    let header_end = find_bytes(part, b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| find_bytes(part, b"\n\n").map(|index| (index, 2)))?;
    let headers = String::from_utf8_lossy(&part[..header_end.0]);
    if !headers
        .to_lowercase()
        .contains(&format!("name=\"{}\"", field.to_lowercase()))
    {
        return None;
    }
    let filename = header_value_param(&headers, "filename");
    let content_type = headers
        .lines()
        .find_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.trim().eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.trim().to_string());
    let bytes = part[header_end.0 + header_end.1..].to_vec();
    Some(ImageUpload {
        bytes_for_mime: bytes.clone(),
        bytes,
        filename,
        content_type,
    })
}

fn header_value_param(headers: &str, name: &str) -> Option<String> {
    headers.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim().trim_matches('"').to_string())
        } else {
            None
        }
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn detect_image_mime_type(filename: &str, content_type: &str, image_bytes: &[u8]) -> String {
    match content_type.trim().to_lowercase().as_str() {
        "image/png" => return "image/png".to_string(),
        "image/webp" => return "image/webp".to_string(),
        "image/jpeg" | "image/jpg" => return "image/jpeg".to_string(),
        _ => {}
    }
    match std::path::Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => return "image/png".to_string(),
        "webp" => return "image/webp".to_string(),
        "jpg" | "jpeg" => return "image/jpeg".to_string(),
        _ => {}
    }
    if image_bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png".to_string();
    }
    if image_bytes.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg".to_string();
    }
    if image_bytes.len() >= 12 && &image_bytes[..4] == b"RIFF" && &image_bytes[8..12] == b"WEBP" {
        return "image/webp".to_string();
    }
    "image/jpeg".to_string()
}

fn ai_error(error: WerkaAiSearchError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "not_configured" => StatusCode::SERVICE_UNAVAILABLE,
        "invalid_image" => StatusCode::BAD_REQUEST,
        _ => StatusCode::BAD_GATEWAY,
    };
    error_with_code(status, &error.message, error.code)
}

fn error(status: StatusCode, message: &'static str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message })))
}

fn error_with_code(status: StatusCode, message: &str, code: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message, "code": code })))
}

struct ImageUpload {
    bytes: Vec<u8>,
    bytes_for_mime: Vec<u8>,
    filename: Option<String>,
    content_type: Option<String>,
}

enum ImageUploadError {
    InvalidUpload,
    MissingImage,
}

#[cfg(test)]
mod tests {
    use super::{detect_image_mime_type, multipart_boundary};

    #[test]
    fn detects_mime_like_go() {
        assert_eq!(detect_image_mime_type("a.jpg", "", b""), "image/jpeg");
        assert_eq!(detect_image_mime_type("a.png", "", b""), "image/png");
        assert_eq!(detect_image_mime_type("", "image/jpg", b""), "image/jpeg");
    }

    #[test]
    fn extracts_multipart_boundary() {
        assert_eq!(
            multipart_boundary("multipart/form-data; boundary=\"abc\""),
            Some("abc".to_string())
        );
    }
}
