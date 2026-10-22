use super::{
    DotEnvPersister, http_gzip_level, parse_bind_addr, positive_u64_from_raw, validate_backend_value,
    validate_positive_usize_raw,
};
use crate::core::admin::ports::AdminEnvPersister;
use crate::error::AppError;

#[test]
fn http_gzip_defaults_to_the_calibrated_traffic_profile() {
    assert_eq!(http_gzip_level(None).unwrap(), 8);
    assert_eq!(http_gzip_level(Some("6")).unwrap(), 6);
}

#[test]
fn http_gzip_accepts_only_supported_compression_levels() {
    for level in 0..=9 {
        assert_eq!(http_gzip_level(Some(&level.to_string())).unwrap(), level);
    }
    for value in ["", "-1", "10", "1.5", "fast", "99999999999999999999"] {
        assert!(matches!(
            http_gzip_level(Some(value)),
            Err(AppError::InvalidConfig { key: "MINI_ERP_HTTP_GZIP_LEVEL", .. })
        ));
    }
}

#[test]
fn parses_go_style_bind_addr() {
    let addr = parse_bind_addr(":8081").expect("addr");

    assert_eq!(addr.to_string(), "0.0.0.0:8081");
}

#[test]
fn rejects_invalid_positive_runtime_values_instead_of_defaulting() {
    assert_eq!(
        positive_u64_from_raw("TEST", "720").expect("valid value"),
        720
    );
    assert!(matches!(
        positive_u64_from_raw("MOBILE_API_SESSION_TTL_HOURS", "not-a-number"),
        Err(AppError::InvalidConfig { key, .. }) if key == "MOBILE_API_SESSION_TTL_HOURS"
    ));
    assert!(matches!(
        positive_u64_from_raw("MINI_ERP_HTTP_TIMEOUT_SECONDS", "0"),
        Err(AppError::InvalidConfig { key, .. }) if key == "MINI_ERP_HTTP_TIMEOUT_SECONDS"
    ));
    assert!(matches!(
        validate_backend_value("MOBILE_API_SESSION_STORE_BACKEND", "oracle"),
        Err(AppError::InvalidConfig { key, .. }) if key == "MOBILE_API_SESSION_STORE_BACKEND"
    ));
    assert!(matches!(
        validate_positive_usize_raw("MOBILE_API_SESSION_LMDB_MAP_SIZE_MB", "0"),
        Err(AppError::InvalidConfig { key, .. }) if key == "MOBILE_API_SESSION_LMDB_MAP_SIZE_MB"
    ));
}

#[test]
fn dotenv_persister_upserts_like_go() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(".env");
    std::fs::write(&path, "MINI_ERP_DEFAULT_UOM=Kg\nWERKA_NAME=keep\n").expect("write env");
    let persister = DotEnvPersister::new(&path);
    persister
        .upsert(std::collections::BTreeMap::from([
            (
                "MINI_ERP_DEFAULT_TARGET_WAREHOUSE",
                "Stores - CH".to_string(),
            ),
            ("MINI_ERP_DEFAULT_UOM", "Roll".to_string()),
        ]))
        .expect("upsert");
    let loaded = dotenvy::from_path_iter(path)
        .expect("read env")
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()
        .expect("parse env");
    assert_eq!(loaded["MINI_ERP_DEFAULT_UOM"], "Roll");
    assert_eq!(loaded["WERKA_NAME"], "keep");
    assert_eq!(loaded["MINI_ERP_DEFAULT_TARGET_WAREHOUSE"], "Stores - CH");
}
