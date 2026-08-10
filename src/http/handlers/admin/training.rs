use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;
use serde::Deserialize;

use super::*;
use crate::app::AppState;
use crate::core::authz::Capability;
use crate::core::calculate_orders::{
    owner_key, validate_template, CalculateOrderError, CalculateOrderTemplate,
};
use crate::core::production_map::ProductionMapDefinition;
use crate::db::postgres_training_workspace::{
    PostgresTrainingWorkspaceStore, TrainingImage, TrainingWorkspaceError,
};

#[derive(Default, Deserialize)]
pub struct TrainingMapsQuery {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize)]
struct TrainingMapSaveWithOrderRequest {
    map: ProductionMapDefinition,
    template: CalculateOrderTemplate,
}

#[derive(Default, Deserialize)]
struct TrainingApparatusModeInput {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    enabled: bool,
}

#[derive(Default, Deserialize)]
pub struct TrainingRawMaterialAssignmentsQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
}

pub async fn training_production_maps(
    State(state): State<AppState>,
    Query(query): Query<TrainingMapsQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            if !query.id.trim().is_empty() {
                let saved = store
                    .map(&query.id)
                    .await
                    .map_err(training_workspace_error)?
                    .ok_or_else(|| not_found("training_map_not_found"))?;
                return Ok(json_response(saved));
            }
            let maps = store.maps().await.map_err(training_workspace_error)?;
            Ok(json_response(maps))
        }
        Method::DELETE => {
            let order_id = query.id.trim();
            if order_id.is_empty() {
                return Err(bad_request("training order id kerak"));
            }
            store
                .delete_order(order_id)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "id": order_id,
            })))
        }
        Method::PUT => {
            let map: ProductionMapDefinition = parse_json(&body)?;
            let saved = store
                .save_map(map)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(saved))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_production_map_save_with_order(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }

    let mut input: TrainingMapSaveWithOrderRequest = parse_json(&body)?;
    validate_template(&input.template).map_err(training_calculate_error)?;
    input.map.customer_name = input.template.customer.trim().to_string();
    if input.template.kg > 0.0 {
        let material_catalog = state
            .calculate_materials
            .list()
            .await
            .map_err(|_| server_error("calculate materials store failed"))?;
        super::production_maps::apply_authoritative_calculation(
            &mut input.map,
            &input.template,
            &material_catalog,
        )?;
    }

    let owner = owner_key("admin", &principal.ref_);
    let saved = training_store(&state)?
        .save_map_with_order(input.map, input.template, &owner)
        .await
        .map_err(training_workspace_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "saved": saved.saved,
        "template": saved.template,
    })))
}

pub async fn training_apparatus_modes(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            let modes = store
                .apparatus_modes()
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(serde_json::json!({"modes": modes})))
        }
        Method::PUT => {
            let input: TrainingApparatusModeInput = parse_json(&body)?;
            store
                .set_apparatus_mode(&input.apparatus, input.enabled)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(serde_json::json!({
                "apparatus": input.apparatus.trim(),
                "enabled": input.enabled,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_raw_material_assignments(
    State(state): State<AppState>,
    Query(query): Query<TrainingRawMaterialAssignmentsQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    let store = training_store(&state)?;
    match method {
        Method::GET => {
            let assignments = store
                .raw_material_assignments(&query.order_id, &query.apparatus)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(assignments))
        }
        Method::POST => {
            let payload: serde_json::Value = parse_json(&body)?;
            let order_id = payload
                .get("order_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim();
            if order_id.is_empty() {
                return Err(bad_request("order_id kerak"));
            }
            if store
                .map(order_id)
                .await
                .map_err(training_workspace_error)?
                .is_none()
            {
                return Err(not_found("training_order_not_found"));
            }
            let assignment = store
                .save_raw_material_assignment(payload)
                .await
                .map_err(training_workspace_error)?;
            Ok(json_response(assignment))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn training_order_image_upload(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    if body.is_empty() {
        return Err(bad_request("rasm kerak"));
    }
    const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
    if body.len() > MAX_IMAGE_BYTES {
        return Err(bad_request("rasm hajmi katta"));
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
    let extension = image_extension(&mime).ok_or_else(|| bad_request("rasm formati noto'g'ri"))?;
    let image_id = format!("training-img{}", unix_micros());
    let image_name = headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(clean_file_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("rang.{extension}"));
    let owner = owner_key("admin", &principal.ref_);
    let image = training_store(&state)?
        .save_image(
            &owner,
            TrainingImage {
                image_id,
                image_name,
                image_mime: mime,
                image_size_bytes: body.len() as u64,
                body: body.to_vec(),
            },
        )
        .await
        .map_err(training_workspace_error)?;
    let image_url = format!(
        "/v1/mobile/admin/training/images/view?id={}",
        image.image_id
    );
    Ok(json_response(serde_json::json!({
        "ok": true,
        "image": {
            "image_id": image.image_id,
            "image_name": image.image_name,
            "image_mime": image.image_mime,
            "image_size_bytes": image.image_size_bytes,
            "image_url": image_url,
        }
    })))
}

pub async fn training_order_image_view(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let image_id = query_value(&uri, "id")
        .filter(|value| safe_image_id(value))
        .ok_or_else(|| bad_request("id kerak"))?;
    let owner = owner_key("admin", &principal.ref_);
    let image = training_store(&state)?
        .image(&owner, &image_id)
        .await
        .map_err(training_workspace_error)?
        .ok_or_else(|| not_found("rasm topilmadi"))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, image.image_mime)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(image.body))
        .map_err(|_| server_error("training image response failed"))
}

fn training_store(state: &AppState) -> Result<&PostgresTrainingWorkspaceStore, AdminError> {
    state
        .training_workspace
        .as_ref()
        .ok_or_else(|| server_error("training workspace unavailable"))
}

fn training_calculate_error(error: CalculateOrderError) -> AdminError {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request(detail),
        CalculateOrderError::StoreFailed => server_error("calculate order save failed"),
    }
}

fn training_workspace_error(error: TrainingWorkspaceError) -> AdminError {
    match error {
        TrainingWorkspaceError::StoreFailed => server_error("training workspace store failed"),
        TrainingWorkspaceError::MapNotFound => not_found("training_map_not_found"),
        TrainingWorkspaceError::DuplicateOrderNumber => conflict("training_order_number_exists"),
        TrainingWorkspaceError::DuplicateRawMaterialAssignment => {
            conflict("training_material_assignment_exists")
        }
        TrainingWorkspaceError::InvalidInput(detail)
        | TrainingWorkspaceError::InvalidMap(detail) => bad_request(detail),
    }
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

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}
