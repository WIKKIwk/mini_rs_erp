use super::*;
use crate::core::backup_doctor::BackupDoctorError;
use axum::body::Body;
use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use tokio::io::AsyncReadExt;

pub async fn system_backup_create(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    let requested_by = if principal.display_name.trim().is_empty() {
        principal.ref_
    } else {
        principal.display_name
    };
    let job = state
        .backup_doctor
        .start_manual_backup(requested_by)
        .map_err(backup_error)?;
    Ok((StatusCode::ACCEPTED, Json(job)).into_response())
}

pub async fn system_backup_download(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    let artifact = state.backup_doctor.artifact(&id).map_err(backup_error)?;
    let mut file = tokio::fs::File::open(&artifact.path)
        .await
        .map_err(|_| server_error("backup_download_open_failed"))?;
    let stream = async_stream::stream! {
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            match file.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => {
                    yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buffer[..read]));
                }
                Err(error) => {
                    yield Err(error);
                    break;
                }
            }
        }
    };
    let filename = safe_download_name(&artifact.filename);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| server_error("backup_download_filename_failed"))?,
    );
    response.headers_mut().insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&artifact.size_bytes.to_string())
            .map_err(|_| server_error("backup_download_size_failed"))?,
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn backup_error(error: BackupDoctorError) -> AdminError {
    match error {
        BackupDoctorError::NotConfigured | BackupDoctorError::EngineUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AdminErrorResponse::new("backup_service_unavailable")),
        ),
        BackupDoctorError::AlreadyRunning => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::new("backup_already_running")),
        ),
        BackupDoctorError::NotFound => not_found("backup_not_found"),
        BackupDoctorError::NotReady => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::new("backup_not_ready")),
        ),
        BackupDoctorError::Storage | BackupDoctorError::RuntimeUnavailable => {
            server_error("backup_service_failed")
        }
    }
}

fn safe_download_name(value: &str) -> String {
    let name = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        .collect::<String>();
    if name.is_empty() {
        "mini_rs_erp.dump".to_string()
    } else {
        name
    }
}
