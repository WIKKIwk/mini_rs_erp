use super::*;

fn multi_product_body(id: &str) -> serde_json::Value {
    let mut products = crate::core::order_layout::tests::products();
    for (index, product) in products.iter_mut().enumerate() {
        product.item_code = format!("MULTI-{index}");
    }
    let map: serde_json::Value = serde_json::from_str(&pechat_order_map_json(
        id,
        "Mixed order",
        "7778",
        "apparatus:default:bosma_8",
    ))
    .unwrap();
    serde_json::json!({
        "map": map,
        "template": {
            "products": products,
            "customer_ref": "CUST-001",
            "customer": "Mixed order customer",
            "kg": 300,
            "waste_percent": 5,
            "edge_allowance_mm": 15,
            "layers": [{"material": "pet", "micron": "12"}]
        }
    })
}

#[tokio::test]
async fn multi_product_order_save_and_reload_keep_each_product() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let body = multi_product_body("zakaz-mixed-save");
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body.to_string(),
        ))
        .await
        .unwrap();
    let status = response.status();
    let saved = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["saved"]["map"]["width_mm"], 575.0);
    assert_eq!(
        saved["saved"]["map"]["products"],
        body["template"]["products"]
    );
    assert_eq!(saved["template"]["frame_count"], 6.0);
    assert_eq!(saved["template"]["width_mm"], 575.0);
    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps?id=zakaz-mixed-save",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["map"]["products"],
        body["template"]["products"]
    );
}

#[tokio::test]
async fn multi_product_order_rejects_missing_customer_and_foreign_product_without_writes() {
    for missing_customer in [true, false] {
        let state = test_state();
        let token = session(&state, PrincipalRole::Admin).await;
        let mut body = multi_product_body("zakaz-mixed-invalid");
        if missing_customer {
            body["template"]["customer_ref"] = serde_json::json!("");
        } else {
            body["template"]["products"][1]["item_code"] = serde_json::json!("MULTI-FOREIGN");
        }
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps/with-order",
                &token,
                &body.to_string(),
            ))
            .await
            .unwrap();
        let status = response.status();
        let rejected = json_body(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
        assert!(
            state
                .production_maps
                .raw_map("zakaz-mixed-invalid")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .calculate_orders
                .list("admin:19621978")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
