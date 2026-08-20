use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::Response;

use crate::app::AppState;
use crate::core::apparatus_standard::aasx::{
    AasxExportError, AasxImportError, export_aasx, import_aasx,
};
use crate::core::apparatus_standard::{AASX_MEDIA_TYPE, ApparatusId};

use super::*;

/// The bounded AASX importer has the same 16 MiB package budget. The route
/// applies this limit before `Bytes` is materialized, while the importer keeps
/// its own independent package and part budgets for direct callers.
pub const MAX_AASX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

pub async fn apparatus_aasx(
    State(state): State<AppState>,
    Path(id): Path<String>,
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
    require_capability(&state, &principal, Capability::ProductionMapManage).await?;

    let apparatus_id = ApparatusId::new(id).map_err(|_| bad_request("apparatus_id_invalid"))?;
    match method {
        Method::GET => export_apparatus(&state, &apparatus_id).await,
        Method::POST => import_apparatus(&state, &apparatus_id, &body).await,
        _ => Err(method_not_allowed()),
    }
}

async fn export_apparatus(
    state: &AppState,
    apparatus_id: &ApparatusId,
) -> Result<Response, AdminError> {
    let canonical = state
        .apparatus_groups
        .canonical_apparatus_by_id(apparatus_id)
        .await
        .map_err(|error| {
            tracing::error!(
                %error,
                apparatus_id = %apparatus_id.as_str(),
                "AASX export canonical lookup failed"
            );
            super::apparatus::apparatus_group_error(error)
        })?
        .ok_or_else(|| not_found("apparatus_not_found"))?;
    let package = export_aasx(&canonical).map_err(aasx_export_error)?;
    let filename = download_filename(apparatus_id);
    let content_length = package.len().to_string();

    let mut response = Response::new(Body::from(package));
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
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn import_apparatus(
    state: &AppState,
    apparatus_id: &ApparatusId,
    package: &[u8],
) -> Result<Response, AdminError> {
    if package.len() > MAX_AASX_UPLOAD_BYTES {
        return Err(aasx_payload_too_large());
    }
    let imported = import_aasx(package).map_err(aasx_import_error)?;
    if imported.identity.id != *apparatus_id {
        return Err(conflict("aasx_identity_conflict"));
    }

    let existing = state
        .apparatus_groups
        .canonical_apparatus_by_id(apparatus_id)
        .await
        .map_err(|error| {
            tracing::error!(
                %error,
                apparatus_id = %apparatus_id.as_str(),
                "AASX import canonical lookup failed"
            );
            super::apparatus::apparatus_group_error(error)
        })?
        .ok_or_else(|| not_found("apparatus_not_found"))?;
    if imported.versioning.revision != existing.versioning.revision {
        return Err(conflict("aasx_revision_conflict"));
    }

    let imported_revision = imported.versioning.revision;
    let display = imported.identity.display;
    let classification = imported.classification;
    let capabilities = imported.capabilities;
    let capability_profiles = imported.capability_profiles;
    let policies = imported.policies;
    let capacity = imported.capacity;
    let placement = imported.placement;
    let training = imported.training;
    let provenance = imported.provenance;
    let aas = imported.aas;
    let updated = state
        .apparatus_groups
        .mutate_canonical_apparatus(apparatus_id, imported_revision, |canonical| {
            // The service owns identity and revision mutation. Only the
            // imported canonical configuration is copied into that protected
            // record, so an AASX package cannot rename or re-key the asset.
            canonical.identity.display = display;
            canonical.classification = classification;
            canonical.capabilities = capabilities;
            canonical.capability_profiles = capability_profiles;
            canonical.policies = policies;
            canonical.capacity = capacity;
            canonical.placement = placement;
            canonical.training = training;
            canonical.provenance = provenance;
            canonical.aas = aas;
            Ok(())
        })
        .await
        .map_err(|error| {
            tracing::error!(
                %error,
                apparatus_id = %apparatus_id.as_str(),
                "AASX canonical mutation failed"
            );
            match error {
                crate::core::apparatus_groups::ApparatusGroupError::Conflict => {
                    conflict("aasx_revision_conflict")
                }
                error => super::apparatus::apparatus_group_error(error),
            }
        })?;
    state.production_maps.notify_live();
    Ok(json_response(updated))
}

fn download_filename(apparatus_id: &ApparatusId) -> String {
    let safe_id = apparatus_id
        .as_str()
        .chars()
        .map(|character| if character == ':' { '_' } else { character })
        .collect::<String>();
    format!("apparatus-{safe_id}.aasx")
}

fn aasx_export_error(error: AasxExportError) -> AdminError {
    tracing::error!(%error, "AASX export failed at admin boundary");
    server_error("aasx_export_failed")
}

fn aasx_import_error(error: AasxImportError) -> AdminError {
    tracing::warn!(%error, "AASX package rejected at admin boundary");
    bad_request("aasx_import_invalid")
}

fn aasx_payload_too_large() -> AdminError {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(AdminErrorResponse::new("aasx_package_too_large")),
    )
}
