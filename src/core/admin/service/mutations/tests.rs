use std::sync::Arc;

use crate::config::AppConfig;
use crate::core::admin::service::AdminService;
use crate::store::admin_store::JsonAdminStore;

#[tokio::test]
async fn finished_goods_item_is_assigned_to_customer_on_create() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let store = Arc::new(JsonAdminStore::new(temp_dir.path().join("admin.json")));
    let service = AdminService::new(&AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: "Stores - CH".to_string(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: "Material taminotchisi".to_string(),
        material_taminotchi_phone: String::new(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    })
    .with_read_port(store.clone())
    .with_write_port(store.clone())
    .with_state_port(store);

    let customer = service
        .create_customer("Customer One", "+998901112233")
        .await
        .expect("customer");
    let item = service
        .create_item(
            "ITEM-FINISHED",
            "Finished Item",
            "Kg",
            "tayyor mahsulot",
            &customer.ref_,
        )
        .await
        .expect("item");

    let detail = service
        .customer_detail(&customer.ref_)
        .await
        .expect("customer detail");
    assert!(
        detail
            .assigned_items
            .iter()
            .any(|entry| entry.code == item.code)
    );
}
