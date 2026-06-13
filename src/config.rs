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
    pub admin_supplier_store_path: PathBuf,
    pub session_ttl_seconds: Option<u64>,
    pub supplier_prefix: String,
    pub werka_prefix: String,
    pub werka_code: String,
    pub werka_name: String,
    pub werka_phone: String,
    pub admin_phone: String,
    pub admin_name: String,
    pub admin_code: String,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let addr = std::env::var("MOBILE_API_ADDR").unwrap_or_else(|_| ":8081".to_string());
        let session_path = std::env::var("MOBILE_API_SESSION_STORE_PATH")
            .or_else(|_| std::env::var("MOBILE_API_SESSION_STORE"))
            .unwrap_or_else(|_| "data/mobile_sessions.json".to_string());
        let profile_path = std::env::var("MOBILE_API_PROFILE_STORE_PATH")
            .unwrap_or_else(|_| "data/mobile_profile_prefs.json".to_string());
        let push_token_path = std::env::var("MOBILE_API_PUSH_TOKEN_STORE_PATH")
            .unwrap_or_else(|_| "data/mobile_push_tokens.json".to_string());
        let ttl_hours = std::env::var("MOBILE_API_SESSION_TTL_HOURS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(24 * 30);
        let http_timeout_seconds = std::env::var("MINI_ERP_HTTP_TIMEOUT_SECONDS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
            .unwrap_or(15);
        let admin_supplier_path = std::env::var("MOBILE_API_ADMIN_SUPPLIER_STORE_PATH")
            .unwrap_or_else(|_| "data/mobile_admin_suppliers.json".to_string());

        Ok(Self {
            bind_addr: parse_bind_addr(&addr)?,
            default_target_warehouse: env_or("MINI_ERP_DEFAULT_TARGET_WAREHOUSE", ""),
            http_timeout: Duration::from_secs(http_timeout_seconds),
            session_store_path: PathBuf::from(session_path),
            profile_store_path: PathBuf::from(profile_path),
            push_token_store_path: PathBuf::from(push_token_path),
            admin_supplier_store_path: PathBuf::from(admin_supplier_path),
            session_ttl_seconds: Some(Duration::from_secs(ttl_hours * 60 * 60).as_secs()),
            supplier_prefix: env_or("MOBILE_DEV_SUPPLIER_PREFIX", "10"),
            werka_prefix: env_or("MOBILE_DEV_WERKA_PREFIX", "20"),
            werka_code: env_or("MOBILE_DEV_WERKA_CODE", ""),
            werka_name: env_or("MOBILE_DEV_WERKA_NAME", "Werka"),
            werka_phone: env_or("WERKA_PHONE", "+99888862440"),
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

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
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
