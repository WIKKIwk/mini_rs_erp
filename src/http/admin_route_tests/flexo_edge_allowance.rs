use super::*;

#[tokio::test]
async fn flexo_edge_allowance_calculate_save_and_reload_use_the_same_width() {
    for extra in [0.0, 40.0, 40.5] {
        let state = test_state();
        let mut flexo = TestApparatusSpec::print(
            "apparatus:default:asset-005",
            "Flexo pechat",
            crate::core::apparatus_standard::ProcessTechnology::Flexographic,
            Some(8),
        );
        flexo.min_web_width_mm = Some(400);
        flexo.max_web_width_mm = Some(800);
        state
            .apparatus
            .seed_for_test(
                ApparatusId::new("apparatus:default:asset-005").expect("Flexo ID"),
                canonical_draft(&flexo),
            )
            .await
            .expect("seed Flexo");
        let token = session(&state, PrincipalRole::Admin).await;
        let template = serde_json::json!({
            "name": "Flexo order", "product": "Flexo product", "item_code": "ITEM-FLEXO",
            "status": "Flexo", "frame_product_size_mm": 250.0, "frame_count": 3.0,
            "edge_allowance_mm": extra, "roll_count": 3, "kg": 120.0,
            "waste_percent": 5.0, "layers": [{"material": "pet", "micron": "12"}]
        });
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/calculate",
                &token,
                &template.to_string(),
            ))
            .await
            .expect("calculate");
        let status = response.status();
        let calculation = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{calculation}");
        let width = 750.0 + extra;
        assert_eq!(calculation["width_mm"], width);
        assert_eq!(calculation["edge_allowance_mm"], extra);
        assert_eq!(calculation["status"], "Flexo");

        // The save endpoint must replace the old +15 width with the new calculation.
        let map: serde_json::Value = serde_json::from_str(&production_order_map_json_with_product(
            "zakaz-flexo-edge",
            "Flexo order",
            "ITEM-FLEXO",
            "4400",
            "apparatus:default:asset-005",
            3,
            765.0,
        ))
        .expect("map");
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps/with-order",
                &token,
                &serde_json::json!({"map": map, "template": template}).to_string(),
            ))
            .await
            .expect("save order");
        let status = response.status();
        let saved = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(saved["saved"]["map"]["width_mm"], width);
        assert_eq!(
            saved["saved"]["map"]["base_length"],
            calculation["results"][0]["rounded_length"]
        );
        assert_eq!(saved["template"]["status"], "Flexo");
        assert_eq!(saved["template"]["edge_allowance_mm"], extra);
        assert_eq!(saved["template"]["width_mm"], width);
        let response = build_router(state.clone())
            .oneshot(request("GET", "/v1/mobile/calculate/orders", &token))
            .await
            .expect("reload templates");
        assert_eq!(response.status(), StatusCode::OK);
        let templates = json_body(response).await;
        let reloaded = templates["templates"]
            .as_array()
            .expect("templates")
            .iter()
            .find(|item| item["id"] == saved["template"]["id"])
            .expect("reloaded template");
        assert_eq!(reloaded["status"], "Flexo");
        assert_eq!(reloaded["edge_allowance_mm"], extra);
        assert_eq!(reloaded["width_mm"], width);
        let response = build_router(state)
            .oneshot(request(
                "GET",
                "/v1/mobile/admin/production-maps?id=zakaz-flexo-edge",
                &token,
            ))
            .await
            .expect("reload map");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["map"]["width_mm"], width);
    }
}
