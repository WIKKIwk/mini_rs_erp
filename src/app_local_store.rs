use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalStoreBackend {
    Lmdb,
    Json,
}

pub(super) fn local_store_backend(env_key: &'static str) -> LocalStoreBackend {
    let raw = std::env::var(env_key).ok();
    let backend = local_store_backend_from(raw.as_deref());
    if raw.is_some()
        && !matches!(
            raw.as_deref().map(|value| value.trim().to_lowercase()),
            Some(value) if value == "lmdb" || value == "json"
        )
    {
        tracing::warn!(
            env_key,
            value = %raw.as_deref().unwrap_or_default(),
            "unknown local store backend; using LMDB"
        );
    }
    backend
}

pub(super) fn local_store_backend_from(raw: Option<&str>) -> LocalStoreBackend {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("json") => LocalStoreBackend::Json,
        _ => LocalStoreBackend::Lmdb,
    }
}

pub(super) fn session_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_SESSION_LMDB_PATH",
        &config.session_store_path,
        "data/mobile_sessions.lmdb",
    )
}

pub(super) fn local_lmdb_map_size_bytes(env_key: &str) -> usize {
    std::env::var(env_key)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(64)
        * 1024
        * 1024
}

pub(super) fn build_profile_store(config: &AppConfig) -> Arc<dyn ProfileStorePort> {
    match local_store_backend("MOBILE_API_PROFILE_STORE_BACKEND") {
        LocalStoreBackend::Lmdb => {
            let lmdb_path = profile_lmdb_path(config);
            match LmdbProfileStore::open(
                lmdb_path.clone(),
                local_lmdb_map_size_bytes("MOBILE_API_PROFILE_LMDB_MAP_SIZE_MB"),
                Some(config.profile_store_path.clone()),
            ) {
                Ok(store) => {
                    tracing::info!(
                        path = %lmdb_path.display(),
                        legacy_json_path = %config.profile_store_path.display(),
                        "LMDB profile preference store enabled"
                    );
                    Arc::new(store)
                }
                Err(error) => {
                    if allow_json_fallback() {
                        tracing::warn!(
                            %error,
                            "LMDB profile preference store unavailable; falling back to JSON profile store"
                        );
                        Arc::new(ProfileStore::new(config.profile_store_path.clone()))
                    } else {
                        panic!("LMDB profile preference store unavailable: {error}");
                    }
                }
            }
        }
        LocalStoreBackend::Json => Arc::new(ProfileStore::new(config.profile_store_path.clone())),
    }
}

pub(super) fn profile_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_PROFILE_LMDB_PATH",
        &config.profile_store_path,
        "data/mobile_profile_prefs.lmdb",
    )
}

pub(super) fn product_map_store_path() -> std::path::PathBuf {
    std::env::var("MOBILE_API_PRODUCTION_MAP_STORE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("data/mobile_production_maps.sqlite"))
}

pub(super) fn apparatus_group_store_path() -> std::path::PathBuf {
    std::env::var("MOBILE_API_APPARATUS_GROUP_STORE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(test)]
            {
                test_sqlite_path("mobile_apparatus_groups")
            }
            #[cfg(not(test))]
            {
                std::path::PathBuf::from("data/mobile_apparatus_groups.sqlite")
            }
        })
}

pub(super) fn calculate_order_store_path() -> std::path::PathBuf {
    std::env::var("MOBILE_API_CALCULATE_ORDER_STORE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(test)]
            {
                test_sqlite_path("mobile_calculate_orders")
            }
            #[cfg(not(test))]
            {
                std::path::PathBuf::from("data/mobile_calculate_orders.sqlite")
            }
        })
}

pub(super) fn calculate_order_image_dir() -> std::path::PathBuf {
    std::env::var("MOBILE_API_CALCULATE_ORDER_IMAGE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            #[cfg(test)]
            {
                let path = test_sqlite_path("mobile_calculate_order_images");
                path.with_extension("images")
            }
            #[cfg(not(test))]
            {
                std::path::PathBuf::from("data/mobile_calculate_order_images")
            }
        })
}

pub(super) fn build_push_token_store(config: &AppConfig) -> Arc<dyn PushTokenStorePort> {
    match local_store_backend("MOBILE_API_PUSH_TOKEN_STORE_BACKEND") {
        LocalStoreBackend::Lmdb => {
            let lmdb_path = push_token_lmdb_path(config);
            match LmdbPushTokenStore::open(
                lmdb_path.clone(),
                local_lmdb_map_size_bytes("MOBILE_API_PUSH_TOKEN_LMDB_MAP_SIZE_MB"),
                Some(config.push_token_store_path.clone()),
            ) {
                Ok(store) => {
                    tracing::info!(
                        path = %lmdb_path.display(),
                        legacy_json_path = %config.push_token_store_path.display(),
                        "LMDB push token store enabled"
                    );
                    Arc::new(store)
                }
                Err(error) => {
                    if allow_json_fallback() {
                        tracing::warn!(
                            %error,
                            "LMDB push token store unavailable; falling back to JSON push token store"
                        );
                        Arc::new(PushTokenStore::new(config.push_token_store_path.clone()))
                    } else {
                        panic!("LMDB push token store unavailable: {error}");
                    }
                }
            }
        }
        LocalStoreBackend::Json => {
            Arc::new(PushTokenStore::new(config.push_token_store_path.clone()))
        }
    }
}

pub(super) fn push_token_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_PUSH_TOKEN_LMDB_PATH",
        &config.push_token_store_path,
        "data/mobile_push_tokens.lmdb",
    )
}

pub(super) fn build_admin_supplier_state_store(config: &AppConfig) -> AdminSupplierStateBackend {
    match local_store_backend("MOBILE_API_ADMIN_SUPPLIER_STORE_BACKEND") {
        LocalStoreBackend::Lmdb => {
            let lmdb_path = admin_supplier_lmdb_path(config);
            match AdminSupplierStateBackend::lmdb(
                lmdb_path.clone(),
                local_lmdb_map_size_bytes("MOBILE_API_ADMIN_SUPPLIER_LMDB_MAP_SIZE_MB"),
                Some(config.admin_supplier_store_path.clone()),
            ) {
                Ok(store) => {
                    tracing::info!(
                        path = %lmdb_path.display(),
                        legacy_json_path = %config.admin_supplier_store_path.display(),
                        "LMDB admin supplier state store enabled"
                    );
                    store
                }
                Err(error) => {
                    if allow_json_fallback() {
                        tracing::warn!(
                            %error,
                            "LMDB admin supplier state store unavailable; falling back to JSON admin supplier state store"
                        );
                        AdminSupplierStateBackend::json(config.admin_supplier_store_path.clone())
                    } else {
                        panic!("LMDB admin supplier state store unavailable: {error}");
                    }
                }
            }
        }
        LocalStoreBackend::Json => {
            AdminSupplierStateBackend::json(config.admin_supplier_store_path.clone())
        }
    }
}

pub(super) fn admin_supplier_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_ADMIN_SUPPLIER_LMDB_PATH",
        &config.admin_supplier_store_path,
        "data/mobile_admin_suppliers.lmdb",
    )
}

pub(super) fn build_rps_batch_store() -> RpsBatchLmdbStore {
    let lmdb_path = rps_batch_lmdb_path();
    match RpsBatchLmdbStore::open(
        lmdb_path.clone(),
        local_lmdb_map_size_bytes("MOBILE_API_RPS_BATCH_LMDB_MAP_SIZE_MB"),
    ) {
        Ok(store) => {
            tracing::info!(
                path = %lmdb_path.display(),
                "LMDB RPS batch store enabled"
            );
            store
        }
        Err(error) => panic!("LMDB RPS batch store unavailable: {error}"),
    }
}

pub(super) fn role_store_path() -> std::path::PathBuf {
    std::env::var("MOBILE_API_ROLE_STORE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("data/mobile_roles.json"))
}

pub(super) fn rps_batch_lmdb_path() -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_RPS_BATCH_LMDB_PATH",
        std::path::Path::new("data/mobile_rps_batches.json"),
        "data/mobile_rps_batches.lmdb",
    )
}

pub(super) fn lmdb_path(
    env_key: &str,
    legacy_json_path: &std::path::Path,
    hardcoded_default: &str,
) -> std::path::PathBuf {
    match std::env::var(env_key) {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            #[cfg(test)]
            {
                test_lmdb_path(legacy_json_path, hardcoded_default)
            }
            #[cfg(not(test))]
            {
                derive_lmdb_path(legacy_json_path, hardcoded_default)
            }
        }
    }
}

pub(super) fn derive_lmdb_path(
    legacy_json_path: &std::path::Path,
    hardcoded_default: &str,
) -> std::path::PathBuf {
    if legacy_json_path.as_os_str().is_empty() {
        return std::path::PathBuf::from(hardcoded_default);
    }
    legacy_json_path.with_extension("lmdb")
}

pub(super) fn allow_json_fallback() -> bool {
    std::env::var("MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK").is_ok_and(|raw| raw.trim() == "1")
}

#[cfg(test)]
pub(super) fn test_lmdb_path(
    legacy_json_path: &std::path::Path,
    hardcoded_default: &str,
) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stem = legacy_json_path
        .file_stem()
        .or_else(|| std::path::Path::new(hardcoded_default).file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("local-store");
    std::env::temp_dir().join(format!(
        "accord-mobile-server-rs-lmdb-test-{}-{count}-{stem}.lmdb",
        std::process::id()
    ))
}

#[cfg(test)]
pub(super) fn test_sqlite_path(stem: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "accord-mobile-server-rs-sqlite-test-{}-{count}-{stem}.sqlite",
        std::process::id()
    ))
}
