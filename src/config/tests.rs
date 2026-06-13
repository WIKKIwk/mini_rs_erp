use super::{DotEnvPersister, parse_bind_addr};
use crate::core::admin::ports::AdminEnvPersister;

#[test]
fn parses_go_style_bind_addr() {
    let addr = parse_bind_addr(":8081").expect("addr");

    assert_eq!(addr.to_string(), "0.0.0.0:8081");
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
