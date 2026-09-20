use mini_rs_erp::config::AppConfig;
use std::process::Command;

#[test]
fn credential_cleanup_preserves_other_quoted_environment_values() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(".env");
    std::fs::write(
        &path,
        "ADMINKA_CODE=test-secret\nKEEP='db$PASSWORD with spaces'\nMULTILINE=\"first\\nsecond\"\n",
    )
    .unwrap();
    mini_rs_erp::config::DotEnvPersister::new(&path)
        .remove_keys(&["ADMINKA_CODE"])
        .unwrap();
    let values = dotenvy::from_path_iter(&path)
        .unwrap()
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()
        .unwrap();
    assert!(!values.contains_key("ADMINKA_CODE"));
    assert_eq!(values["KEEP"], "db$PASSWORD with spaces");
    assert_eq!(values["MULTILINE"], "first\nsecond");
}

#[test]
fn runtime_never_loads_passwords_from_environment() {
    const CASE: &str = "MINI_ERP_ADMIN_CONFIG_TEST_CASE";
    if std::env::var_os(CASE).is_some() {
        let config = AppConfig::from_env().expect("runtime config needs no plaintext credentials");
        assert!(config.admin_code.is_empty());
        assert!(config.werka_code.is_empty());
        assert!(config.material_taminotchi_code.is_empty());
        return;
    }
    for supply_legacy_credentials in [false, true] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "runtime_never_loads_passwords_from_environment",
            "--nocapture",
        ]);
        command.env_clear().env(CASE, "1");
        if supply_legacy_credentials {
            for key in [
                "ADMINKA_CODE",
                "MOBILE_DEV_WERKA_CODE",
                "MOBILE_DEV_MATERIAL_TAMINOTCHI_CODE",
            ] {
                command.env(key, "legacy-test-code");
            }
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
