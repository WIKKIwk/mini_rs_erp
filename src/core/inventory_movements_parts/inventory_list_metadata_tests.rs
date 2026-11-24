use super::*;

#[tokio::test]
async fn inventory_list_metadata_denies_other_roles_and_unknown_repository_state() {
    let service = InventoryMovementService::new(Arc::new(MemoryInventoryMovementStore::new()));
    let actor = |role| {
        InventoryActor::new(
            Principal {
                role,
                display_name: String::new(),
                legal_name: String::new(),
                ref_: "material".into(),
                phone: String::new(),
                avatar_url: String::new(),
            },
            false,
            ["warehouse".into()],
        )
    };
    let barcodes = vec!["RM-1".into()];
    let apparatus = vec!["apparatus:catalog:pechat-001".into()];
    assert_eq!(
        service
            .list_order_assignments(&actor(PrincipalRole::Werka), &barcodes, &apparatus)
            .await
            .unwrap_err(),
        InventoryMovementError::WarehouseForbidden
    );
    assert!(
        service
            .list_order_assignments(&actor(PrincipalRole::MaterialTaminotchi), &barcodes, &[])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        service
            .list_order_assignments(
                &actor(PrincipalRole::MaterialTaminotchi),
                &barcodes,
                &apparatus
            )
            .await
            .unwrap_err(),
        InventoryMovementError::StoreFailed
    );
}

#[test]
fn inventory_list_metadata_is_opt_in() {
    let query: InventoryAssetQuery = serde_json::from_str("{}").unwrap();
    assert!(!query.include_order_assignment);
    let query: InventoryAssetQuery =
        serde_json::from_str("{\"include_order_assignment\":true}").unwrap();
    assert!(query.include_order_assignment);
}
