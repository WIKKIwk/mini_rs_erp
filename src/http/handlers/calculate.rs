use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri, header};
use serde::Serialize;

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::authz::Capability;
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderImage, CalculateOrderTemplate, owner_key,
};
use crate::core::formula::{CalculateRequest, calculate};
use crate::http::handlers::auth::bearer_token;

pub async fn calculate_route(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<CalculateErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let can_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let can_production_map = state
        .admin
        .principal_has_capability(&principal, Capability::ProductionMapManage)
        .await;
    if !can_admin && !can_production_map {
        return Err(forbidden());
    }
    let request: CalculateRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let response = calculate(request).map_err(|error| bad_request("invalid_input", error))?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

pub async fn calculate_orders_route(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<CalculateErrorResponse>)> {
    let principal = authorize_calculate_admin(&state, &headers).await?;
    let key = principal_owner_key(&principal);
    match method {
        Method::GET => {
            let templates = state
                .calculate_orders
                .list(&key)
                .await
                .map_err(store_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "templates": templates,
            })))
        }
        Method::POST => {
            let template: CalculateOrderTemplate = serde_json::from_slice(&body)
                .map_err(|_| bad_request("invalid_json", "invalid json"))?;
            let saved = state
                .calculate_orders
                .upsert(&key, template)
                .await
                .map_err(calculate_order_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "template": saved,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn calculate_order_delete_route(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<CalculateErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authorize_calculate_admin(&state, &headers).await?;
    let key = principal_owner_key(&principal);
    let request: CalculateOrderDeleteRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    if request.id.trim().is_empty() {
        return Err(bad_request("invalid_input", "id kerak"));
    }
    state
        .calculate_orders
        .delete(&key, &request.id)
        .await
        .map_err(store_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn calculate_order_image_upload_route(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<CalculateErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authorize_calculate_admin(&state, &headers).await?;
    if body.is_empty() {
        return Err(bad_request("invalid_input", "rasm kerak"));
    }
    const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
    if body.len() > MAX_IMAGE_BYTES {
        return Err(bad_request("invalid_input", "rasm hajmi katta"));
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
        .ok_or_else(|| bad_request("invalid_input", "rasm formati noto'g'ri"))?;
    let key = principal_owner_key(&principal);
    let image_id = new_image_id();
    let file_name = headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(clean_file_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("rang.{extension}"));
    let image = state
        .calculate_orders
        .save_image(
            &key,
            CalculateOrderImage {
                image_id,
                image_name: file_name,
                image_mime: mime,
                image_size_bytes: body.len() as u64,
                body: body.to_vec(),
            },
        )
        .await
        .map_err(calculate_order_error)?;
    let image_url = format!(
        "/v1/mobile/calculate/orders/image/view?id={}",
        image.image_id
    );
    Ok(Json(serde_json::json!({
        "ok": true,
        "image": {
            "image_id": &image.image_id,
            "image_name": &image.image_name,
            "image_mime": &image.image_mime,
            "image_size_bytes": image.image_size_bytes,
            "image_url": image_url
        }
    })))
}

pub async fn calculate_order_image_view_route(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response<Body>, (StatusCode, Json<CalculateErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authorize_calculate_admin(&state, &headers).await?;
    let image_id = query_value(&uri, "id")
        .filter(|value| safe_image_id(value))
        .ok_or_else(|| bad_request("invalid_input", "id kerak"))?;
    let key = principal_owner_key(&principal);
    let image = state
        .calculate_orders
        .get_image(&key, &image_id)
        .await
        .map_err(store_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(CalculateErrorResponse::new("not_found", "rasm topilmadi")),
            )
        })?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, image.image_mime)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(image.body))
        .map_err(|_| store_error(CalculateOrderError::StoreFailed))
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<CalculateErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn authorize_calculate_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<CalculateErrorResponse>)> {
    let principal = authenticated_principal(state, headers).await?;
    let can_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let can_production_map = state
        .admin
        .principal_has_capability(&principal, Capability::ProductionMapManage)
        .await;
    if !can_admin && !can_production_map {
        return Err(forbidden());
    }
    Ok(principal)
}

fn principal_owner_key(principal: &Principal) -> String {
    let role = match principal.role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    };
    owner_key(role, &principal.ref_)
}

fn image_extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn clean_file_name(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '/' | '\\' | '\0' | '\r' | '\n'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn safe_image_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn query_value(uri: &Uri, key: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (raw_key == key).then(|| raw_value.trim().to_string())
    })
}

fn new_image_id() -> String {
    format!("img{}", unix_micros())
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

fn calculate_order_error(error: CalculateOrderError) -> (StatusCode, Json<CalculateErrorResponse>) {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request("invalid_input", detail),
        CalculateOrderError::StoreFailed => store_error(CalculateOrderError::StoreFailed),
    }
}

fn store_error(_: CalculateOrderError) -> (StatusCode, Json<CalculateErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(CalculateErrorResponse::new("store_failed", "store failed")),
    )
}

fn unauthorized() -> (StatusCode, Json<CalculateErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(CalculateErrorResponse::new("unauthorized", "unauthorized")),
    )
}

fn forbidden() -> (StatusCode, Json<CalculateErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(CalculateErrorResponse::new("forbidden", "forbidden")),
    )
}

fn bad_request(
    error: &'static str,
    detail: impl Into<String>,
) -> (StatusCode, Json<CalculateErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(CalculateErrorResponse::new(error, detail)),
    )
}

fn method_not_allowed() -> (StatusCode, Json<CalculateErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(CalculateErrorResponse::new(
            "method_not_allowed",
            "method not allowed",
        )),
    )
}

#[derive(Debug, Serialize)]
pub struct CalculateErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

#[derive(Debug, serde::Deserialize)]
struct CalculateOrderDeleteRequest {
    #[serde(default)]
    id: String,
}

impl CalculateErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}
