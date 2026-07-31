use super::*;
use crate::core::backup_doctor::BackupDoctorError;
use axum::body::Body;
use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use http_body_util::BodyExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};

const MAX_BACKUP_IMPORT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const BACKUP_IMPORT_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

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

pub async fn system_backup_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut body: Body,
) -> Result<Response, AdminError> {
    let principal = authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_BACKUP_IMPORT_BYTES)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(AdminErrorResponse::new("backup_import_too_large")),
        ));
    }
    let requested_by = if principal.display_name.trim().is_empty() {
        principal.ref_
    } else {
        principal.display_name
    };
    let filename = headers
        .get("x-backup-filename")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("mini_rs_erp.dump");
    let upload = state
        .backup_doctor
        .prepare_import(requested_by, filename)
        .map_err(backup_error)?;
    let upload_id = upload.job.id.clone();
    let mut file = match tokio::fs::File::create(&upload.path).await {
        Ok(file) => file,
        Err(_) => {
            state
                .backup_doctor
                .abort_import(&upload_id, "backup upload fayli yaratilmadi");
            return Err(server_error("backup_import_upload_failed"));
        }
    };
    let mut received = 0_u64;
    loop {
        let frame = match timeout(BACKUP_IMPORT_IDLE_TIMEOUT, body.frame()).await {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(_) => {
                state
                    .backup_doctor
                    .abort_import(&upload_id, "backup upload oqimi vaqtida uzildi");
                return Err(server_error("backup_import_upload_timeout"));
            }
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(_) => {
                state
                    .backup_doctor
                    .abort_import(&upload_id, "backup upload oqimi uzildi");
                return Err(server_error("backup_import_upload_failed"));
            }
        };
        let Ok(data) = frame.into_data() else {
            continue;
        };
        received = received.saturating_add(data.len() as u64);
        if received > MAX_BACKUP_IMPORT_BYTES {
            state
                .backup_doctor
                .abort_import(&upload_id, "backup fayli hajmi limitdan oshdi");
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(AdminErrorResponse::new("backup_import_too_large")),
            ));
        }
        if file.write_all(&data).await.is_err() {
            state
                .backup_doctor
                .abort_import(&upload_id, "backup upload fayliga yozilmadi");
            return Err(server_error("backup_import_upload_failed"));
        }
    }
    if file.flush().await.is_err() || file.sync_all().await.is_err() {
        state
            .backup_doctor
            .abort_import(&upload_id, "backup upload fayli yopilmadi");
        return Err(server_error("backup_import_upload_failed"));
    }
    if received == 0 {
        state
            .backup_doctor
            .abort_import(&upload_id, "backup fayli bo‘sh");
        return Err(bad_request("backup_import_invalid"));
    }
    drop(file);
    let job = match state.backup_doctor.complete_import(upload) {
        Ok(job) => job,
        Err(error) => {
            state
                .backup_doctor
                .abort_import(&upload_id, "backup import jobi boshlanmadi");
            return Err(backup_error(error));
        }
    };
    Ok((StatusCode::ACCEPTED, Json(job)).into_response())
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
        BackupDoctorError::InvalidImport => bad_request("backup_import_invalid"),
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
