use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::admin::ports::{AdminEnvPersister, AdminPortError};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub erp_url: String,
    pub erp_api_key: String,
    pub erp_api_secret: String,
    pub default_target_warehouse: String,
    pub erp_timeout: Duration,
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
    pub direct_read_enabled: bool,
    pub direct_site_config_path: String,
    pub direct_db_host: String,
    pub direct_db_port: Option<u16>,
    pub direct_db_user: String,
    pub direct_db_password: String,
    pub direct_db_name: String,
    pub catalog_cache_enabled: bool,
    pub catalog_cache_fallback_direct_db: bool,
    pub catalog_cache_path: PathBuf,
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
        let erp_timeout_seconds = std::env::var("ERP_TIMEOUT_SECONDS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
            .unwrap_or(15);
        let admin_supplier_path = std::env::var("MOBILE_API_ADMIN_SUPPLIER_STORE_PATH")
            .unwrap_or_else(|_| "data/mobile_admin_suppliers.json".to_string());
        let direct_db_port = std::env::var("ERP_DIRECT_DB_PORT")
            .ok()
            .and_then(|raw| raw.trim().parse::<u16>().ok())
            .filter(|port| *port > 0);
        let catalog_cache = catalog_cache_config_from(&|key| std::env::var(key).ok());

        Ok(Self {
            bind_addr: parse_bind_addr(&addr)?,
            erp_url: env_or("ERP_URL", ""),
            erp_api_key: env_or("ERP_API_KEY", ""),
            erp_api_secret: env_or("ERP_API_SECRET", ""),
            default_target_warehouse: env_or("ERP_DEFAULT_TARGET_WAREHOUSE", ""),
            erp_timeout: Duration::from_secs(erp_timeout_seconds),
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
            direct_read_enabled: env_or("ERP_DIRECT_READ_ENABLED", "") == "1",
            direct_site_config_path: env_or("ERP_DIRECT_SITE_CONFIG_PATH", ""),
            direct_db_host: env_or("ERP_DIRECT_DB_HOST", ""),
            direct_db_port,
            direct_db_user: env_or("ERP_DIRECT_DB_USER", ""),
            direct_db_password: env_or("ERP_DIRECT_DB_PASSWORD", ""),
            direct_db_name: env_or("ERP_DIRECT_DB_NAME", ""),
            catalog_cache_enabled: catalog_cache.enabled,
            catalog_cache_fallback_direct_db: catalog_cache.fallback_direct_db,
            catalog_cache_path: catalog_cache.path,
        })
    }

    pub fn erp_configured(&self) -> bool {
        !self.erp_url.trim().is_empty()
            && !self.erp_api_key.trim().is_empty()
            && !self.erp_api_secret.trim().is_empty()
    }

    pub fn direct_db_config(&self) -> Result<Option<DirectDbConfig>, AppError> {
        if !self.direct_read_enabled {
            return Ok(None);
        }
        if self.direct_site_config_path.trim().is_empty() {
            return Err(AppError::InvalidConfig {
                key: "ERP_DIRECT_SITE_CONFIG_PATH",
                value: String::new(),
            });
        }

        let mut config = DirectDbConfig::from_site_config(&self.direct_site_config_path)?;
        if !self.direct_db_host.trim().is_empty() {
            config.host = self.direct_db_host.trim().to_string();
        }
        if let Some(port) = self.direct_db_port {
            config.port = port;
        }
        if !self.direct_db_user.trim().is_empty() {
            config.user = self.direct_db_user.trim().to_string();
        }
        if !self.direct_db_password.trim().is_empty() {
            config.password = self.direct_db_password.trim().to_string();
        }
        if !self.direct_db_name.trim().is_empty() {
            config.name = self.direct_db_name.trim().to_string();
        }
        config.default_warehouse = self.default_target_warehouse.trim().to_string();
        Ok(Some(config))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectDbConfig {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub user: String,
    pub password: String,
    pub encryption_key: String,
    pub default_warehouse: String,
    pub pool: DirectDbPoolConfig,
}

impl DirectDbConfig {
    fn from_site_config(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let raw = std::fs::read_to_string(path).map_err(AppError::Io)?;
        let site: SiteConfig = serde_json::from_str(&raw).map_err(AppError::Json)?;
        if !site.db_type.trim().is_empty() && !site.db_type.trim().eq_ignore_ascii_case("mariadb") {
            return Err(AppError::InvalidConfig {
                key: "db_type",
                value: site.db_type,
            });
        }
        let name = site.db_name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::InvalidConfig {
                key: "db_name",
                value: String::new(),
            });
        }

        Ok(Self {
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: name.clone(),
            name,
            password: site.db_password.trim().to_string(),
            encryption_key: site.encryption_key.trim().to_string(),
            default_warehouse: String::new(),
            pool: DirectDbPoolConfig::from_env(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectDbPoolConfig {
    pub min_connections: u32,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
}

impl DirectDbPoolConfig {
    fn from_env() -> Self {
        Self::from_env_with(system_resources(), |key| std::env::var(key).ok())
    }

    fn from_env_with(resources: SystemResources, get_env: impl Fn(&str) -> Option<String>) -> Self {
        let defaults = Self::auto_defaults(resources);
        let max_connections = get_env_u32(&get_env, "ERP_DIRECT_DB_MAX_CONNECTIONS")
            .filter(|value| *value > 0)
            .unwrap_or(defaults.max_connections);
        let min_connections = get_env_u32(&get_env, "ERP_DIRECT_DB_MIN_CONNECTIONS")
            .filter(|value| *value > 0)
            .unwrap_or(defaults.min_connections)
            .min(max_connections);
        let acquire_timeout = get_env_duration_ms(&get_env, "ERP_DIRECT_DB_ACQUIRE_TIMEOUT_MS")
            .unwrap_or(defaults.acquire_timeout);
        let idle_timeout = get_env_duration_seconds(&get_env, "ERP_DIRECT_DB_IDLE_TIMEOUT_SECONDS")
            .unwrap_or(defaults.idle_timeout);

        Self {
            min_connections,
            max_connections,
            acquire_timeout,
            idle_timeout,
        }
    }

    fn auto_defaults(resources: SystemResources) -> Self {
        let cpu_target = (resources.cpu_count as u32).saturating_mul(2).clamp(4, 32);
        let memory_target = resources
            .total_memory_bytes
            .map(|bytes| (bytes / (512 * 1024 * 1024)) as u32)
            .unwrap_or(16)
            .clamp(4, 64);
        let max_connections = cpu_target.min(memory_target).max(4);
        let min_connections = (max_connections / 4).clamp(1, 4);

        Self {
            min_connections,
            max_connections,
            acquire_timeout: Duration::from_millis(500),
            idle_timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SystemResources {
    cpu_count: usize,
    total_memory_bytes: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct SiteConfig {
    #[serde(default)]
    db_name: String,
    #[serde(default)]
    db_password: String,
    #[serde(default)]
    db_type: String,
    #[serde(default)]
    encryption_key: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogCacheEnvConfig {
    enabled: bool,
    fallback_direct_db: bool,
    path: PathBuf,
}

fn catalog_cache_config_from(get_env: &impl Fn(&str) -> Option<String>) -> CatalogCacheEnvConfig {
    CatalogCacheEnvConfig {
        enabled: env_or_from(get_env, "ERP_CATALOG_CACHE_ENABLED", "") == "1",
        fallback_direct_db: env_or_from(get_env, "ERP_CATALOG_CACHE_FALLBACK_DIRECT_DB", "1")
            != "0",
        path: PathBuf::from(env_or_from(
            get_env,
            "ERP_CATALOG_CACHE_PATH",
            "data/catalog_cache.sqlite",
        )),
    }
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn env_or_from(get_env: &impl Fn(&str) -> Option<String>, key: &str, fallback: &str) -> String {
    get_env(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn get_env_u32(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<u32> {
    get_env(key).and_then(|raw| raw.trim().parse::<u32>().ok())
}

fn get_env_duration_ms(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<Duration> {
    get_env_u32(get_env, key)
        .filter(|value| *value > 0)
        .map(|value| Duration::from_millis(u64::from(value)))
}

fn get_env_duration_seconds(
    get_env: &impl Fn(&str) -> Option<String>,
    key: &str,
) -> Option<Duration> {
    get_env_u32(get_env, key)
        .filter(|value| *value > 0)
        .map(|value| Duration::from_secs(u64::from(value)))
}

fn system_resources() -> SystemResources {
    SystemResources {
        cpu_count: std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4),
        total_memory_bytes: detect_total_memory_bytes(),
    }
}

fn detect_total_memory_bytes() -> Option<u64> {
    detect_proc_meminfo_bytes().or_else(detect_sysctl_memsize_bytes)
}

fn detect_proc_meminfo_bytes() -> Option<u64> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|raw| parse_proc_meminfo_bytes(&raw))
}

fn parse_proc_meminfo_bytes(raw: &str) -> Option<u64> {
    raw.lines().find_map(|line| {
        let rest = line.strip_prefix("MemTotal:")?.trim();
        let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        kb.checked_mul(1024)
    })
}

fn detect_sysctl_memsize_bytes() -> Option<u64> {
    let output = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|bytes| *bytes > 0)
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
