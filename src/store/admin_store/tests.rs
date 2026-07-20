use super::*;

fn test_store_path() -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("mini-rs-erp-item-update-{suffix}.json"))
}

#[tokio::test]
async fn item_update_preserves_assignments_and_creation_time() {
    let path = test_store_path();
    let store = JsonAdminStore::new(path.clone());
    let customer = store
        .create_customer("Customer One", "+998900000001")
        .await
        .expect("customer");
    store
        .create_item("ITEM-OLD", "Old name", "Kg", "Tayyor mahsulot")
        .await
        .expect("item");
    store
        .assign_customer_item(&customer.ref_, "ITEM-OLD")
        .await
        .expect("customer assignment");
    store
        .assign_supplier_item("SUP-001", "ITEM-OLD")
        .await
        .expect("supplier assignment");
    let before = store.item_detail("ITEM-OLD").await.expect("detail");

    let updated = store
        .update_item("ITEM-OLD", "ITEM-NEW", "New name")
        .await
        .expect("update");

    assert_eq!(updated.code, "ITEM-NEW");
    assert_eq!(updated.name, "New name");
    assert!(updated.is_finished_goods);
    assert_eq!(updated.created_at_unix, before.created_at_unix);
    assert!(updated.created_at_unix > 0);
    assert_eq!(updated.customers.len(), 1);
    assert_eq!(updated.customers[0].ref_, customer.ref_);
    let picker_items = store
        .items_page("Customer One", 10, 0)
        .await
        .expect("items searchable by customer");
    assert_eq!(picker_items.len(), 1);
    assert_eq!(picker_items[0].code, "ITEM-NEW");
    assert_eq!(
        picker_items[0].customer_names,
        vec!["Customer One".to_string()]
    );
    assert!(matches!(
        store.item_detail("ITEM-OLD").await,
        Err(AdminPortError::NotFound)
    ));
    assert_eq!(
        store
            .assigned_supplier_items("SUP-001", 10)
            .await
            .expect("supplier items")[0]
            .code,
        "ITEM-NEW"
    );
    assert_eq!(
        store
            .customer_items(&customer.ref_, "", 10)
            .await
            .expect("customer items")[0]
            .code,
        "ITEM-NEW"
    );

    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn item_update_rejects_duplicate_code() {
    let path = test_store_path();
    let store = JsonAdminStore::new(path.clone());
    store
        .create_item("ITEM-ONE", "One", "Kg", "Products")
        .await
        .expect("first item");
    store
        .create_item("ITEM-TWO", "Two", "Kg", "Products")
        .await
        .expect("second item");

    let error = store
        .update_item("ITEM-ONE", "item-two", "Renamed")
        .await
        .expect_err("duplicate code");
    assert!(matches!(
        error,
        AdminPortError::InvalidInput(message) if message == "item code already exists"
    ));
    assert_eq!(
        store.item_detail("ITEM-ONE").await.expect("original").name,
        "One"
    );

    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn item_create_rejects_duplicate_code_without_overwriting_existing_item() {
    let path = test_store_path();
    let store = JsonAdminStore::new(path.clone());
    store
        .create_item("ITEM-ONE", "Original", "Kg", "Products")
        .await
        .expect("first item");

    let error = store
        .create_item("item-one", "Replacement", "Dona", "Tayyor mahsulot")
        .await
        .expect_err("duplicate create");

    assert!(matches!(
        error,
        AdminPortError::InvalidInput(message) if message == "item code already exists"
    ));
    let original = store.item_detail("ITEM-ONE").await.expect("original item");
    assert_eq!(original.name, "Original");
    assert_eq!(original.uom, "Kg");
    assert_eq!(original.item_group, "Products");

    let _ = tokio::fs::remove_file(path).await;
}
