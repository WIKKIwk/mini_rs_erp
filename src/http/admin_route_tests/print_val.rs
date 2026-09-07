use super::*;

#[tokio::test]
async fn print_val_save_reload_and_clear_preserve_material_width_and_rezka() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let mut map: serde_json::Value = serde_json::from_str(&production_order_map_json_with_product(
        "zakaz-val-850",
        "Val order",
        "ITEM-VAL",
        "8500",
        "apparatus:default:asset-010",
        7,
        655.0,
    ))
    .expect("map");
    map["nodes"].as_array_mut().unwrap().insert(
        1,
        serde_json::json!({
            "id": "print", "kind": "apparatus", "title": "9 ta rangli bosma",
            "apparatus_id": "apparatus:default:bosma_9"
        }),
    );
    map["edges"][0]["to"] = serde_json::json!("print");
    map["edges"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"from": "print", "to": "apparatus"}));
    let mut template = serde_json::json!({
        "name": "Val order", "product": "Val product", "item_code": "ITEM-VAL",
        "frame_product_size_mm": 320.0, "frame_count": 2.0, "roll_count": 7,
        "print_val_size_mm": 850.0, "kg": 500.0, "waste_percent": 5.0,
        "first_layer_material": "pet", "first_layer_micron": "12"
    });
    let mut base_length = serde_json::Value::Null;
    for enabled in [true, false] {
        if !enabled {
            template
                .as_object_mut()
                .unwrap()
                .remove("print_val_size_mm");
        }
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
        let value = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        map = value["saved"]["map"].clone();
        assert_eq!(map["width_mm"], 655.0);
        assert_eq!(map["order_kg"], 500.0);
        let rezka = map["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == "apparatus")
            .unwrap();
        assert_eq!(rezka["rezka_kadr_count"], 2);
        assert_eq!(rezka["rezka_label_length"], 100.0);
        assert_eq!(value["template"]["frame_product_size_mm"], 320.0);
        assert_eq!(value["template"]["frame_count"], 2.0);
        assert_eq!(value["template"]["width_mm"], 655.0);
        let expected = if enabled {
            serde_json::json!(850.0)
        } else {
            serde_json::Value::Null
        };
        assert_eq!(map["print_val_size_mm"], expected);
        assert_eq!(value["template"]["print_val_size_mm"], expected);
        if enabled {
            base_length = map["base_length"].clone();
            assert!(base_length.as_f64().is_some_and(|length| length > 0.0));
        } else {
            assert_eq!(map["base_length"], base_length);
        }
        let response = build_router(state.clone())
            .oneshot(request(
                "GET",
                "/v1/mobile/admin/production-maps?id=zakaz-val-850",
                &token,
            ))
            .await
            .expect("reload");
        assert_eq!(response.status(), StatusCode::OK);
        let loaded = json_body(response).await;
        assert_eq!(loaded["map"]["print_val_size_mm"], expected);
        assert_eq!(loaded["map"]["width_mm"], 655.0);
    }
}

#[tokio::test]
async fn print_val_direct_map_rejects_nonpositive_size() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let mut map: serde_json::Value = serde_json::from_str(&pechat_order_map_json(
        "zakaz-invalid-val",
        "Invalid val",
        "8501",
        "apparatus:default:bosma_9",
    ))
    .unwrap();
    for size in [0.0, -850.0] {
        map["print_val_size_mm"] = serde_json::json!(size);
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &token,
                &map.to_string(),
            ))
            .await
            .expect("invalid val request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["error"], "invalid_print_val_size");
    }
}

#[tokio::test]
async fn print_val_batch_move_uses_saved_override_for_nine_color_press() {
    for val_size in [None, Some(850.0)] {
        let state = test_state();
        let token = session(&state, PrincipalRole::Admin).await;
        let mut map: serde_json::Value = serde_json::from_str(&pechat_order_map_json_with_dims(
            "zakaz-val-move",
            "Val move",
            "8502",
            "apparatus:default:bosma_7",
            7,
            655.0,
        ))
        .unwrap();
        if let Some(size) = val_size {
            map["print_val_size_mm"] = serde_json::json!(size);
        }
        let saved = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &token,
                &map.to_string(),
            ))
            .await
            .expect("save");
        assert_eq!(saved.status(), StatusCode::OK);
        let moved = build_router(state).oneshot(request_with_body(
            "POST", "/v1/mobile/admin/production-maps/move-batch", &token,
            r#"{"from_apparatus":"apparatus:default:bosma_7","to_apparatus":"apparatus:default:bosma_9","map_ids":["zakaz-val-move"]}"#,
        )).await.expect("move");
        let status = moved.status();
        let value = json_body(moved).await;
        if val_size.is_some() {
            assert_eq!(status, StatusCode::OK, "{value}");
        } else {
            assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");
        }
    }
}
