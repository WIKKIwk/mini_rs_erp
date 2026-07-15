use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, Response, StatusCode, header};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::returned_paint::{
    ReturnedPaintError, ReturnedPaintRequest, ReturnedPaintRequestComplete,
    ReturnedPaintRequestCreate,
};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

#[derive(Debug, Deserialize)]
pub struct ReturnedPaintListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ReturnedPaintListResponse {
    items: Vec<ReturnedPaintRequest>,
    has_more: bool,
}

#[derive(Debug, Deserialize)]
pub struct ReturnedPaintImageQuery {
    #[serde(default)]
    id: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
}

pub async fn requests(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ReturnedPaintListQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    match method {
        Method::POST => {
            require_capability(
                &state,
                &principal,
                Capability::ReturnedPaintRequestCreate,
            )
            .await?;
            let input = serde_json::from_slice::<ReturnedPaintRequestCreate>(&body)
                .map_err(|_| bad_request("invalid json"))?;
            let request = state
                .returned_paint
                .create(input, &principal)
                .await
                .map_err(returned_paint_error)?;
            Ok(Json(
                serde_json::to_value(request).map_err(|_| server_error())?,
            ))
        }
        Method::GET => {
            require_capability(
                &state,
                &principal,
                Capability::ReturnedPaintRequestRead,
            )
            .await?;
            let limit = query.limit.unwrap_or(20).clamp(1, 100);
            let offset = query.offset.unwrap_or(0);
            let mut items = state
                .returned_paint
                .list(limit.saturating_add(1), offset)
                .await
                .map_err(returned_paint_error)?;
            let has_more = items.len() > limit;
            items.truncate(limit);
            Ok(Json(
                serde_json::to_value(ReturnedPaintListResponse { items, has_more })
                    .map_err(|_| server_error())?,
            ))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn complete_request(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authorize(&state, &headers).await?;
    require_capability(
        &state,
        &principal,
        Capability::ReturnedPaintRequestRead,
    )
    .await?;
    let input = serde_json::from_slice::<ReturnedPaintRequestComplete>(&body)
        .map_err(|_| bad_request("invalid json"))?;
    let request = state
        .returned_paint
        .complete(input)
        .await
        .map_err(returned_paint_error)?;
    Ok(Json(
        serde_json::to_value(request).map_err(|_| server_error())?,
    ))
}

pub async fn images(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ReturnedPaintImageQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_capability(
        &state,
        &principal,
        Capability::ReturnedPaintRequestCreate,
    )
    .await?;
    match method {
        Method::POST => {
            if body.is_empty() {
                return Err(bad_request("returned paint image is required"));
            }
            const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
            if body.len() > MAX_IMAGE_BYTES {
                return Err(bad_request("returned paint image is too large"));
            }
            let mime = headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("image/jpeg")
                .split(';')
                .next()
                .unwrap_or("image/jpeg")
                .trim()
                .to_ascii_lowercase();
            let extension = image_extension(&mime)
                .ok_or_else(|| bad_request("returned paint image format is invalid"))?;
            let image_name = headers
                .get("x-file-name")
                .and_then(|value| value.to_str().ok())
                .map(clean_file_name)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| format!("qaytarilgan-boyoq.{extension}"));
            let image = state
                .returned_paint
                .save_image(
                    query.order_id,
                    query.apparatus,
                    image_name,
                    mime,
                    body.to_vec(),
                    &principal,
                )
                .await
                .map_err(returned_paint_error)?;
            Ok(Json(serde_json::json!({"ok": true, "image": image})))
        }
        Method::DELETE => {
            state
                .returned_paint
                .delete_image(&query.id, &principal)
                .await
                .map_err(returned_paint_error)?;
            Ok(Json(serde_json::json!({"ok": true})))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn image_view(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ReturnedPaintImageQuery>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authorize(&state, &headers).await?;
    let image = state
        .returned_paint
        .image(&query.id)
        .await
        .map_err(returned_paint_error)?;
    let can_read = state
        .admin
        .principal_has_capability(&principal, Capability::ReturnedPaintRequestRead)
        .await;
    if image.owner_ref.trim() != principal.ref_.trim() && !can_read {
        return Err(forbidden());
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, image.image.image_mime)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(image.body))
        .map_err(|_| server_error())
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn require_capability(
    state: &AppState,
    principal: &Principal,
    capability: Capability,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if state
        .admin
        .principal_has_capability(principal, capability)
        .await
    {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "forbidden" }),
        ))
    }
}

fn returned_paint_error(
    error: ReturnedPaintError,
) -> (StatusCode, Json<ErrorResponse>) {
    let message = returned_paint_error_message(&error);
    match error {
        ReturnedPaintError::RequestNotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Qaytarilgan bo‘yoq hisoboti topilmadi",
            }),
        ),
        ReturnedPaintError::ImageNotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Qaytarilgan bo‘yoq rasmi topilmadi",
            }),
        ),
        ReturnedPaintError::MissingOrderId
        | ReturnedPaintError::MissingApparatus
        | ReturnedPaintError::MissingItems
        | ReturnedPaintError::InsufficientValues
        | ReturnedPaintError::ImageMismatch
        | ReturnedPaintError::ImageDeleteNotAllowed
        | ReturnedPaintError::InvalidUsage
        | ReturnedPaintError::InvalidCategory
        | ReturnedPaintError::MissingItemName
        | ReturnedPaintError::MissingValues
        | ReturnedPaintError::InvalidValue
        | ReturnedPaintError::NegativeFinalValue => {
            bad_request(message)
        }
        ReturnedPaintError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Qaytarilgan bo‘yoq hisoboti saqlanmadi",
            }),
        ),
    }
}

fn returned_paint_error_message(error: &ReturnedPaintError) -> &'static str {
    match error {
        ReturnedPaintError::MissingOrderId => "Buyurtma IDsi kiritilmagan",
        ReturnedPaintError::MissingApparatus => "Apparat kiritilmagan",
        ReturnedPaintError::MissingItems => "Kamida bitta qaytarilgan bo‘yoq qiymati kerak",
        ReturnedPaintError::InsufficientValues => {
            "Rasxot va Astatka tablarining har birida kamida 3 ta field kerak"
        }
        ReturnedPaintError::ImageMismatch => "Qaytarilgan bo‘yoq rasmi bu buyurtmaga tegishli emas",
        ReturnedPaintError::ImageDeleteNotAllowed => "Qaytarilgan bo‘yoq rasmini olib tashlab bo‘lmaydi",
        ReturnedPaintError::InvalidUsage => "Qaytarilgan bo‘yoq ishlatilish turi noto‘g‘ri",
        ReturnedPaintError::InvalidCategory => "Qaytarilgan bo‘yoq kategoriyasi noto‘g‘ri",
        ReturnedPaintError::MissingItemName => "Qaytarilgan bo‘yoq maydoni nomi kiritilmagan",
        ReturnedPaintError::MissingValues => "Qaytarilgan bo‘yoq maydoni qiymatlari kiritilmagan",
        ReturnedPaintError::InvalidValue => "Qaytarilgan bo‘yoq qiymati noto‘g‘ri",
        ReturnedPaintError::NegativeFinalValue => "Astatka Rasxotdan katta bo‘lishi mumkin emas",
        _ => "Qaytarilgan bo‘yoq hisoboti noto‘g‘ri",
    }
}

fn image_extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/heic" => Some("heic"),
        "image/heif" => Some("heif"),
        _ => None,
    }
}

fn clean_file_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '-' | '_' | ' ')
        })
        .take(120)
        .collect::<String>()
        .trim()
        .to_string()
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

fn forbidden() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse { error: "forbidden" }),
    )
}

fn bad_request(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::BAD_REQUEST, Json(ErrorResponse { error }))
}

fn server_error() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "returned paint request failed",
        }),
    )
}

fn method_not_allowed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ErrorResponse {
            error: "method not allowed",
        }),
    )
}
