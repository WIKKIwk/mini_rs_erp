use super::*;
use crate::core::qolip::{
    MemoryQolipStore, QolipBlock, QolipLocation, QolipService, QolipStorePort,
};

#[tokio::test]
async fn qolip_block_rename_preserves_existing_locations() {
    let store = Arc::new(MemoryQolipStore::new());
    store
        .seed_blocks(vec![QolipBlock {
            name: "A block".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }])
        .await;
    store
        .put_location(QolipLocation {
            id: "legacy-location-a1".to_string(),
            block: "A block".to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Kross".to_string(),
            qolip_code: "Q-LEGACY".to_string(),
            size: 40,
            quantity: 1,
            row_letter: "A".to_string(),
            column_number: Some(1),
            location_label: "A1".to_string(),
            created_by_role: "qolipchi".to_string(),
            created_by_ref: "qolipchi-1".to_string(),
            created_by_name: "Qolipchi".to_string(),
        })
        .await
        .expect("seed qolip location");

    let mut state = test_state();
    state.qolip = QolipService::new(store.clone());
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/qolip/blocks",
            &token,
            r#"{
                "warehouse":"Qolip ombor",
                "block":"A block",
                "new_block":"B block"
            }"#,
        ))
        .await
        .expect("rename qolip block");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["block"]["name"], "B block");
    assert!(
        store
            .locations("A block")
            .await
            .expect("old block locations")
            .is_empty()
    );
    let locations = store
        .locations("B block")
        .await
        .expect("renamed block locations");
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].qolip_code, "Q-LEGACY");
}
