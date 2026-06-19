use super::*;

#[tokio::test]
async fn production_map_save_with_order_saves_map_and_template() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json =
        pechat_order_map_json("zakaz-7777", "Atomic zakaz", "7777", "8 ta rangli pechat");
    let body = format!(
        r#"{{
            "map":{map_json},
            "template":{{
                "name":"atomic mahsulot",
                "product":"atomic mahsulot",
                "frame_product_size_mm":635,
                "frame_count":1,
                "waste_percent":5,
                "first_layer_material":"pet",
                "first_layer_micron":"12",
                "second_layer_material":"pe oq",
                "second_layer_micron":"30"
            }}
        }}"#
    );
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("save with order");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["saved"]["map"]["id"], "zakaz-7777");
    assert_eq!(value["template"]["name"], "atomic mahsulot");
    assert_eq!(value["template"]["source_map_id"], "zakaz-7777");
    let template_id = value["template"]["id"]
        .as_str()
        .expect("template id")
        .to_string();
    assert!(!template_id.is_empty());
    assert!(
        value["template"]["code"]
            .as_str()
            .map(|code| !code.trim().is_empty())
            .unwrap_or(false)
    );

    let fetched = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps?id=zakaz-7777",
            &token,
        ))
        .await
        .expect("fetch map by id");
    assert_eq!(fetched.status(), StatusCode::OK);
    let fetched_value = json_body(fetched).await;
    assert_eq!(fetched_value["map"]["id"], "zakaz-7777");

    let cleanup_body = format!(r#"{{"id":"{template_id}"}}"#);
    let cleanup = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/calculate/orders/delete",
            &token,
            &cleanup_body,
        ))
        .await
        .expect("cleanup");
    assert_eq!(cleanup.status(), StatusCode::OK);
}

#[tokio::test]
async fn production_map_save_with_order_records_mini_order_without_blocking_response() {
    let sink = Arc::new(FakeProductionOrderSink::fail_after(Duration::from_millis(
        200,
    )));
    let mut state = test_state();
    state.production_orders = sink.clone();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json =
        pechat_order_map_json("zakaz-7799", "Catalog zakaz", "7799", "8 ta rangli pechat");
    let body = format!(
        r#"{{
            "map":{map_json},
            "template":{{
                "name":"mini mahsulot",
                "product":"mini mahsulot",
                "item_code":"ITEM-MINI",
                "frame_product_size_mm":635,
                "frame_count":1,
                "waste_percent":5,
                "roll_count":7,
                "first_layer_material":"pet",
                "first_layer_micron":"12",
                "second_layer_material":"pe oq",
                "second_layer_micron":"30",
                "kg":500
            }}
        }}"#
    );

    let response = tokio::time::timeout(
        Duration::from_millis(75),
        build_router(state).oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        )),
    )
    .await
    .expect("response must not wait for mini order write")
    .expect("save with order");

    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn production_map_save_with_order_does_not_store_cloned_order_as_quick_template() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json = pechat_order_map_json("zakaz-5555", "Dolce order", "5555", "8 ta rangli pechat");
    let body = format!(
        r#"{{
            "map":{map_json},
            "template":{{
                "id":"",
                "code":"5555",
                "order_number":"5555",
                "name":"dolce cake",
                "product":"dolce cake",
                "item_code":"DOLCE-001",
                "source_map_id":"quick-dolce-map",
                "frame_product_size_mm":715,
                "frame_count":1,
                "waste_percent":5,
                "roll_count":7,
                "first_layer_material":"pet",
                "first_layer_micron":"12",
                "second_layer_material":"pe oq",
                "second_layer_micron":"50",
                "kg":500
            }}
        }}"#
    );

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("save with order");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["saved"]["map"]["id"], "zakaz-5555");
    assert!(value["template"].is_null());

    let list_response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/calculate/orders", &token))
        .await
        .expect("list calculate orders");
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_value = json_body(list_response).await;
    let rows = list_value["templates"].as_array().expect("templates array");
    assert!(rows.iter().all(|row| row["code"] != "5555"));
}

#[tokio::test]
async fn production_map_sequence_round_trips_on_server() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let put = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
            r#"{
                "apparatus":"8 ta rangli pechat",
                "order_ids":["zakaz-1111","zakaz-2222"," "]
            }"#,
        ))
        .await
        .expect("put sequence");
    assert_eq!(put.status(), StatusCode::OK);

    let get = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("get sequence");
    assert_eq!(get.status(), StatusCode::OK);
    let body = json_body(get).await;
    assert_eq!(
        body["sequences"]["8 ta rangli pechat"],
        serde_json::json!(["zakaz-1111", "zakaz-2222"])
    );
}

#[tokio::test]
async fn production_map_calendar_routes_are_removed() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let removed_prefix = ['d', 'a', 'i', 'l', 'y'].iter().collect::<String>();

    for path in [
        format!("/v1/mobile/admin/production-maps/{removed_prefix}-sequence"),
        format!("/v1/mobile/admin/production-maps/{removed_prefix}-apparatus-sequence"),
    ] {
        let response = build_router(state.clone())
            .oneshot(request("GET", &path, &token))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn production_map_save_with_order_rejects_invalid_template_before_saving_map() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json = pechat_order_map_json(
        "zakaz-5555",
        "Invalid template zakaz",
        "5555",
        "8 ta rangli pechat",
    );
    let body = format!(r#"{{"map":{map_json},"template":{{"name":"x","product":""}}}}"#);
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Map must not be saved when the template is invalid.
    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    assert_eq!(
        json_body(list).await.as_array().map(|maps| maps.len()),
        Some(0)
    );
}

#[tokio::test]
async fn production_maps_list_falls_back_to_order_number_as_code() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-3333", "Legacy zakaz", "3333", "8 ta rangli pechat"),
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    assert_eq!(maps[0]["map"]["code"], "3333");
}

#[tokio::test]
async fn production_map_order_number_is_immutable_on_update() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-1234", "Locked zakaz", "1234", "Paket aparat"),
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let changed = pechat_order_map_json("zakaz-1234", "Locked zakaz", "5678", "Paket aparat");
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &changed,
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "order_number_immutable");

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    assert_eq!(maps[0]["map"]["order_number"], "1234");
}

#[tokio::test]
async fn production_map_save_with_order_rolls_back_map_when_template_store_fails() {
    let state = test_state_with_failing_calculate();
    let token = session(&state, PrincipalRole::Admin).await;

    let map_json = pechat_order_map_json("zakaz-8888", "Rollback zakaz", "8888", "Paket aparat");
    let body = format!(
        r#"{{"map":{map_json},"template":{{
            "name":"rollback mahsulot",
            "product":"rollback mahsulot",
            "frame_product_size_mm":635,
            "frame_count":1,
            "waste_percent":5,
            "first_layer_material":"pet",
            "first_layer_micron":"12",
            "second_layer_material":"pe oq",
            "second_layer_micron":"30"
        }}}}"#
    );
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .expect("with-order");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    assert_eq!(
        json_body(list).await.as_array().map(|maps| maps.len()),
        Some(0)
    );
}
