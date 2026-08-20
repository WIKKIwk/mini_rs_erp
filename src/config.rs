use std::env::VarError;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::admin::ports::{AdminEnvPersister, AdminPortError};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub default_target_warehouse: String,
    pub http_timeout: Duration,
    pub session_store_path: PathBuf,
    pub profile_store_path: PathBuf,
    pub push_token_store_path: PathBuf,
    pub session_ttl_seconds: Option<u64>,
    pub supplier_prefix: String,
    pub werka_prefix: String,
    pub werka_code: String,
    pub werka_name: String,
    pub werka_phone: String,
    pub material_taminotchi_code: String,
    pub material_taminotchi_name: String,
    pub material_taminotchi_phone: String,
    pub admin_phone: String,
    pub admin_name: String,
    pub admin_code: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let addr = env_or_default("MOBILE_API_ADDR", ":8081")?;
        let session_path = env_path_with_legacy(
            "MOBILE_API_SESSION_STORE_PATH",
            Some("MOBILE_API_SESSION_STORE"),
            "data/mobile_sessions.json",
        )?;
        let profile_path = env_path_with_legacy(
            "MOBILE_API_PROFILE_STORE_PATH",
            None,
            "data/mobile_profile_prefs.json",
        )?;
        let push_token_path = env_path_with_legacy(
            "MOBILE_API_PUSH_TOKEN_STORE_PATH",
            None,
            "data/mobile_push_tokens.json",
        )?;
        let ttl_hours = positive_env_u64("MOBILE_API_SESSION_TTL_HOURS", 24 * 30)?;
        let http_timeout_seconds = positive_env_u64("MINI_ERP_HTTP_TIMEOUT_SECONDS", 15)?;
        let session_ttl_seconds = ttl_hours
            .checked_mul(60 * 60)
            .ok_or_else(|| invalid_config("MOBILE_API_SESSION_TTL_HOURS", "value is too large"))?;
        validate_runtime_env()?;
        Ok(Self {
            bind_addr: parse_bind_addr(&addr)?,
            default_target_warehouse: env_or("MINI_ERP_DEFAULT_TARGET_WAREHOUSE", "")?,
            http_timeout: Duration::from_secs(http_timeout_seconds),
            session_store_path: PathBuf::from(session_path),
            profile_store_path: PathBuf::from(profile_path),
            push_token_store_path: PathBuf::from(push_token_path),
            session_ttl_seconds: Some(session_ttl_seconds),
            supplier_prefix: env_or("MOBILE_DEV_SUPPLIER_PREFIX", "10")?,
            werka_prefix: env_or("MOBILE_DEV_WERKA_PREFIX", "20")?,
            werka_code: env_or("MOBILE_DEV_WERKA_CODE", "")?,
            werka_name: env_or("MOBILE_DEV_WERKA_NAME", "Werka")?,
            werka_phone: env_or("WERKA_PHONE", "+99888862440")?,
            material_taminotchi_code: env_or("MOBILE_DEV_MATERIAL_TAMINOTCHI_CODE", "")?,
            material_taminotchi_name: env_or(
                "MOBILE_DEV_MATERIAL_TAMINOTCHI_NAME",
                "Material taminotchisi",
            )?,
            material_taminotchi_phone: env_or("MOBILE_DEV_MATERIAL_TAMINOTCHI_PHONE", "")?,
            admin_phone: "+998880000000".to_string(),
            admin_name: "Admin".to_string(),
            admin_code: "19621978".to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct DotEnvPersister {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl DotEnvPersister {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let path = if path.as_os_str().is_empty() {
            PathBuf::from(".env")
        } else {
            path
        };
        Self {
            path,
            lock: Arc::new(Mutex::new(())),
        }
    }
}

impl AdminEnvPersister for DotEnvPersister {
    fn upsert(
        &self,
        values: std::collections::BTreeMap<&'static str, String>,
    ) -> Result<(), AdminPortError> {
        let _guard = self.lock.lock().map_err(|_| AdminPortError::LookupFailed)?;
        let mut current = std::collections::BTreeMap::new();
        if self.path.exists() {
            let iter =
                dotenvy::from_path_iter(&self.path).map_err(|_| AdminPortError::LookupFailed)?;
            for item in iter {
                let (key, value) = item.map_err(|_| AdminPortError::LookupFailed)?;
                current.insert(key, value);
            }
        }
        for (key, value) in values {
            let key = key.trim();
            if !key.is_empty() {
                current.insert(key.to_string(), value.trim().to_string());
            }
        }
        let mut body = String::new();
        for (key, value) in current {
            body.push_str(&key);
            body.push('=');
            body.push_str(&dotenv_value(&value));
            body.push('\n');
        }
        std::fs::write(&self.path, body).map_err(|_| AdminPortError::LookupFailed)
    }
}

fn dotenv_value(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '+'))
    {
        return value.to_string();
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

fn env_or(key: &'static str, fallback: &str) -> Result<String, AppError> {
    Ok(read_env(key)?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string()))
}

fn read_env(key: &'static str) -> Result<Option<String>, AppError> {
    match std::env::var(key) {
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(invalid_config(key, "value is not valid UTF-8")),
    }
}

fn env_or_default(key: &'static str, fallback: &str) -> Result<String, AppError> {
    match read_env(key)? {
        Some(value) if value.is_empty() => Err(invalid_config(key, "value must not be empty")),
        Some(value) => Ok(value),
        None => Ok(fallback.to_string()),
    }
}

fn env_path_with_legacy(
    key: &'static str,
    legacy_key: Option<&'static str>,
    fallback: &str,
) -> Result<String, AppError> {
    if let Some(value) = read_env(key)? {
        return if value.is_empty() {
            Err(invalid_config(key, "path must not be empty"))
        } else {
            Ok(value)
        };
    }
    if let Some(legacy_key) = legacy_key {
        if let Some(value) = read_env(legacy_key)? {
            return if value.is_empty() {
                Err(invalid_config(legacy_key, "path must not be empty"))
            } else {
                Ok(value)
            };
        }
    }
    Ok(fallback.to_string())
}

fn positive_env_u64(key: &'static str, fallback: u64) -> Result<u64, AppError> {
    match read_env(key)? {
        Some(raw) => positive_u64_from_raw(key, &raw),
        None => Ok(fallback),
    }
}

fn positive_u64_from_raw(key: &'static str, raw: &str) -> Result<u64, AppError> {
    let value = raw
        .trim()
        .parse::<u64>()
        .map_err(|_| invalid_config(key, "must be a positive integer"))?;
    if value == 0 {
        return Err(invalid_config(key, "must be greater than zero"));
    }
    Ok(value)
}

fn validate_runtime_env() -> Result<(), AppError> {
    for key in [
        "MOBILE_API_SESSION_STORE_BACKEND",
        "MOBILE_API_PROFILE_STORE_BACKEND",
        "MOBILE_API_PUSH_TOKEN_STORE_BACKEND",
        "MOBILE_API_ADMIN_SUPPLIER_STORE_BACKEND",
    ] {
        validate_backend(key)?;
    }

    if let Some(value) = read_env("MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK")?
        && !matches!(value.as_str(), "0" | "1")
    {
        return Err(invalid_config(
            "MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK",
            "must be 0 or 1",
        ));
    }

    for key in [
        "MOBILE_API_SESSION_LMDB_PATH",
        "MOBILE_API_PROFILE_LMDB_PATH",
        "MOBILE_API_PUSH_TOKEN_LMDB_PATH",
        "MOBILE_API_ADMIN_SUPPLIER_LMDB_PATH",
        "MOBILE_API_RPS_BATCH_LMDB_PATH",
        "MOBILE_API_ROLE_STORE_PATH",
        "MOBILE_API_TELEGRAM_STORE_PATH",
        "MOBILE_API_ADMIN_STORE_PATH",
        "MOBILE_API_APPARATUS_GROUP_STORE_PATH",
        "MOBILE_API_CALCULATE_ORDER_STORE_PATH",
        "MOBILE_API_CALCULATE_MATERIAL_STORE_PATH",
        "MOBILE_API_CALCULATE_ORDER_IMAGE_DIR",
    ] {
        validate_nonempty_path(key)?;
    }

    for key in [
        "MOBILE_API_SESSION_LMDB_MAP_SIZE_MB",
        "MOBILE_API_PROFILE_LMDB_MAP_SIZE_MB",
        "MOBILE_API_PUSH_TOKEN_LMDB_MAP_SIZE_MB",
        "MOBILE_API_ADMIN_SUPPLIER_LMDB_MAP_SIZE_MB",
        "MOBILE_API_RPS_BATCH_LMDB_MAP_SIZE_MB",
    ] {
        validate_positive_map_size(key)?;
    }
    validate_positive_usize("MOBILE_API_LISTENER_COUNT")?;
    Ok(())
}

fn validate_backend(key: &'static str) -> Result<(), AppError> {
    let Some(value) = read_env(key)? else {
        return Ok(());
    };
    validate_backend_value(key, &value)
}

fn validate_backend_value(key: &'static str, value: &str) -> Result<(), AppError> {
    if !matches!(value.trim().to_ascii_lowercase().as_str(), "lmdb" | "json") {
        return Err(invalid_config(key, "must be lmdb or json"));
    }
    Ok(())
}

fn validate_nonempty_path(key: &'static str) -> Result<(), AppError> {
    if read_env(key)?.is_some_and(|value| value.is_empty()) {
        return Err(invalid_config(key, "path must not be empty"));
    }
    Ok(())
}

fn validate_positive_usize(key: &'static str) -> Result<(), AppError> {
    let Some(raw) = read_env(key)? else {
        return Ok(());
    };
    validate_positive_usize_raw(key, &raw)
}

fn validate_positive_usize_raw(key: &'static str, raw: &str) -> Result<(), AppError> {
    let value = raw
        .trim()
        .parse::<usize>()
        .map_err(|_| invalid_config(key, "must be a positive integer"))?;
    if value == 0 {
        return Err(invalid_config(key, "must be greater than zero"));
    }
    Ok(())
}

fn validate_positive_map_size(key: &'static str) -> Result<(), AppError> {
    let Some(raw) = read_env(key)? else {
        return Ok(());
    };
    let value = raw
        .trim()
        .parse::<usize>()
        .map_err(|_| invalid_config(key, "must be a positive integer"))?;
    if value == 0 || value.checked_mul(1024 * 1024).is_none() {
        return Err(invalid_config(key, "must be a positive value within size limits"));
    }
    Ok(())
}

fn invalid_config(key: &'static str, value: &str) -> AppError {
    AppError::InvalidConfig {
        key,
        value: value.to_string(),
    }
}

fn parse_bind_addr(raw: &str) -> Result<SocketAddr, AppError> {
    let trimmed = raw.trim();
    let normalized = if trimmed.starts_with(':') {
        format!("0.0.0.0{trimmed}")
    } else {
        trimmed.to_string()
    };

    normalized.parse().map_err(|_| AppError::InvalidConfig {
        key: "MOBILE_API_ADDR",
        value: raw.to_string(),
    })
}

#[cfg(test)]
mod tests;
