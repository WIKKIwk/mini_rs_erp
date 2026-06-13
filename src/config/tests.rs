use super::{
    DirectDbConfig, DirectDbPoolConfig, DotEnvPersister, SystemResources,
    catalog_cache_config_from, parse_bind_addr, parse_proc_meminfo_bytes,
};
use crate::core::admin::ports::AdminEnvPersister;
use std::time::Duration;

#[test]
fn parses_go_style_bind_addr() {
    let addr = parse_bind_addr(":8081").expect("addr");

    assert_eq!(addr.to_string(), "0.0.0.0:8081");
}

#[test]
fn direct_db_config_reads_frappe_site_config_like_go() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("site_config.json");
    std::fs::write(
        &path,
        r#"{"db_name":"_site1","db_password":"secret","db_type":"mariadb"}"#,
    )
    .expect("write config");

    let config = DirectDbConfig::from_site_config(path).expect("direct db config");

    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 3306);
    assert_eq!(config.name, "_site1");
    assert_eq!(config.user, "_site1");
    assert_eq!(config.password, "secret");
    assert_eq!(config.encryption_key, "");
    assert!(config.pool.max_connections >= config.pool.min_connections);
    assert!(config.pool.max_connections >= 4);
    assert_eq!(config.pool.acquire_timeout, Duration::from_millis(500));
    assert_eq!(config.pool.idle_timeout, Duration::from_secs(60));
}

#[test]
fn direct_db_pool_defaults_scale_from_cpu_and_memory() {
    let pool = DirectDbPoolConfig::auto_defaults(SystemResources {
        cpu_count: 8,
        total_memory_bytes: Some(16 * 1024 * 1024 * 1024),
    });

    assert_eq!(pool.max_connections, 16);
    assert_eq!(pool.min_connections, 4);
    assert_eq!(pool.acquire_timeout, Duration::from_millis(500));
    assert_eq!(pool.idle_timeout, Duration::from_secs(60));
}

#[test]
fn direct_db_pool_env_overrides_defaults_and_clamps_min() {
    let env = std::collections::BTreeMap::from([
        ("ERP_DIRECT_DB_MAX_CONNECTIONS", "12"),
        ("ERP_DIRECT_DB_MIN_CONNECTIONS", "20"),
        ("ERP_DIRECT_DB_ACQUIRE_TIMEOUT_MS", "900"),
        ("ERP_DIRECT_DB_IDLE_TIMEOUT_SECONDS", "30"),
    ]);
    let pool = DirectDbPoolConfig::from_env_with(
        SystemResources {
            cpu_count: 4,
            total_memory_bytes: Some(8 * 1024 * 1024 * 1024),
        },
        |key| env.get(key).map(|value| value.to_string()),
    );

    assert_eq!(pool.max_connections, 12);
    assert_eq!(pool.min_connections, 12);
    assert_eq!(pool.acquire_timeout, Duration::from_millis(900));
    assert_eq!(pool.idle_timeout, Duration::from_secs(30));
}

#[test]
fn catalog_cache_config_defaults_disabled_with_direct_db_fallback() {
    let env = std::collections::BTreeMap::<&str, &str>::new();
    let config = catalog_cache_config_from(&|key| env.get(key).map(|value| value.to_string()));

    assert!(!config.enabled);
    assert!(config.fallback_direct_db);
    assert_eq!(
        config.path,
        std::path::PathBuf::from("data/catalog_cache.sqlite")
    );
}

#[test]
fn catalog_cache_config_reads_env() {
    let env = std::collections::BTreeMap::from([
        ("ERP_CATALOG_CACHE_ENABLED", "1"),
        ("ERP_CATALOG_CACHE_FALLBACK_DIRECT_DB", "0"),
        ("ERP_CATALOG_CACHE_PATH", "/tmp/catalog.sqlite"),
    ]);
    let config = catalog_cache_config_from(&|key| env.get(key).map(|value| value.to_string()));

    assert!(config.enabled);
    assert!(!config.fallback_direct_db);
    assert_eq!(config.path, std::path::PathBuf::from("/tmp/catalog.sqlite"));
}

#[test]
fn proc_meminfo_parser_reads_total_memory_bytes() {
    let bytes =
        parse_proc_meminfo_bytes("MemTotal:       16384000 kB\nMemFree: 1 kB\n").expect("memtotal");

    assert_eq!(bytes, 16_384_000 * 1024);
}

#[test]
fn dotenv_persister_upserts_like_go() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(".env");
    std::fs::write(&path, "ERP_URL=https://old.test\nERP_API_KEY=keep\n").expect("write env");
    let persister = DotEnvPersister::new(&path);
    persister
        .upsert(std::collections::BTreeMap::from([
            ("ERP_URL", "https://new.test".to_string()),
            ("ERP_DEFAULT_TARGET_WAREHOUSE", "Stores - CH".to_string()),
        ]))
        .expect("upsert");
    let loaded = dotenvy::from_path_iter(path)
        .expect("read env")
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()
        .expect("parse env");
    assert_eq!(loaded["ERP_URL"], "https://new.test");
    assert_eq!(loaded["ERP_API_KEY"], "keep");
    assert_eq!(loaded["ERP_DEFAULT_TARGET_WAREHOUSE"], "Stores - CH");
}
