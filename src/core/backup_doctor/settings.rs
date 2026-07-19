use std::path::PathBuf;

pub(super) fn backup_directory() -> PathBuf {
    if let Some(path) = non_empty_env("MINI_ERP_BACKUP_DIR").map(PathBuf::from) {
        return path;
    }
    first_existing_backup_directory([
        PathBuf::from("backups/mini_rs_erp_db"),
        PathBuf::from("../backups/mini_rs_erp_db"),
    ])
}

pub(super) fn backup_script_path() -> PathBuf {
    if let Some(path) = non_empty_env("MINI_ERP_BACKUP_SCRIPT").map(PathBuf::from) {
        return path;
    }
    [
        PathBuf::from("tools/db/backup_postgres.sh"),
        PathBuf::from("../mini_rs_erp/tools/db/backup_postgres.sh"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(|| PathBuf::from("tools/db/backup_postgres.sh"))
}

pub fn first_existing_backup_directory<const N: usize>(candidates: [PathBuf; N]) -> PathBuf {
    let fallback = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("backups/mini_rs_erp_db"));
    candidates
        .into_iter()
        .find(|candidate| candidate.is_dir())
        .unwrap_or(fallback)
}

pub(super) fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn bool_env(key: &str, fallback: bool) -> bool {
    non_empty_env(key)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(fallback)
}

pub(super) fn int_env(key: &str, fallback: i64) -> i64 {
    non_empty_env(key)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(fallback)
}

pub(super) fn uint_env(key: &str, fallback: u64) -> u64 {
    non_empty_env(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

pub(super) fn parse_clock(value: &str) -> Option<(u8, u8)> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour = hour.parse::<u8>().ok()?;
    let minute = minute.parse::<u8>().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}
