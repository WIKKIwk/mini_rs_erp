use super::*;
use crate::core::backup_doctor::BackupDoctorError;
use crate::db::postgres_order_reset::OrderResetError;
use tokio::time::{Duration, Instant, sleep};

const ORDER_RESET_CONFIRMATION: &str = "RESET ORDERS";
const ORDER_RESET_BACKUP_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60 + 30);

#[derive(Debug, thiserror::Error)]
enum OrderResetBackupError {
    #[error(transparent)]
    Backup(#[from] BackupDoctorError),
    #[error("backup failed: {0}")]
    Failed(String),
    #[error("backup timed out")]
    TimedOut,
}

#[derive(Debug, Deserialize)]
struct OrderResetRequest {
    #[serde(default)]
    confirmation: String,
}

pub async fn reset_orders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    let input: OrderResetRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid json"))?;
    if input.confirmation.trim() != ORDER_RESET_CONFIRMATION {
        return Err(bad_request("order_reset_confirmation_required"));
    }

    let requested_by = if principal.display_name.trim().is_empty() {
        principal.ref_.clone()
    } else {
        principal.display_name.clone()
    };
    let backup = verified_order_reset_backup(&state.backup_doctor, requested_by)
        .await
        .map_err(backup_error)?;
    let store = state
        .order_reset
        .as_ref()
        .ok_or_else(|| server_error("order_reset_unavailable"))?;
    let result = store.reset_all_orders().await.map_err(order_reset_error)?;
    state.production_maps.notify_live();

    Ok(json_response(serde_json::json!({
        "ok": true,
        "scope": "orders",
        "backup": {
            "id": backup.id,
            "verified": backup.verified,
            "checksum_sha256": backup.checksum_sha256,
            "size_bytes": backup.size_bytes,
        },
        "result": result,
    })))
}

async fn verified_order_reset_backup(
    doctor: &crate::core::backup_doctor::BackupDoctor,
    requested_by: String,
) -> Result<crate::core::admin::models::AdminServerMonitorBackupSnapshot, OrderResetBackupError> {
    let job = doctor.start_manual_backup(requested_by)?;
    let deadline = Instant::now() + ORDER_RESET_BACKUP_TIMEOUT;

    loop {
        let snapshot = doctor
            .report(time::OffsetDateTime::now_utc())
            .snapshots
            .into_iter()
            .find(|snapshot| snapshot.id == job.id);
        match snapshot {
            Some(snapshot) if snapshot.status == "ready" && snapshot.verified => {
                return Ok(snapshot);
            }
            Some(snapshot) if snapshot.status == "failed" => {
                return Err(OrderResetBackupError::Failed(snapshot.error));
            }
            _ if Instant::now() >= deadline => return Err(OrderResetBackupError::TimedOut),
            _ => sleep(Duration::from_millis(250)).await,
        }
    }
}

fn backup_error(error: OrderResetBackupError) -> AdminError {
    match error {
        OrderResetBackupError::Backup(
            BackupDoctorError::NotConfigured | BackupDoctorError::EngineUnavailable,
        ) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AdminErrorResponse::new("backup_service_unavailable")),
        ),
        OrderResetBackupError::Backup(BackupDoctorError::AlreadyRunning) => (
            StatusCode::CONFLICT,
            Json(AdminErrorResponse::new("backup_already_running")),
        ),
        OrderResetBackupError::Failed(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AdminErrorResponse::new("backup_failed")),
        ),
        OrderResetBackupError::TimedOut => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AdminErrorResponse::new("backup_timed_out")),
        ),
        OrderResetBackupError::Backup(
            BackupDoctorError::RuntimeUnavailable | BackupDoctorError::Storage,
        ) => server_error("backup_service_failed"),
        OrderResetBackupError::Backup(
            BackupDoctorError::NotFound
            | BackupDoctorError::NotReady
            | BackupDoctorError::InvalidImport,
        ) => server_error("backup_service_failed"),
    }
}

fn order_reset_error(error: OrderResetError) -> AdminError {
    match error {
        OrderResetError::VerificationFailed => server_error("order_reset_verification_failed"),
        OrderResetError::StoreFailed(_) => server_error("order_reset_failed"),
    }
}
