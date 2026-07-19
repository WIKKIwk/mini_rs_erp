use std::sync::Arc;

use crate::config::AppConfig;
use crate::core::admin::service::AdminService;
use crate::store::admin_store::JsonAdminStore;

use crate::core::admin::item_customer_policy::FINISHED_GOODS_CUSTOMER_REQUIRED;
use crate::core::admin::ports::AdminPortError;

fn service_with_store(path: &std::path::Path) -> AdminService {
    let store = Arc::new(JsonAdminStore::new(path.join("admin.json")));
    AdminService::new(&AppConfig {
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
    .with_state_port(store)
}

#[tokio::test]
async fn finished_goods_item_is_assigned_to_customer_on_create() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let service = service_with_store(temp_dir.path());

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

#[tokio::test]
async fn nested_finished_goods_group_requires_customer_and_uses_same_detail_rule() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let service = service_with_store(temp_dir.path());
    service
        .create_item_group("Tayyor mahsulot", "All Item Groups", true)
        .await
        .expect("finished group");
    service
        .create_item_group("Paketlar", "Tayyor mahsulot", true)
        .await
        .expect("child group");

    let error = service
        .create_item("PACK-001", "Package", "dona", "Paketlar", "")
        .await
        .expect_err("nested finished item must require customer");
    assert!(matches!(
        error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));

    let customer = service
        .create_customer("Customer One", "+998901112233")
        .await
        .expect("customer");
    service
        .create_item("PACK-001", "Package", "dona", "Paketlar", &customer.ref_)
        .await
        .expect("finished item with customer");
    let detail = service.item_detail("PACK-001").await.expect("detail");
    assert!(detail.is_finished_goods);
    assert_eq!(detail.customers.len(), 1);

    service
        .create_item_group("Yarim tayyor mahsulot", "All Item Groups", true)
        .await
        .expect("wip group");
    service
        .create_item("WIP-001", "Intermediate", "kg", "Yarim tayyor mahsulot", "")
        .await
        .expect("similar words must not require customer");
    assert!(
        !service
            .item_detail("WIP-001")
            .await
            .expect("wip detail")
            .is_finished_goods
    );
}

#[tokio::test]
async fn last_finished_goods_customer_cannot_be_unassigned() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let service = service_with_store(temp_dir.path());
    service
        .create_item_group("Tayyor mahsulot", "All Item Groups", true)
        .await
        .expect("finished group");
    let first = service
        .create_customer("First Customer", "+998901112233")
        .await
        .expect("first customer");
    let second = service
        .create_customer("Second Customer", "+998901112244")
        .await
        .expect("second customer");
    service
        .create_item(
            "FIN-001",
            "Finished",
            "dona",
            "Tayyor mahsulot",
            &first.ref_,
        )
        .await
        .expect("finished item");

    let error = service
        .unassign_customer_item(&first.ref_, "FIN-001")
        .await
        .expect_err("last customer must be protected");
    assert!(matches!(
        error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));

    service
        .assign_customer_item(&second.ref_, "FIN-001")
        .await
        .expect("second assignment");
    service
        .unassign_customer_item(&first.ref_, "FIN-001")
        .await
        .expect("one of two customers can be removed");
    let detail = service.item_detail("FIN-001").await.expect("detail");
    assert_eq!(detail.customers.len(), 1);
    assert_eq!(detail.customers[0].ref_, second.ref_);
}

#[tokio::test]
async fn customerless_items_cannot_be_reclassified_as_finished_goods() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let service = service_with_store(temp_dir.path());
    service
        .create_item_group("Tayyor mahsulot", "All Item Groups", true)
        .await
        .expect("finished group");
    service
        .create_item_group("Paketlar", "All Item Groups", true)
        .await
        .expect("package group");
    service
        .create_item("PACK-001", "Package", "dona", "Paketlar", "")
        .await
        .expect("ordinary item");

    let bulk_error = service
        .move_items_to_group(vec!["PACK-001".to_string()], "Tayyor mahsulot")
        .await
        .expect_err("bulk move must protect invariant");
    assert!(matches!(
        bulk_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    assert_eq!(
        service
            .item_detail("PACK-001")
            .await
            .expect("detail")
            .item_group,
        "Paketlar"
    );

    let parent_error = service
        .move_item_group_parent("Paketlar", "Tayyor mahsulot")
        .await
        .expect_err("parent move must protect invariant");
    assert!(matches!(
        parent_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    let package_group = service
        .item_group_tree()
        .await
        .expect("tree")
        .into_iter()
        .find(|group| group.name == "Paketlar")
        .expect("package group");
    assert_eq!(package_group.parent_item_group, "All Item Groups");

    let upsert_error = service
        .create_item_group("Paketlar", "Tayyor mahsulot", true)
        .await
        .expect_err("group upsert must not bypass parent protection");
    assert!(matches!(
        upsert_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    let package_group = service
        .item_group_tree()
        .await
        .expect("tree after rejected upsert")
        .into_iter()
        .find(|group| group.name == "Paketlar")
        .expect("package group after rejected upsert");
    assert_eq!(package_group.parent_item_group, "All Item Groups");
}
