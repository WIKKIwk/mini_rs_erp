use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, header};
use axum::response::Response;

use crate::app::AppState;
use crate::core::apparatus_standard::{AASX_MEDIA_TYPE, ApparatusId};

use super::*;

pub const MAX_AASX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

pub async fn apparatus_aasx(
    State(state): State<AppState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = super::apparatus::authorize_apparatus(&state, &headers).await?;
    require_capability(&state, &principal, Capability::ProductionMapManage).await?;
    let apparatus_id = super::apparatus::parse_apparatus_id(id)?;
    match method {
        Method::GET => export_apparatus(&state, &apparatus_id).await,
        Method::POST => {
            if body.len() > MAX_AASX_UPLOAD_BYTES {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(AdminErrorResponse::new("aasx_package_too_large")),
                ));
            }
            let expected_revision = expected_revision(&headers)?;
            let committed = state
                .apparatus
                .replace_from_aasx(
                    apparatus_id,
                    expected_revision,
                    &body,
                    canonical_command_metadata(&principal, &headers)?,
                )
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(committed))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn export_apparatus(
    state: &AppState,
    apparatus_id: &ApparatusId,
) -> Result<Response, AdminError> {
    let stored = state
        .apparatus
        .current_aasx(apparatus_id)
        .await
        .map_err(canonical_apparatus_error)?
        .ok_or_else(|| not_found("apparatus_not_found"))?;
    let filename = download_filename(apparatus_id);
    let content_length = stored.artifact.bytes().len().to_string();
    let etag = format!(
        "\"{}-{}\"",
        stored.revision,
        stored.artifact.sha256().to_hex()
    );
    let mut response = Response::new(Body::from(stored.artifact.into_bytes()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(AASX_MEDIA_TYPE),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| server_error("aasx_download_filename_failed"))?,
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&content_length)
            .map_err(|_| server_error("aasx_download_size_failed"))?,
    );
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag).map_err(|_| server_error("aasx_etag_failed"))?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn expected_revision(headers: &HeaderMap) -> Result<u64, AdminError> {
    let value = headers
        .get(header::IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .ok_or_else(|| bad_request("expected_revision_required"))?;
    value
        .trim_matches('"')
        .parse::<u64>()
        .ok()
        .filter(|revision| *revision > 0)
        .ok_or_else(|| bad_request("expected_revision_invalid"))
}

fn download_filename(apparatus_id: &ApparatusId) -> String {
    let safe_id = apparatus_id.as_str().replace(':', "_");
    format!("apparatus-{safe_id}.aasx")
}
