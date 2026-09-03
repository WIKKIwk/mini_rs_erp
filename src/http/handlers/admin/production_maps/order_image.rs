use super::*;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::Response;

/// Serves the calculate-page photo linked to a production order for the order
/// detail sheet.
///
/// The image is resolved through the map's `image_id`, not through the
/// viewer's owner key: sheets are opened by operators other than the admin
/// who uploaded the photo. Any live-stream viewer may load it, and orders
/// without a photo get a fast `404` so the sheet simply hides the image.
pub async fn production_map_order_image_view(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
            Capability::RawMaterialAssign,
            Capability::QolipManage,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let order_id = query_value(&uri, "order_id")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| bad_request("order_id kerak"))?;
    let map = state
        .production_maps
        .raw_map(&order_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| not_found("order topilmadi"))?;
    let image_id = match map_image_id(&map) {
        Some(image_id) => Some(image_id),
        // Orders created before photo linkage (or through paths that skip
        // it) fall back to the calculate template archive, matched the same
        // way the sheets/telegram sync matches templates to orders.
        None => template_image_id_for_map(&state, &map).await?,
    };
    let Some(image_id) = image_id else {
        return Err(not_found("rasm topilmadi"));
    };
    let image = state
        .calculate_orders
        .get_image_global(&image_id)
        .await
        .map_err(|_| server_error("order image store failed"))?
        .ok_or_else(|| not_found("rasm topilmadi"))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, image.image_mime)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(image.body))
        .map_err(|_| server_error("order image response failed"))
}

fn map_image_id(map: &ProductionMapDefinition) -> Option<String> {
    let image_id = map.image_id.trim().to_string();
    (!image_id.is_empty()).then_some(image_id)
}

async fn template_image_id_for_map(
    state: &AppState,
    map: &ProductionMapDefinition,
) -> Result<Option<String>, AdminError> {
    let templates = state
        .calculate_orders
        .list_all()
        .await
        .map_err(|_| server_error("order template lookup failed"))?;
    let map_id = map.id.trim();
    let order_number = map.order_number.trim();
    let code = map.code.trim();
    for template in &templates {
        let matches = (!template.source_map_id.trim().is_empty()
            && template.source_map_id.trim() == map_id)
            || (!order_number.is_empty() && template.order_number.trim() == order_number)
            || (!code.is_empty() && template.code.trim() == code);
        if !matches {
            continue;
        }
        let image_id = template.image_id.trim().to_string();
        if !image_id.is_empty() {
            return Ok(Some(image_id));
        }
    }
    Ok(None)
}

fn query_value(uri: &Uri, key: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (raw_key, raw_value) = pair.split_once('=')?;
        (raw_key == key).then(|| raw_value.trim().to_string())
    })
}
