use crate::core::admin::models::AdminState;
use crate::core::admin::ports::AdminStatePort;
use crate::core::auth::ports::AdminAccessStateLookup;
use crate::core::werka::ports::WerkaSupplierAdminStateLookup;
use crate::store::admin_state_store::{AdminSupplierStateStore, LmdbAdminSupplierStateStore};

#[tokio::test]
async fn reads_go_admin_supplier_state_shape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("admin.json");
    tokio::fs::write(
        &path,
        r#"{"SUP-001":{"custom_code":"10CUSTOM","blocked":true,"removed":false}}"#,
    )
    .await
    .expect("write state");

    let states = AdminSupplierStateStore::new(path)
        .list_states()
        .await
        .expect("states");

    let state = states.get("SUP-001").expect("supplier state");
    assert_eq!(state.custom_code, "10CUSTOM");
    assert!(state.blocked);
    assert!(!state.removed);
}

#[tokio::test]
async fn reads_go_assigned_item_codes_for_werka_fallback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("admin.json");
    tokio::fs::write(
        &path,
        r#"{"SUP-001":{"assigned_item_codes":["ITEM-001","ITEM-002"],"blocked":false,"removed":false}}"#,
    )
    .await
    .expect("write state");

    let state = AdminSupplierStateStore::new(path)
        .werka_supplier_admin_state("SUP-001")
        .await
        .expect("state");

    assert_eq!(state.assigned_item_codes, ["ITEM-001", "ITEM-002"]);
    assert!(!state.blocked);
    assert!(!state.removed);
}

#[tokio::test]
async fn lmdb_admin_state_round_trips_full_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LmdbAdminSupplierStateStore::open(dir.path().join("admin.lmdb"), 1024 * 1024, None)
        .expect("lmdb store");
    let now = time::OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp");

    store
        .put_state(
            "SUP-001",
            AdminState {
                custom_code: "10CUSTOM".into(),
                blocked: true,
                removed: false,
                assigned_item_codes: vec!["ITEM-001".into(), "ITEM-002".into()],
                cooldown_until: Some(now + time::Duration::minutes(10)),
                regen_window_started_at: Some(now),
                regen_window_count: 3,
                pending_persist_code: "10PENDING".into(),
                pending_persist_at: Some(now + time::Duration::seconds(5)),
                assignments_configured: true,
            },
        )
        .await
        .expect("put state");

    let states = store.states().await.expect("states");
    let state = states.get("SUP-001").expect("state");
    assert_eq!(state.custom_code, "10CUSTOM");
    assert!(state.blocked);
    assert_eq!(state.assigned_item_codes, ["ITEM-001", "ITEM-002"]);
    assert_eq!(state.regen_window_count, 3);
    assert_eq!(state.pending_persist_code, "10PENDING");
    assert!(state.assignments_configured);
    assert_eq!(state.regen_window_started_at, Some(now));
}

#[tokio::test]
async fn lmdb_admin_state_migrates_legacy_json_for_all_ports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("admin.json");
    tokio::fs::write(
        &json_path,
        r#"{"SUP-001":{"custom_code":"10CUSTOM","blocked":true,"removed":false,"assigned_item_codes":["ITEM-001"],"assignments_configured":true}}"#,
    )
    .await
    .expect("write json");

    let lmdb_path = dir.path().join("admin.lmdb");
    let store =
        LmdbAdminSupplierStateStore::open(lmdb_path.clone(), 1024 * 1024, Some(json_path.clone()))
            .expect("lmdb store");

    let access = store.list_states().await.expect("access states");
    assert_eq!(access["SUP-001"].custom_code, "10CUSTOM");
    assert!(access["SUP-001"].blocked);

    let werka = store
        .werka_supplier_admin_state("SUP-001")
        .await
        .expect("werka state");
    assert_eq!(werka.assigned_item_codes, ["ITEM-001"]);
    assert!(werka.blocked);
    drop(store);

    tokio::fs::remove_file(json_path)
        .await
        .expect("remove json");
    let reloaded =
        LmdbAdminSupplierStateStore::open(lmdb_path, 1024 * 1024, None).expect("reload lmdb");
    let states = reloaded.states().await.expect("states");
    assert_eq!(states["SUP-001"].custom_code, "10CUSTOM");
    assert!(states["SUP-001"].assignments_configured);
}
