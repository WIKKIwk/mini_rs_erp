use super::*;
use crate::core::qolip::{
    MemoryQolipStore, QolipBlock, QolipProduct, QolipProductSpec, QolipService, QolipStorePort,
};

async fn assert_admin_order_detail_rework(
    router: &axum::Router,
    token: &str,
    order: &str,
    expected: bool,
) {
    for apparatus in ["", "apparatus:default:bosma_7"] {
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/mobile/admin/production-maps/sequence?order_id={order}&apparatus={apparatus}&detail=true"),
                token,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let detail = json_body(response).await;
        assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
        assert_eq!(detail["maps"][0]["qolip_in_rework"], expected);
    }
}

async fn assert_attached_mold_rework(router: &axum::Router, token: &str, expected_codes: &[&str]) {
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/qolip/shared-sets?item_code=ITEM",
            token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let products = body["products"].as_array().unwrap();
    assert_eq!(products.len(), 2);
    for product in products {
        let code = product["qolip_code"].as_str().unwrap();
        assert_eq!(product["is_in_rework"], expected_codes.contains(&code));
    }
}

#[tokio::test]
async fn qolip_rework_blocks_requirements_for_every_order_until_all_codes_return() {
    let store = Arc::new(MemoryQolipStore::new());
    store
        .seed_blocks(vec![QolipBlock {
            name: "Block A".into(),
            warehouse: "Qolip ombor".into(),
        }])
        .await;
    store
        .seed_products(vec![QolipProduct {
            code: "ITEM".into(),
            name: "Label".into(),
            item_group: "Flexo".into(),
            ..Default::default()
        }])
        .await;
    for (code, set) in [("Q-ONE", "950100000001"), ("Q-TWO", "950100000002")] {
        store
            .put_product_spec(QolipProductSpec {
                qolip_set_id: set.into(),
                warehouse: "Qolip ombor".into(),
                item_code: "ITEM".into(),
                item_name: "Label".into(),
                item_group: "Flexo".into(),
                qolip_code: code.into(),
                size: 420,
                color: "BLACK".into(),
                ..Default::default()
            })
            .await
            .unwrap();
    }
    let mut state = test_state();
    state.qolip = QolipService::new(store);
    for order in ["zakaz-rework-first", "zakaz-rework-second"] {
        let map = serde_json::from_str(&production_order_map_json_with_product(
            order,
            "Label",
            "ITEM",
            order,
            "apparatus:default:bosma_7",
            1,
            420.0,
        ))
        .unwrap();
        state.production_maps.upsert_map(map).await.unwrap();
    }
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);
    assert_admin_order_detail_rework(&router, &token, "zakaz-rework-first", false).await;
    assert_attached_mold_rework(&router, &token, &[]).await;
    let sent = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/reworks/send",
            &token,
            r#"{"request_id":"both-away","qolip_codes":["Q-ONE","Q-TWO"]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(sent.status(), StatusCode::OK);
    let entries = json_body(sent).await["reworks"].as_array().unwrap().clone();
    assert_attached_mold_rework(&router, &token, &["Q-ONE", "Q-TWO"]).await;
    for (returned_count, entry) in entries.iter().enumerate() {
        for order in ["zakaz-rework-first", "zakaz-rework-second"] {
            assert_admin_order_detail_rework(&router, &token, order, true).await;
            let body = serde_json::json!({"apparatus":"apparatus:default:bosma_7",
                "order_id":order,"qolip_code":""})
            .to_string();
            let response = router
                .clone()
                .oneshot(request_with_body(
                    "POST",
                    "/v1/mobile/admin/production-maps/qolip-validate",
                    &token,
                    &body,
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::CONFLICT,
                "both orders stay incomplete after {returned_count} returns"
            );
            assert_eq!(json_body(response).await["error"], "qolip_in_rework");
        }
        let body = serde_json::json!({"request_id":format!("return-{returned_count}"),
            "rework_ids":[entry["id"]]})
        .to_string();
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/qolip/reworks/return",
                &token,
                &body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let remaining_codes = entries
            .iter()
            .skip(returned_count + 1)
            .map(|entry| entry["qolip_code"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_attached_mold_rework(&router, &token, &remaining_codes).await;
    }
    for order in ["zakaz-rework-first", "zakaz-rework-second"] {
        assert_admin_order_detail_rework(&router, &token, order, false).await;
        let body = serde_json::json!({"apparatus":"apparatus:default:bosma_7",
            "order_id":order,"qolip_code":""})
        .to_string();
        let response = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/production-maps/qolip-validate",
                &token,
                &body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn qolip_reworks_routes_enforce_access_and_support_partial_return_contract() {
    let store = Arc::new(MemoryQolipStore::new());
    store
        .seed_blocks(vec![QolipBlock {
            name: "Block A".into(),
            warehouse: "Qolip ombor".into(),
        }])
        .await;
    for code in ["Q-ONE", "Q-TWO"] {
        store
            .put_product_spec(QolipProductSpec {
                qolip_set_id: "950100000001".into(),
                warehouse: "Qolip ombor".into(),
                item_code: "ITEM".into(),
                item_name: "Label".into(),
                item_group: "Flexo".into(),
                qolip_code: code.into(),
                size: 420,
                color: "BLACK".into(),
                ..Default::default()
            })
            .await
            .unwrap();
    }
    let mut state = test_state();
    state.qolip = QolipService::new(store);
    let admin = session(&state, PrincipalRole::Admin).await;
    let supplier = session(&state, PrincipalRole::Supplier).await;
    let router = build_router(state);
    let denied = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/reworks/send",
            &supplier,
            r#"{"request_id":"denied","qolip_codes":["Q-ONE"]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let sent = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/reworks/send",
            &admin,
            r#"{"request_id":"send","qolip_codes":["Q-ONE","Q-TWO"]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(sent.status(), StatusCode::OK);
    let sent = json_body(sent).await;
    assert_eq!(sent["reworks"].as_array().unwrap().len(), 2);
    let id = sent["reworks"][0]["id"].as_str().unwrap();
    let body = serde_json::json!({"request_id":"return-one","rework_ids":[id]}).to_string();
    let returned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/reworks/return",
            &admin,
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(returned.status(), StatusCode::OK);
    let returned = json_body(returned).await;
    assert!(returned["reworks"][0]["returned_at"].is_string());
    let open = router
        .clone()
        .oneshot(request_with_body(
            "GET",
            "/v1/mobile/qolip/reworks?status=open",
            &admin,
            "",
        ))
        .await
        .unwrap();
    assert_eq!(
        json_body(open).await["reworks"].as_array().unwrap().len(),
        1
    );
    let invalid = router
        .oneshot(request_with_body(
            "GET",
            "/v1/mobile/qolip/reworks?status=unknown",
            &admin,
            "",
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
}
